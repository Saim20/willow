use anyhow::Result;
use std::sync::Arc;
use tracing::{error, info, warn};

use super::kws::{KeywordsSource, KwsEngine};
use super::speaker::SpeakerVerifier;
use super::stream_asr::StreamAsrEngine;
use super::vad::VadEngine;
use super::whisper::WhisperEngine;
use crate::audio::{Agc, AudioRouter};
use crate::commands::phrase_index::kws_keyword_matches;
use crate::config::WillowConfig;
use crate::models::ModelPaths;
use crate::types::{Mode, TranscriptionResult};

/// Side effects produced while processing one audio chunk (no re-entrant callbacks).
#[derive(Debug, Default)]
pub struct PipelineEvents {
    /// Mode/command keywords to hand to the mode state machine ("command", "normal", …).
    pub keywords: Vec<String>,
    pub speaker_fails: Vec<String>,
    pub command_pending: Vec<(String, bool)>,
    /// Typing-mode VAD segments — Whisper outside the service mutex.
    pub speech_segments: Vec<Vec<f32>>,
    /// Command-mode streaming ASR updates (partials / endpoints).
    pub stream_results: Vec<TranscriptionResult>,
}

pub struct SpeechPipeline {
    pub kws: KwsEngine,
    pub vad: VadEngine,
    pub stream_asr: StreamAsrEngine,
    pub whisper: Arc<WhisperEngine>,
    pub speaker: SpeakerVerifier,
    pub router: AudioRouter,
    agc: Agc,
    last_chunk: Vec<f32>,
    mode: Mode,
    hotword: String,
    config: WillowConfig,
    kws_ready: bool,
    asr_ready: bool,
    stream_ready: bool,
    init_error: Option<String>,
}

impl SpeechPipeline {
    pub fn new(paths: ModelPaths) -> Self {
        Self {
            kws: KwsEngine::new(paths.clone()),
            vad: VadEngine::new(paths.clone()),
            stream_asr: StreamAsrEngine::new(paths.clone()),
            whisper: Arc::new(WhisperEngine::new(paths.clone())),
            speaker: SpeakerVerifier::new(&paths),
            router: AudioRouter::new(),
            agc: Agc::new(),
            last_chunk: Vec::new(),
            mode: Mode::Normal,
            hotword: "hey willow".into(),
            config: WillowConfig::default(),
            kws_ready: false,
            asr_ready: false,
            stream_ready: false,
            init_error: None,
        }
    }

    pub fn initialize(&mut self, config: &WillowConfig) -> Result<()> {
        self.config = config.clone();
        self.hotword = config.hotword.clone();
        self.init_error = None;

        let provider = &config.inference.provider;
        let threads = config.inference.num_threads;

        let kws_ok = self
            .kws
            .initialize(config.kws.threshold, &config.kws_phrases(), provider, threads)
            .is_ok();
        if !kws_ok {
            self.init_error = self
                .kws
                .init_error()
                .map(|s| s.to_string())
                .or_else(|| Some("KWS initialization failed".into()));
        }

        let vad_ok = self
            .vad
            .initialize(
                config.command_mode.endpoint_silence,
                config.command_mode.min_speech_duration,
                config.command_mode.vad_threshold,
                provider,
                threads,
            )
            .is_ok();

        let stream_ok = self
            .stream_asr
            .initialize(
                provider,
                threads,
                config.streaming_asr.rule1_min_trailing_silence,
                config.streaming_asr.rule2_min_trailing_silence,
            )
            .is_ok();

        let mut whisper = WhisperEngine::new(ModelPaths::from_home());
        let whisper_ok = match whisper.initialize(provider, threads) {
            Ok(()) => true,
            Err(e) => {
                warn!("Whisper init failed: {e}");
                false
            }
        };
        self.whisper = Arc::new(whisper);

        if kws_ok && (!vad_ok || !whisper_ok || !stream_ok) {
            let mut parts = Vec::new();
            if !vad_ok {
                parts.push("VAD model not found");
            }
            if !whisper_ok {
                parts.push("Whisper model not found");
            }
            if !stream_ok {
                parts.push("Streaming ASR model not found (asr-stream)");
            }
            let msg = parts.join("; ");
            self.init_error = Some(
                self.init_error
                    .take()
                    .map(|e| format!("{e}; {msg}"))
                    .unwrap_or(msg),
            );
        }

        let speaker_ok = if config.speaker_verification.enabled {
            self.speaker
                .initialize(&ModelPaths::from_home(), provider, threads)
                .is_ok()
        } else {
            true
        };

        self.speaker.set_enabled(config.speaker_verification.enabled);
        self.speaker
            .set_threshold(config.speaker_verification.threshold);
        self.speaker
            .set_enrolled_user(&config.speaker_verification.enrolled_user);

        self.kws_ready = kws_ok;
        self.asr_ready = self.vad.is_loaded() && self.whisper.is_loaded();
        self.stream_ready = self.stream_asr.is_loaded();
        if config.speaker_verification.enabled && !speaker_ok {
            error!("Speaker model unavailable");
        }
        if self.stream_ready {
            info!(
                "Command streaming ASR ready (provider={})",
                self.stream_asr.provider()
            );
        }
        if self.asr_ready {
            info!(
                "Typing ASR ready (VAD + Whisper, provider={})",
                self.whisper.provider()
            );
        }
        Ok(())
    }

