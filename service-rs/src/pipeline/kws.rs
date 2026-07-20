use std::path::Path;

use anyhow::{Context, Result};
use sherpa_onnx::{
    KeywordSpotter, KeywordSpotterConfig, OnlineModelConfig, OnlineStream,
    OnlineTransducerModelConfig,
};
use tracing::{error, info, warn};

use crate::models::{encode_keywords, keywords_look_encoded, ModelPaths, TransducerModelFiles};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordsSource {
    Encoded,
    Fallback,
    Failed,
}

impl KeywordsSource {
    pub fn as_str(self) -> &'static str {
        match self {
            KeywordsSource::Encoded => "encoded",
            KeywordsSource::Fallback => "fallback",
            KeywordsSource::Failed => "failed",
        }
    }
}

/// Below this RMS (post-AGC) audio is treated as silence for decode scheduling.
const KWS_SILENCE_RMS: f32 = 0.012;
/// Keep decoding after speech so trailing-blank confirmation can fire (~0.6s).
const KWS_DECODE_HANGOVER: u32 = 10;

pub struct KwsEngine {
    spotter: Option<KeywordSpotter>,
    stream: Option<OnlineStream>,
    keywords: Vec<String>,
    threshold: f32,
    paths: ModelPaths,
    enabled: bool,
    keywords_source: KeywordsSource,
    init_error: Option<String>,
    /// Remaining silence chunks to keep decoding after speech.
    decode_hangover: u32,
}

impl KwsEngine {
    pub fn new(paths: ModelPaths) -> Self {
        Self {
            spotter: None,
            stream: None,
            keywords: Vec::new(),
            threshold: 0.25,
            paths,
            enabled: true,
            keywords_source: KeywordsSource::Failed,
            init_error: None,
            decode_hangover: 0,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.spotter.is_some()
    }

    pub fn keywords_source(&self) -> KeywordsSource {
        self.keywords_source
    }

    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn initialize(
        &mut self,
        threshold: f32,
        keywords: &[String],
        requested_provider: &str,
        num_threads: i32,
    ) -> Result<()> {
        self.threshold = threshold;
        self.keywords = keywords.to_vec();
        self.init_error = None;

        let files = self
            .paths
            .find_kws_model()
            .context("KWS model not found")?;

        let (keywords_path, source) = self.write_keywords_file(&files, keywords)?;
        self.keywords_source = source;

        if source == KeywordsSource::Fallback {
            warn!(
                "KWS keyword encoding failed; using default keywords file — \
                 configured hotword may not be active"
            );
        }

        let threads = crate::pipeline::provider::resolve_num_threads(num_threads);
        for provider in crate::pipeline::provider::resolve_provider(requested_provider) {
            let config = build_config(&files, threshold, &keywords_path, provider, threads);
            if let Some(spotter) = KeywordSpotter::create(&config) {
                let stream = spotter.create_stream();
                self.spotter = Some(spotter);
                self.stream = Some(stream);
                crate::pipeline::provider::log_provider_choice(requested_provider, provider);
                info!(
                    "KWS engine initialized (keywords: {}, provider={provider})",
                    source.as_str()
                );
                return Ok(());
            }
            warn!("KWS create failed for provider={provider}");
        }

        self.spotter = None;
        self.stream = None;
        self.keywords_source = KeywordsSource::Failed;
        let msg = "create KeywordSpotter failed".to_string();
        self.init_error.replace(msg.clone());
        error!("{msg}");
        Err(anyhow::anyhow!(msg))
    }

    /// Decode audio and return any keywords detected in this chunk.
    /// Waveform is always accepted; decode runs during speech and a short
    /// post-speech hangover (needed for trailing-blank confirmation), then
    /// idles to keep continuous listening cheap.
    pub fn process_audio(&mut self, chunk: &[f32]) -> Vec<String> {
        let mut detected = Vec::new();
        if !self.enabled || chunk.is_empty() {
            return detected;
        }
        if self.spotter.is_none() || self.stream.is_none() {
            return detected;
        }

        let speaking = chunk_rms(chunk) >= KWS_SILENCE_RMS;
        let should_decode = speaking || self.decode_hangover > 0;
        if speaking {
            self.decode_hangover = KWS_DECODE_HANGOVER;
        } else if self.decode_hangover > 0 {
            self.decode_hangover -= 1;
        }

        let spotter = self.spotter.as_ref().unwrap();
        let stream = self.stream.as_ref().unwrap();
        stream.accept_waveform(16000, chunk);

        if !should_decode {
            return detected;
        }

        while spotter.is_ready(stream) {
            spotter.decode(stream);
            if let Some(result) = spotter.get_result(stream) {
                if !result.keyword.is_empty() {
                    info!("Keyword detected: {}", result.keyword);
                    detected.push(result.keyword);
                    spotter.reset(stream);
                }
            }
        }
        detected
    }

    fn write_keywords_file(
        &self,
        files: &TransducerModelFiles,
        keywords: &[String],
    ) -> Result<(String, KeywordsSource)> {
        let model_dir = Path::new(&files.encoder)
            .parent()
            .context("encoder parent")?;
        let path = model_dir.join("keywords.txt");
        let bpe = model_dir.join("bpe.model");

        let source = if bpe.is_file() {
            if encode_keywords(&files.tokens, bpe.to_str().unwrap(), &path, keywords).is_ok() {
                KeywordsSource::Encoded
            } else {
                error!("keyword encoding failed for {:?}", keywords);
                copy_default_keywords(&path)?;
                KeywordsSource::Fallback
            }
        } else {
            warn!("bpe.model missing; using default keywords");
            copy_default_keywords(&path)?;
            KeywordsSource::Fallback
        };

        if !keywords_look_encoded(&path) {
            anyhow::bail!("keywords file not encoded: {}", path.display());
        }
        Ok((path.to_string_lossy().into_owned(), source))
    }
}

fn chunk_rms(chunk: &[f32]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    let sum: f64 = chunk.iter().map(|s| (*s as f64) * (*s as f64)).sum();
    (sum / chunk.len() as f64).sqrt() as f32
}

fn build_config(
    files: &TransducerModelFiles,
    threshold: f32,
    keywords_file: &str,
    provider: &str,
    num_threads: i32,
) -> KeywordSpotterConfig {
    let mut config = KeywordSpotterConfig::default();
    config.feat_config.sample_rate = 16000;
    config.feat_config.feature_dim = 80;
    config.model_config = OnlineModelConfig {
        transducer: OnlineTransducerModelConfig {
            encoder: Some(files.encoder.clone()),
            decoder: Some(files.decoder.clone()),
            joiner: Some(files.joiner.clone()),
        },
        tokens: Some(files.tokens.clone()),
        num_threads,
        provider: Some(provider.into()),
        ..Default::default()
    };
    // Slight keyword bias + two trailing blanks improve recall without flooding FPs.
    config.max_active_paths = 4;
    config.num_trailing_blanks = 2;
    config.keywords_score = 1.5;
    config.keywords_threshold = threshold;
    config.keywords_file = Some(keywords_file.to_string());
    config
}

fn copy_default_keywords(path: &Path) -> Result<()> {
    for candidate in [
        "/usr/share/willow/kws-default-keywords.txt",
        concat!(env!("CARGO_MANIFEST_DIR"), "/../data/kws-default-keywords.txt"),
    ] {
        let src = Path::new(candidate);
        if src.is_file() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(src, path)?;
            return Ok(());
        }
    }
    error!("no default keywords file found");
    anyhow::bail!("default keywords missing")
}
