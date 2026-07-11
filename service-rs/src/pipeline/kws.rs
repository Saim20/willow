use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use sherpa_onnx::{
    KeywordSpotter, KeywordSpotterConfig, OnlineModelConfig, OnlineStream,
    OnlineTransducerModelConfig,
};
use tracing::{error, info};

use crate::models::{encode_keywords, keywords_look_encoded, ModelPaths, TransducerModelFiles};

pub struct KwsEngine {
    spotter: Option<KeywordSpotter>,
    stream: Option<OnlineStream>,
    keywords: Vec<String>,
    threshold: f32,
    paths: ModelPaths,
    enabled: bool,
    on_keyword: Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>,
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
            on_keyword: Mutex::new(None),
        }
    }

    pub fn set_callback<F>(&self, f: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        *self.on_keyword.lock().unwrap() = Some(Box::new(f));
    }

    pub fn is_loaded(&self) -> bool {
        self.spotter.is_some()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn initialize(&mut self, threshold: f32, keywords: &[String]) -> Result<()> {
        self.threshold = threshold;
        self.keywords = keywords.to_vec();

        let files = self
            .paths
            .find_kws_model()
            .context("KWS model not found")?;

        let keywords_path = self.write_keywords_file(&files, keywords)?;
        let config = build_config(&files, threshold, &keywords_path);

        let spotter = KeywordSpotter::create(&config).context("create KeywordSpotter")?;
        let stream = spotter.create_stream();
        self.spotter = Some(spotter);
        self.stream = Some(stream);
        info!("KWS engine initialized");
        Ok(())
    }

    pub fn process_audio(&self, chunk: &[f32]) {
        if !self.enabled || chunk.is_empty() {
            return;
        }
        let (Some(spotter), Some(stream)) = (&self.spotter, &self.stream) else {
            return;
        };

        stream.accept_waveform(16000, chunk);
        while spotter.is_ready(stream) {
            spotter.decode(stream);
            if let Some(result) = spotter.get_result(stream) {
                if !result.keyword.is_empty() {
                    info!("Keyword detected: {}", result.keyword);
                    if let Some(cb) = self.on_keyword.lock().unwrap().as_ref() {
                        cb(&result.keyword);
                    }
                    spotter.reset(stream);
                }
            }
        }
    }

    fn write_keywords_file(&self, files: &TransducerModelFiles, keywords: &[String]) -> Result<String> {
        let model_dir = Path::new(&files.encoder)
            .parent()
            .context("encoder parent")?;
        let path = model_dir.join("keywords.txt");
        let bpe = model_dir.join("bpe.model");

        if bpe.is_file() {
            if encode_keywords(&files.tokens, bpe.to_str().unwrap(), &path, keywords).is_err() {
                copy_default_keywords(&path)?;
            }
        } else {
            copy_default_keywords(&path)?;
        }

        if !keywords_look_encoded(&path) {
            anyhow::bail!("keywords file not encoded: {}", path.display());
        }
        Ok(path.to_string_lossy().into_owned())
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
        num_threads: 2,
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
