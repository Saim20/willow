use anyhow::{Context, Result};
use sherpa_onnx::{
    OnlineModelConfig, OnlineRecognizer, OnlineRecognizerConfig, OnlineStream,
    OnlineTransducerModelConfig,
};
use tracing::info;

use crate::models::{ModelPaths, TransducerModelFiles};
use crate::types::TranscriptionResult;

pub struct AsrEngine {
    recognizer: Option<OnlineRecognizer>,
    stream: Option<OnlineStream>,
    paths: ModelPaths,
    endpoint_silence: f32,
    enabled: bool,
    last_partial: String,
}

impl AsrEngine {
    pub fn new(paths: ModelPaths) -> Self {
        Self {
            recognizer: None,
            stream: None,
            paths,
            endpoint_silence: 0.3,
            enabled: false,
            last_partial: String::new(),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.recognizer.is_some()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn initialize(&mut self, endpoint_silence: f32) -> Result<()> {
        self.endpoint_silence = endpoint_silence;
        let files = self
            .paths
            .find_streaming_model()
            .context("streaming ASR model not found")?;
        let config = build_config(&files, endpoint_silence);
        let recognizer = OnlineRecognizer::create(&config).context("create OnlineRecognizer")?;
        let stream = recognizer.create_stream();
        self.recognizer = Some(recognizer);
        self.stream = Some(stream);
        info!("Streaming ASR initialized (endpoint {endpoint_silence}s)");
        Ok(())
    }

    pub fn set_endpoint_silence(&mut self, seconds: f32) -> Result<()> {
        if (seconds - self.endpoint_silence).abs() < f32::EPSILON {
            return Ok(());
        }
        self.initialize(seconds)
    }

    pub fn reset_stream(&mut self) {
        if let Some(recognizer) = &self.recognizer {
            self.stream = Some(recognizer.create_stream());
        }
        self.last_partial.clear();
    }

    /// Decode audio and return any new partial/final transcription results.
    pub fn process_audio(&mut self, chunk: &[f32]) -> Vec<TranscriptionResult> {
        let mut results = Vec::new();
        if !self.enabled || chunk.is_empty() {
            return results;
        }
        let (Some(recognizer), Some(stream)) = (&self.recognizer, &self.stream) else {
            return results;
        };

        stream.accept_waveform(16000, chunk);
        while recognizer.is_ready(stream) {
            recognizer.decode(stream);
        }

        if let Some(result) = recognizer.get_result(stream) {
            let text = result.text.trim().to_string();
            if !text.is_empty() && text != self.last_partial {
                self.last_partial = text.clone();
                results.push(TranscriptionResult {
                    text: text.clone(),
                    is_final: false,
                    is_endpoint: false,
                });
            }

            if recognizer.is_endpoint(stream) {
                if !text.is_empty() {
                    results.push(TranscriptionResult {
                        text,
                        is_final: true,
                        is_endpoint: true,
                    });
                }
                recognizer.reset(stream);
                self.last_partial.clear();
            }
        }
        results
    }
}

fn build_config(files: &TransducerModelFiles, endpoint_silence: f32) -> OnlineRecognizerConfig {
    let mut config = OnlineRecognizerConfig::default();
    config.feat_config.sample_rate = 16000;
    config.feat_config.feature_dim = 80;
    config.model_config = OnlineModelConfig {
        transducer: OnlineTransducerModelConfig {
            encoder: Some(files.encoder.clone()),
            decoder: Some(files.decoder.clone()),
            joiner: Some(files.joiner.clone()),
        },
        tokens: Some(files.tokens.clone()),
        num_threads: 1,
        provider: Some("cpu".into()),
        ..Default::default()
    };
    config.decoding_method = Some("greedy_search".into());
    config.enable_endpoint = true;
    config.rule1_min_trailing_silence = endpoint_silence;
    config.rule2_min_trailing_silence = endpoint_silence * 0.75;
    config.rule3_min_utterance_length = 5.0;
    config
}