    pub fn ready_for_normal(&self) -> bool {
        self.kws_ready
    }

    /// Command mode needs streaming ASR (partials + early fire).
    pub fn ready_for_command(&self) -> bool {
        self.kws_ready && self.stream_ready
    }

    /// Typing / dictation needs offline Whisper + VAD.
    pub fn ready_for_typing(&self) -> bool {
        self.kws_ready && self.asr_ready
    }

    pub fn kws_ready(&self) -> bool {
        self.kws_ready
    }

    pub fn asr_ready(&self) -> bool {
        self.asr_ready
    }

    pub fn stream_ready(&self) -> bool {
        self.stream_ready
    }

    pub fn whisper_ready(&self) -> bool {
        self.whisper.is_loaded()
    }

    pub fn whisper_handle(&self) -> Arc<WhisperEngine> {
        Arc::clone(&self.whisper)
    }

    pub fn active_provider(&self) -> &str {
        if self.stream_ready {
            self.stream_asr.provider()
        } else {
            self.whisper.provider()
        }
    }

    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    pub fn keywords_source(&self) -> KeywordsSource {
        self.kws.keywords_source()
    }

    pub fn reset_listening(&mut self) {
        self.vad.reset();
        self.stream_asr.reset();
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.vad.reset();
        self.stream_asr.reset();
        match mode {
            Mode::Normal => {
                self.vad.set_enabled(false);
                self.stream_asr.set_enabled(false);
                self.kws.set_enabled(true);
            }
            Mode::Command => {
                self.kws.set_enabled(true);
                // VAD+Whisper for search (and other utterance finals); stream for live HUD / early commands.
                if self.asr_ready {
                    // Slightly longer than bare command silence so search queries finish cleanly.
                    let silence = self
                        .config
                        .command_mode
                        .endpoint_silence
                        .max(0.45)
                        .min(1.2);
                    self.vad.set_min_silence(silence);
                    self.vad.set_enabled(true);
                } else {
                    self.vad.set_enabled(false);
                }
                self.stream_asr.set_enabled(self.stream_ready);
            }
            Mode::Typing => {
                self.kws.set_enabled(true);
                self.stream_asr.set_enabled(false);
                if self.asr_ready {
                    self.vad
                        .set_min_silence(self.config.streaming_asr.endpoint_silence_typing);
                    self.vad.set_enabled(true);
                } else {
                    self.vad.set_enabled(false);
                }
            }
        }
    }

