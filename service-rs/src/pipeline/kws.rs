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

pub struct KwsEngine {
    spotter: Option<KeywordSpotter>,
    stream: Option<OnlineStream>,
    keywords: Vec<String>,
    threshold: f32,
    paths: ModelPaths,
    enabled: bool,
    keywords_source: KeywordsSource,
    init_error: Option<String>,
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

    pub fn initialize(&mut self, threshold: f32, keywords: &[String]) -> Result<()> {
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

        let config = build_config(&files, threshold, &keywords_path);

        match KeywordSpotter::create(&config) {
            Some(spotter) => {
                let stream = spotter.create_stream();
                self.spotter = Some(spotter);
                self.stream = Some(stream);
                info!("KWS engine initialized (keywords: {})", source.as_str());
                Ok(())
            }
            None => {
                self.spotter = None;
                self.stream = None;
                self.keywords_source = KeywordsSource::Failed;
                let msg = "create KeywordSpotter failed".to_string();
                self.init_error = Some(msg.clone());
                error!("{msg}");
                Err(anyhow::anyhow!(msg))
            }
        }
    }

    /// Decode audio and return any keywords detected in this chunk.
    pub fn process_audio(&self, chunk: &[f32]) -> Vec<String> {
        let mut detected = Vec::new();
        if !self.enabled || chunk.is_empty() {
            return detected;
        }
        let (Some(spotter), Some(stream)) = (&self.spotter, &self.stream) else {
            return detected;
        };

        stream.accept_waveform(16000, chunk);
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

fn build_config(files: &TransducerModelFiles, threshold: f32, keywords_file: &str) -> KeywordSpotterConfig {
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
        num_threads: 1,
        provider: Some("cpu".into()),
        ..Default::default()
    };
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
