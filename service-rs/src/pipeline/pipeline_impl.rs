use anyhow::Result;
use tracing::{info, warn, error};

use super::asr::AsrEngine;
use super::kws::{KeywordsSource, KwsEngine};
use super::speaker::SpeakerVerifier;
use crate::audio::AudioRouter;
use crate::commands::phrase_index::kws_keyword_matches;
use crate::commands::CommandIntentResolver;
use crate::config::WillowConfig;
use crate::models::ModelPaths;
use crate::types::{Mode, TranscriptionResult};

/// Side effects produced while processing one audio chunk (no re-entrant callbacks).
#[derive(Debug, Default)]
pub struct PipelineEvents {
    /// Mode/command keywords to hand to the mode state machine ("command", "normal", …).
    pub keywords: Vec<String>,
    pub transcriptions: Vec<TranscriptionResult>,
    pub speaker_fails: Vec<String>,
    pub command_pending: Vec<(String, bool)>,
    pub asr_unavailable: bool,
}

pub struct SpeechPipeline {
    pub kws: KwsEngine,
    pub asr: AsrEngine,
    pub speaker: SpeakerVerifier,
    pub router: AudioRouter,
    pub resolver: CommandIntentResolver,
    mode: Mode,
    hotword: String,
    config: WillowConfig,
    kws_ready: bool,
    asr_ready: bool,
    init_error: Option<String>,
}

impl SpeechPipeline {
    pub fn new(paths: ModelPaths, resolver: CommandIntentResolver) -> Self {
        Self {
            kws: KwsEngine::new(paths.clone()),
            asr: AsrEngine::new(paths.clone()),
            speaker: SpeakerVerifier::new(&paths),
            router: AudioRouter::new(),
            resolver,
            mode: Mode::Normal,
            hotword: "hey willow".into(),
            config: WillowConfig::default(),
            kws_ready: false,
            asr_ready: false,
            init_error: None,
        }
    }

    pub fn initialize(&mut self, config: &WillowConfig) -> Result<()> {
        self.config = config.clone();
        self.hotword = config.hotword.clone();
        self.init_error = None;

        let kws_ok = self
            .kws
            .initialize(config.kws.threshold, &config.kws_phrases())
            .is_ok();
        if !kws_ok {
            self.init_error = self
                .kws
                .init_error()
                .map(|s| s.to_string())
                .or_else(|| Some("KWS initialization failed".into()));
        }

        let command_silence = config.command_mode.endpoint_silence;
        let asr_ok = self.asr.initialize(command_silence).is_ok();
        if !asr_ok && kws_ok {
            self.init_error = Some(
                self.init_error
                    .take()
                    .map(|e| format!("{e}; ASR model not found"))
                    .unwrap_or_else(|| "ASR model not found".into()),
            );
        }

        let speaker_ok = self.speaker.initialize(&ModelPaths::from_home()).is_ok();

        self.speaker.set_enabled(config.speaker_verification.enabled);
        self.speaker
            .set_threshold(config.speaker_verification.threshold);
        self.speaker
            .set_enrolled_user(&config.speaker_verification.enrolled_user);
        self.resolver
            .set_threshold(config.command_threshold_fraction());
        self.resolver.set_commands(config.commands.clone());

        self.kws_ready = kws_ok;
        self.asr_ready = asr_ok;
        if !speaker_ok {
            error!("Speaker model unavailable");
        }
        Ok(())
    }

    /// KWS loaded — sufficient for Normal mode / wake word listening.
    pub fn ready_for_normal(&self) -> bool {
        self.kws_ready
    }

    /// KWS + ASR loaded — required for Command/Typing modes.
    pub fn ready_for_command(&self) -> bool {
        self.kws_ready && self.asr_ready
    }

    pub fn kws_ready(&self) -> bool {
        self.kws_ready
    }

    pub fn asr_ready(&self) -> bool {
        self.asr_ready
    }

    pub fn init_error(&self) -> Option<&str> {
        self.init_error.as_deref()
    }

    pub fn keywords_source(&self) -> KeywordsSource {
        self.kws.keywords_source()
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.asr.reset_stream();
        match mode {
            Mode::Normal => {
                self.asr.set_enabled(false);
                self.kws.set_enabled(true);
            }
            Mode::Command => {
                self.kws.set_enabled(true);
                if self.asr_ready {
                    self.asr.set_enabled(true);
                    let _ = self.asr.set_endpoint_silence(
                        self.config.command_mode.endpoint_silence,
                    );
                } else {
                    self.asr.set_enabled(false);
                }
            }
            Mode::Typing => {
                self.kws.set_enabled(true);
                if self.asr_ready {
                    self.asr.set_enabled(true);
                    let _ = self
                        .asr
                        .set_endpoint_silence(self.config.streaming_asr.endpoint_silence_typing);
                } else {
                    self.asr.set_enabled(false);
                }
            }
        }
    }

    pub fn apply_config(&mut self, config: &WillowConfig) -> Result<()> {
        self.config = config.clone();
        self.hotword = config.hotword.clone();
        self.speaker.set_enabled(config.speaker_verification.enabled);
        self.speaker
            .set_threshold(config.speaker_verification.threshold);
        self.resolver
            .set_threshold(config.command_threshold_fraction());
        self.resolver.set_commands(config.commands.clone());
        let kws_ok = self
            .kws
            .initialize(config.kws.threshold, &config.kws_phrases())
            .is_ok();
        self.kws_ready = kws_ok;
        if !kws_ok {
            self.init_error = self
                .kws
                .init_error()
                .map(|s| s.to_string())
                .or_else(|| Some("KWS re-initialization failed".into()));
        } else if self.asr_ready {
            self.init_error = None;
        }
        Ok(())
    }

    pub fn process_audio(&mut self, chunk: &[f32]) -> PipelineEvents {
        let mut events = PipelineEvents::default();
        if !self.kws_ready {
            return events;
        }
        self.router.push_chunk(chunk);

        if self.kws.is_loaded() {
            for keyword in self.kws.process_audio(chunk) {
                self.handle_keyword_detected(&keyword, &mut events);
            }
        }
        if matches!(self.mode, Mode::Command | Mode::Typing) {
            if self.asr_ready {
                for result in self.asr.process_audio(chunk) {
                    self.handle_transcription_result(&result, &mut events);
                    events.transcriptions.push(result);
                }
            } else {
                events.asr_unavailable = true;
            }
        }
        events
    }

    fn handle_keyword_detected(&mut self, keyword: &str, events: &mut PipelineEvents) {
        if self.is_mode_control_keyword(keyword) {
            self.asr.reset_stream();
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
            // Shorter window keeps more hotword speech vs leading silence.
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
            return;
        }
        // Non-mode-control KWS hits in Command/Typing are ignored (commands use ASR).
    }

    fn handle_transcription_result(
        &mut self,
        result: &TranscriptionResult,
        events: &mut PipelineEvents,
    ) {
        if self.mode == Mode::Command && !result.is_endpoint {
            let pending = self.resolver.process_partial(&result.text);
            if pending.pending {
                events.command_pending.push((
                    pending.matched_phrase.clone(),
                    pending.blocked_by_prefix,
                ));
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
    use crate::commands::CommandIntentResolver;
    use std::sync::Arc;

    fn empty_pipeline() -> SpeechPipeline {
        SpeechPipeline::new(
            ModelPaths::from_home(),
            CommandIntentResolver::new(Arc::new(crate::commands::CommandExecutor::new())),
        )
    }

    #[test]
    fn readiness_gates() {
        let p = empty_pipeline();
        assert!(!p.ready_for_normal());
        assert!(!p.ready_for_command());
    }
}
