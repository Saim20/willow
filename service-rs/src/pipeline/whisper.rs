use anyhow::{Context, Result};
use sherpa_onnx::{
    OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineWhisperModelConfig,
};
use tracing::{info, warn};

use crate::models::{ModelPaths, WhisperModelFiles};
use crate::pipeline::provider::{log_provider_choice, resolve_num_threads, resolve_provider};

/// Offline Whisper ASR — sole command/typing recognizer (paired with Silero VAD).
pub struct WhisperEngine {
    recognizer: Option<OfflineRecognizer>,
    paths: ModelPaths,
    provider: String,
    num_threads: i32,
}

impl WhisperEngine {
    pub fn new(paths: ModelPaths) -> Self {
        Self {
            recognizer: None,
            paths,
            provider: "cpu".into(),
            num_threads: 2,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.recognizer.is_some()
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn initialize(&mut self, requested_provider: &str, num_threads: i32) -> Result<()> {
        let files = self
            .paths
            .find_whisper_model()
            .context("Whisper model not found")?;
        self.num_threads = resolve_num_threads(num_threads);

        let mut last_err = None;
        for provider in resolve_provider(requested_provider) {
            match try_create(&files, provider, self.num_threads) {
                Ok(recognizer) => {
                    self.recognizer = Some(recognizer);
                    self.provider = provider.to_string();
                    log_provider_choice(requested_provider, provider);
                    info!(
                        "Whisper ASR ready ({}, threads={}, provider={})",
                        files.label, self.num_threads, provider
                    );
                    return Ok(());
                }
                Err(e) => {
                    warn!("Whisper init with provider={provider} failed: {e}");
                    last_err = Some(e);
                }
            }
        }
        self.recognizer = None;
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("Whisper init failed")))
    }

    /// Transcribe a full utterance (16 kHz mono f32). Returns empty on failure/junk.
    ///
    /// `pre_pad_secs` of silence is prepended so Whisper does not drop short
    /// leading words (common with offline Whisper packs).
    pub fn transcribe(&self, samples: &[f32], pre_pad_secs: f32) -> String {
        let Some(recognizer) = &self.recognizer else {
            return String::new();
        };
        // Ignore clicks / tiny scraps — they produce hallucinations like "[Music]".
        if samples.len() < 4800 {
            // <300 ms at 16 kHz
            return String::new();
        }
        let pad_n = ((pre_pad_secs.max(0.0)) * 16000.0) as usize;
        let stream = recognizer.create_stream();
        if pad_n > 0 {
            let silence = vec![0.0f32; pad_n];
            stream.accept_waveform(16000, &silence);
        }
        stream.accept_waveform(16000, samples);
        recognizer.decode(&stream);
        let raw = stream
            .get_result()
            .map(|r| r.text.trim().to_string())
            .unwrap_or_default();
        sanitize_transcript(&raw).unwrap_or_default()
    }
}

/// Drop Whisper noise / hallucinations that are useless for commands.
pub fn sanitize_transcript(text: &str) -> Option<String> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();

    // Bracket tags: [Music], [Blank_Audio], [MUSIC], incomplete "[music"
    if lower.contains("[music")
        || lower.contains("[blank")
        || lower.contains("[silence")
        || lower.contains("[applause")
        || lower.contains("[laugh")
        || (t.starts_with('[') && t.contains(']'))
    {
        return None;
    }

    // Common tiny.en garbage on noise
    const JUNK: &[&str] = &[
        "thank you",
        "thank you.",
        "thanks for watching",
        "thanks for watching.",
        "subscribe",
        "you",
        ".",
        "...",
        "hmm",
        "uh",
        "um",
    ];
    if JUNK.iter().any(|j| lower == *j) {
        return None;
    }

    // Mostly non-alphanumeric
    let alpha: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
    if alpha.len() < 2 {
        return None;
    }

    Some(t.to_string())
}

fn try_create(
    files: &WhisperModelFiles,
    provider: &str,
    num_threads: i32,
) -> Result<OfflineRecognizer> {
    let mut config = OfflineRecognizerConfig::default();
    config.feat_config.sample_rate = 16000;
    config.feat_config.feature_dim = 80;
    config.model_config = OfflineModelConfig {
        whisper: OfflineWhisperModelConfig {
            encoder: Some(files.encoder.clone()),
            decoder: Some(files.decoder.clone()),
            language: Some("en".into()),
            task: Some("transcribe".into()),
            tail_paddings: -1,
            ..Default::default()
        },
        tokens: Some(files.tokens.clone()),
        num_threads,
        provider: Some(provider.into()),
        model_type: Some("whisper".into()),
        ..Default::default()
    };
    OfflineRecognizer::create(&config).context(format!("create OfflineRecognizer ({provider})"))
}

#[cfg(test)]
mod tests {
    use super::sanitize_transcript;

    #[test]
    fn drops_music_hallucinations() {
        assert!(sanitize_transcript("[Music]").is_none());
        assert!(sanitize_transcript("[music").is_none());
        assert!(sanitize_transcript("Thank you.").is_none());
        assert_eq!(
            sanitize_transcript("open firefox").as_deref(),
            Some("open firefox")
        );
    }
}
