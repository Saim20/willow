use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use sherpa_onnx::{
    OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineStream,
    OnlineTransducerModelConfig,
};
use tracing::{info, warn};

use crate::commands::phrase_index::normalize;
use crate::models::{ModelPaths, TransducerModelFiles};
use crate::pipeline::provider::{log_provider_choice, resolve_num_threads, resolve_provider};
use crate::types::TranscriptionResult;

const STABILITY_REEMIT: Duration = Duration::from_millis(200);

/// Streaming zipformer/transducer ASR for low-latency command recognition.
pub struct StreamAsrEngine {
    recognizer: Option<OnlineRecognizer>,
    stream: Option<OnlineStream>,
    paths: ModelPaths,
    provider: String,
    num_threads: i32,
    enabled: bool,
    last_text: String,
    text_changed_at: Option<Instant>,
    last_stable_emit_at: Option<Instant>,
    rule1_silence: f32,
    rule2_silence: f32,
}

impl StreamAsrEngine {
    pub fn new(paths: ModelPaths) -> Self {
        Self {
            recognizer: None,
            stream: None,
            paths,
            provider: "cpu".into(),
            num_threads: 2,
            enabled: false,
            last_text: String::new(),
            text_changed_at: None,
            last_stable_emit_at: None,
            rule1_silence: 2.4,
            rule2_silence: 0.8,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.recognizer.is_some()
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.reset();
        }
    }

    pub fn set_endpoint_rules(&mut self, rule1: f32, rule2: f32) {
        self.rule1_silence = rule1.clamp(0.5, 8.0);
        self.rule2_silence = rule2.clamp(0.2, 4.0);
    }

    pub fn initialize(
        &mut self,
        requested_provider: &str,
        num_threads: i32,
        rule1: f32,
        rule2: f32,
    ) -> Result<()> {
        self.num_threads = resolve_num_threads(num_threads);
        self.set_endpoint_rules(rule1, rule2);
        let files = self
            .paths
            .find_streaming_asr_model()
            .context("Streaming ASR model not found (asr-stream)")?;

        let mut last_err = None;
        for provider in resolve_provider(requested_provider) {
            match try_create(&files, provider, self.num_threads, self.rule1_silence, self.rule2_silence)
            {
                Ok((recognizer, stream)) => {
                    self.recognizer = Some(recognizer);
                    self.stream = Some(stream);
                    self.provider = provider.to_string();
                    self.last_text.clear();
                    self.text_changed_at = None;
                    self.last_stable_emit_at = None;
                    log_provider_choice(requested_provider, provider);
                    info!(
                        "Streaming ASR ready (provider={provider}, threads={})",
                        self.num_threads
                    );
                    return Ok(());
                }
                Err(e) => {
                    warn!("Streaming ASR init with provider={provider} failed: {e}");
                    last_err = Some(e);
                }
            }
        }
        self.recognizer = None;
        self.stream = None;
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Streaming ASR init failed")))
    }

    pub fn reset(&mut self) {
        if let (Some(recognizer), Some(stream)) = (&self.recognizer, &self.stream) {
            recognizer.reset(stream);
        }
        self.last_text.clear();
        self.text_changed_at = None;
        self.last_stable_emit_at = None;
    }

    /// Feed audio; return a transcription update when the hypothesis changes, stabilizes, or endpoint fires.
    pub fn process_audio(&mut self, chunk: &[f32]) -> Option<TranscriptionResult> {
        if !self.enabled || chunk.is_empty() {
            return None;
        }
        let Some(recognizer) = &self.recognizer else {
            return None;
        };
        let Some(stream) = &self.stream else {
            return None;
        };

        stream.accept_waveform(16000, chunk);
        while recognizer.is_ready(stream) {
            recognizer.decode(stream);
        }

        let text = recognizer
            .get_result(stream)
            .map(|r| normalize(&r.text))
            .unwrap_or_default();
        let endpoint = recognizer.is_endpoint(stream);

        if endpoint {
            let final_text = text.clone();
            recognizer.reset(stream);
            self.last_text.clear();
            self.text_changed_at = None;
            self.last_stable_emit_at = None;
            if final_text.is_empty() {
                return None;
            }
            return Some(TranscriptionResult {
                text: final_text,
                is_final: true,
                is_endpoint: true,
                from_whisper: false,
                is_stable: true,
            });
        }

        if text.is_empty() {
            return None;
        }

        if text != self.last_text {
            self.last_text = text.clone();
            self.text_changed_at = Some(Instant::now());
            self.last_stable_emit_at = None;
            return Some(TranscriptionResult {
                text,
                is_final: false,
                is_endpoint: false,
                from_whisper: false,
                is_stable: false,
            });
        }

        // Re-emit while hypothesis is unchanged so session fills can commit after a hold.
        let stable_long_enough = self
            .text_changed_at
            .is_some_and(|t| t.elapsed() >= STABILITY_REEMIT);
        if stable_long_enough {
            let due = self
                .last_stable_emit_at
                .is_none_or(|t| t.elapsed() >= STABILITY_REEMIT);
            if due {
                self.last_stable_emit_at = Some(Instant::now());
                return Some(TranscriptionResult {
                    text,
                    is_final: false,
                    is_endpoint: false,
                    from_whisper: false,
                    is_stable: true,
                });
            }
        }

        None
    }
}

fn try_create(
    files: &TransducerModelFiles,
    provider: &str,
    num_threads: i32,
    rule1: f32,
    rule2: f32,
) -> Result<(OnlineRecognizer, OnlineStream)> {
    // Wrong model_type aborts inside sherpa (not a recoverable Result). Sniff encoder.
    let model_type = detect_streaming_model_type(&files.encoder);
    let mut config = OnlineRecognizerConfig::default();
    config.feat_config.sample_rate = 16000;
    config.feat_config.feature_dim = 80;
    config.decoding_method = Some("greedy_search".into());
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = rule1;
    config.rule2_min_trailing_silence = rule2;
    config.rule3_min_utterance_length = 20.0;
    config.model_config = OnlineModelConfig {
        transducer: OnlineTransducerModelConfig {
            encoder: Some(files.encoder.clone()),
            decoder: Some(files.decoder.clone()),
            joiner: Some(files.joiner.clone()),
        },
        tokens: Some(files.tokens.clone()),
        num_threads,
        provider: Some(provider.into()),
        model_type: Some(model_type.into()),
        ..Default::default()
    };

    let recognizer = OnlineRecognizer::create(&config)
        .ok_or_else(|| anyhow::anyhow!("OnlineRecognizer::create returned null"))?;
    let stream = recognizer.create_stream();
    Ok((recognizer, stream))
}

/// Sherpa aborts if zipformer2 is forced on a zipformer-v1 pack (missing query_head_dims).
fn detect_streaming_model_type(encoder_path: &str) -> &'static str {
    let Ok(bytes) = std::fs::read(encoder_path) else {
        return "zipformer";
    };
    if bytes.windows(b"query_head_dims".len()).any(|w| w == b"query_head_dims") {
        return "zipformer2";
    }
    "zipformer"
}