    pub fn apply_config(&mut self, config: &WillowConfig) -> Result<()> {
        let provider_changed = config.inference.provider != self.config.inference.provider
            || config.inference.num_threads != self.config.inference.num_threads;
        let stream_timing_changed = (config.streaming_asr.rule1_min_trailing_silence
            - self.config.streaming_asr.rule1_min_trailing_silence)
            .abs()
            > 0.01
            || (config.streaming_asr.rule2_min_trailing_silence
                - self.config.streaming_asr.rule2_min_trailing_silence)
                .abs()
                > 0.01;
        let cmd_timing_changed = (config.command_mode.endpoint_silence
            - self.config.command_mode.endpoint_silence)
            .abs()
            > 0.01
            || (config.command_mode.min_speech_duration - self.config.command_mode.min_speech_duration)
                .abs()
                > 0.01
            || (config.command_mode.vad_threshold - self.config.command_mode.vad_threshold).abs()
                > 0.01;

        self.config = config.clone();
        self.hotword = config.hotword.clone();
        self.speaker.set_enabled(config.speaker_verification.enabled);
        self.speaker
            .set_threshold(config.speaker_verification.threshold);

        let provider = &config.inference.provider;
        let threads = config.inference.num_threads;
        let kws_ok = self
            .kws
            .initialize(config.kws.threshold, &config.kws_phrases(), provider, threads)
            .is_ok();
        self.kws_ready = kws_ok;

        if provider_changed || cmd_timing_changed {
            let _ = self.vad.initialize(
                config.command_mode.endpoint_silence,
                config.command_mode.min_speech_duration,
                config.command_mode.vad_threshold,
                provider,
                threads,
            );
        }
        if provider_changed || stream_timing_changed {
            let _ = self.stream_asr.initialize(
                provider,
                threads,
                config.streaming_asr.rule1_min_trailing_silence,
                config.streaming_asr.rule2_min_trailing_silence,
            );
        }
        if provider_changed {
            let mut whisper = WhisperEngine::new(ModelPaths::from_home());
            let whisper_ok = whisper.initialize(provider, threads).is_ok();
            self.whisper = Arc::new(whisper);
            self.asr_ready = self.vad.is_loaded() && whisper_ok;
            if config.speaker_verification.enabled {
                let _ = self
                    .speaker
                    .initialize(&ModelPaths::from_home(), provider, threads);
            }
        } else {
            self.asr_ready = self.vad.is_loaded() && self.whisper.is_loaded();
        }
        self.stream_ready = self.stream_asr.is_loaded();

        if !kws_ok {
            self.init_error = self
                .kws
                .init_error()
                .map(|s| s.to_string())
                .or_else(|| Some("KWS re-initialization failed".into()));
        } else if self.stream_ready || self.asr_ready {
            self.init_error = None;
        }
        Ok(())
    }

    /// AGC + KWS + streaming ASR (command) or VAD (typing). Whisper runs later for typing.
    pub fn process_audio(&mut self, chunk: &[f32]) -> PipelineEvents {
        let mut events = PipelineEvents::default();

        self.last_chunk = self.agc.process(chunk);
        if !self.kws_ready {
            return events;
        }

        let chunk = self.last_chunk.clone();
        self.router.push_chunk(&chunk);

        if self.kws.is_loaded() {
            for keyword in self.kws.process_audio(&chunk) {
                self.handle_keyword_detected(&keyword, &mut events);
            }
        }

        match self.mode {
            Mode::Command => {
                if self.stream_ready {
                    if let Some(result) = self.stream_asr.process_audio(&chunk) {
                        events.stream_results.push(result);
                    }
                }
                // Parallel VAD for Whisper search (and other) finals.
                if self.asr_ready && self.vad.is_loaded() {
                    let raw = self.vad.process_audio(&chunk);
                    let preroll = self.config.command_mode.preroll;
                    events.speech_segments = raw
                        .into_iter()
                        .map(|seg| self.with_preroll(seg, preroll))
                        .collect();
                }
            }
            Mode::Typing => {
                if self.asr_ready {
                    let raw = self.vad.process_audio(&chunk);
                    let preroll = self.config.command_mode.preroll;
                    events.speech_segments = raw
                        .into_iter()
                        .map(|seg| self.with_preroll(seg, preroll))
                        .collect();
                }
            }
            Mode::Normal => {}
        }
        events
    }

