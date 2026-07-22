use anyhow::{Context, Result};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use tracing::{info, warn};

use crate::models::ModelPaths;
use crate::pipeline::provider::{resolve_num_threads, resolve_provider};

/// Silero VAD window size (samples) required by sherpa-onnx's 16 kHz model.
const WINDOW_SIZE: i32 = 512;

pub struct VadEngine {
    vad: Option<VoiceActivityDetector>,
    paths: ModelPaths,
    provider: String,
    min_silence: f32,
    min_speech: f32,
    threshold: f32,
    enabled: bool,
}

impl VadEngine {
    pub fn new(paths: ModelPaths) -> Self {
        Self {
            vad: None,
            paths,
            provider: "cpu".into(),
            min_silence: 0.35,
            min_speech: 0.08,
            threshold: 0.4,
            enabled: false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.vad.is_some()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.reset();
        }
    }

    pub fn initialize(
        &mut self,
        min_silence: f32,
        min_speech: f32,
        threshold: f32,
        requested_provider: &str,
        num_threads: i32,
    ) -> Result<()> {
        self.min_silence = min_silence.clamp(0.15, 2.0);
        self.min_speech = min_speech.clamp(0.05, 1.0);
        self.threshold = threshold.clamp(0.15, 0.9);
        let model = self
            .paths
            .find_vad_model()
            .context("Silero VAD model not found")?;
        let threads = resolve_num_threads(num_threads);

        // VAD is tiny — prefer CPU even when CUDA is available (less GPU contention for Whisper).
        let providers = if requested_provider.eq_ignore_ascii_case("cuda") {
            resolve_provider(requested_provider)
        } else {
            vec!["cpu"]
        };

        for provider in providers {
            let config = VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(model.clone()),
                    threshold: self.threshold,
                    min_silence_duration: self.min_silence,
                    min_speech_duration: self.min_speech,
                    window_size: WINDOW_SIZE,
                    max_speech_duration: 25.0,
                },
                sample_rate: 16000,
                num_threads: threads,
                provider: Some(provider.into()),
                debug: false,
                ..Default::default()
            };
            if let Some(vad) = VoiceActivityDetector::create(&config, 30.0) {
                self.vad = Some(vad);
                self.provider = provider.to_string();
                // VAD stays on CPU by design (tiny model; frees GPU for Whisper).
                info!(
                    "Silero VAD ready (silence={}s, min_speech={}s, threshold={}, provider={provider})",
                    self.min_silence, self.min_speech, self.threshold
                );
                return Ok(());
            }
            warn!("VAD create failed for provider={provider}");
        }
        self.vad = None;
        anyhow::bail!("create VoiceActivityDetector failed")
    }

    pub fn set_min_silence(&mut self, seconds: f32) {
        let seconds = seconds.clamp(0.15, 2.0);
        if (seconds - self.min_silence).abs() < 0.01 {
            return;
        }
        let provider = self.provider.clone();
        let _ = self.initialize(seconds, self.min_speech, self.threshold, &provider, 0);
    }

    pub fn reset(&mut self) {
        if let Some(vad) = &self.vad {
            vad.clear();
            vad.reset();
        }
    }

    /// Feed audio; return completed speech segments (copied samples).
    pub fn process_audio(&mut self, chunk: &[f32]) -> Vec<Vec<f32>> {
        let mut segments = Vec::new();
        if !self.enabled || chunk.is_empty() {
            return segments;
        }
        let Some(vad) = &self.vad else {
            return segments;
        };

        vad.accept_waveform(chunk);
        Self::drain_segments(vad, &mut segments);
        segments
    }

    fn drain_segments(vad: &VoiceActivityDetector, segments: &mut Vec<Vec<f32>>) {
        while !vad.is_empty() {
            if let Some(seg) = vad.front() {
                let samples = seg.samples().to_vec();
                drop(seg);
                vad.pop();
                // ≥50 ms at 16 kHz — short command words need a low floor.
                if samples.len() >= 800 {
                    segments.push(samples);
                }
            } else {
                break;
            }
        }
    }
}