    pub fn whisper_pre_pad(&self) -> f32 {
        self.config.command_mode.whisper_pre_pad
    }

    fn with_preroll(&self, segment: Vec<f32>, preroll_secs: f32) -> Vec<f32> {
        if preroll_secs <= 0.0 || segment.is_empty() {
            return segment;
        }
        let preroll_n = (preroll_secs * 16000.0) as usize;
        let window_secs = (segment.len() as f32 / 16000.0) + preroll_secs + 0.05;
        let window = self.router.recent_audio(window_secs);
        if window.len() <= segment.len() {
            return segment;
        }
        let start = window.len().saturating_sub(segment.len());
        let pre = &window[..start];
        let take = pre.len().min(preroll_n);
        if take == 0 {
            return segment;
        }
        let mut out = Vec::with_capacity(take + segment.len());
        out.extend_from_slice(&pre[pre.len() - take..]);
        out.extend_from_slice(&segment);
        out
    }

    pub fn last_normalized_chunk(&self) -> &[f32] {
        &self.last_chunk
    }

    fn handle_keyword_detected(&mut self, keyword: &str, events: &mut PipelineEvents) {
        if self.is_mode_control_keyword(keyword) {
            self.vad.reset();
            self.stream_asr.reset();
            if let Some(action) = self.mode_keyword_action(keyword) {
                events.keywords.push(action);
            }
            return;
        }

        if self.mode == Mode::Normal && kws_keyword_matches(keyword, &self.hotword) {
            info!("Hotword detected: {keyword} (configured: {})", self.hotword);
            if self.speaker.in_verify_cooldown() {
                warn!("Hotword ignored: speaker verify cooldown active");
                return;
            }
            let audio = self.router.recent_audio(1.0);
            if self.speaker.is_enabled() && self.speaker.is_enrolled() {
                if self.speaker.verify(&audio) {
                    self.speaker.record_verify_pass();
                    info!("Speaker verification passed — entering command mode");
                    events.keywords.push("command".into());
                } else {
                    self.speaker.record_verify_fail();
                    warn!("Speaker verification failed for hotword");
                    events
                        .speaker_fails
                        .push("unrecognized speaker".into());
                }
            } else {
                events.keywords.push("command".into());
            }
        }
    }

    fn is_mode_control_keyword(&self, keyword: &str) -> bool {
        self.config
            .typing_mode
            .exit_phrases
            .iter()
            .any(|p| kws_keyword_matches(keyword, p))
            || self
                .config
                .normal_mode_phrases()
                .iter()
                .any(|p| kws_keyword_matches(keyword, p))
            || self
                .config
                .typing_mode_phrases()
                .iter()
                .any(|p| kws_keyword_matches(keyword, p))
    }

    fn mode_keyword_action(&self, keyword: &str) -> Option<String> {
        if self
            .config
            .typing_mode
            .exit_phrases
            .iter()
            .chain(self.config.normal_mode_phrases().iter())
            .any(|p| kws_keyword_matches(keyword, p))
        {
            return Some("normal".into());
        }
        if self
            .config
            .typing_mode_phrases()
            .iter()
            .any(|p| kws_keyword_matches(keyword, p))
        {
            return Some("typing".into());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_gates() {
        let p = SpeechPipeline::new(ModelPaths::from_home());
        assert!(!p.ready_for_normal());
        assert!(!p.ready_for_command());
        assert!(!p.ready_for_typing());
    }

    #[test]
    fn command_mode_does_not_emit_whisper_segments_without_stream() {
        let mut p = SpeechPipeline::new(ModelPaths::from_home());
        p.set_mode(Mode::Command);
        let events = p.process_audio(&[0.0f32; 1600]);
        assert!(events.speech_segments.is_empty());
        assert!(events.stream_results.is_empty());
    }
}
