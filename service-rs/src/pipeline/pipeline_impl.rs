use std::sync::Arc;

use anyhow::Result;
use tracing::error;

use super::asr::AsrEngine;
use super::kws::KwsEngine;
use super::speaker::SpeakerVerifier;
use crate::audio::AudioRouter;
use crate::commands::phrase_index::kws_keyword_matches;
use crate::commands::CommandIntentResolver;
use crate::config::WillowConfig;
use crate::models::ModelPaths;
use crate::types::{Mode, TranscriptionResult};

pub struct SpeechPipeline {
    pub kws: KwsEngine,
    pub asr: AsrEngine,
    pub speaker: SpeakerVerifier,
    pub router: AudioRouter,
    pub resolver: CommandIntentResolver,
    mode: Mode,
    hotword: String,
    config: WillowConfig,
    ready: bool,
    on_keyword: Option<Arc<dyn Fn(&str) + Send + Sync>>,
    on_transcription: Option<Arc<dyn Fn(TranscriptionResult) + Send + Sync>>,
    on_speaker_fail: Option<Arc<dyn Fn(String) + Send + Sync>>,
    on_command_pending: Option<Arc<dyn Fn(String, bool) + Send + Sync>>,
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
            ready: false,
            on_keyword: None,
            on_transcription: None,
            on_speaker_fail: None,
            on_command_pending: None,
        }
    }

    pub fn initialize(&mut self, config: &WillowConfig) -> Result<()> {
        self.config = config.clone();
        self.hotword = config.hotword.clone();

        let kws_ok = self
            .kws
            .initialize(config.kws.threshold, &config.kws_phrases())
            .is_ok();
        let asr_ok = self
            .asr
            .initialize(config.streaming_asr.endpoint_silence_command)
            .is_ok();
        let speaker_ok = self.speaker.initialize(&ModelPaths::from_home()).is_ok();

        self.speaker.set_enabled(config.speaker_verification.enabled);
        self.speaker
            .set_threshold(config.speaker_verification.threshold);
        self.speaker
            .set_enrolled_user(&config.speaker_verification.enrolled_user);
        self.resolver
            .set_threshold(config.command_threshold_fraction());
        self.resolver.set_commands(config.commands.clone());

        self.ready = kws_ok && asr_ok;
        if !speaker_ok {
            error!("Speaker model unavailable");
        }
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.ready
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
                self.asr.set_enabled(true);
                let _ = self
                    .asr
                    .set_endpoint_silence(self.config.streaming_asr.endpoint_silence_command);
            }
            Mode::Typing => {
                self.kws.set_enabled(true);
                self.asr.set_enabled(true);
                let _ = self
                    .asr
                    .set_endpoint_silence(self.config.streaming_asr.endpoint_silence_typing);
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
        self.kws
            .initialize(config.kws.threshold, &config.kws_phrases())?;
        Ok(())
    }

    pub fn set_action_callback<F>(&mut self, f: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        self.on_keyword = Some(Arc::new(f));
    }

    pub fn set_keyword_callback<F>(&mut self, f: F)
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        let inner_cb = Arc::new(f);
        self.kws.set_callback(move |kw| inner_cb(kw));
    }

    pub fn set_transcription_callback<F>(&mut self, f: F)
    where
        F: Fn(TranscriptionResult) + Send + Sync + 'static,
    {
        let cb = Arc::new(f);
        self.on_transcription = Some(cb.clone());
        self.asr.set_callback(move |r| cb(r));
    }

    pub fn set_speaker_fail_callback<F>(&mut self, f: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.on_speaker_fail = Some(Arc::new(f));
    }

    pub fn set_command_pending_callback<F>(&mut self, f: F)
    where
        F: Fn(String, bool) + Send + Sync + 'static,
    {
        self.on_command_pending = Some(Arc::new(f));
    }

    pub fn process_audio(&mut self, chunk: &[f32]) {
        if !self.ready {
            return;
        }
        self.router.push_chunk(chunk);

        if self.mode == Mode::Normal || self.kws.is_loaded() {
            self.kws.process_audio(chunk);
        }
        if matches!(self.mode, Mode::Command | Mode::Typing) {
            self.asr.process_audio(chunk);
        }
    }

    pub fn on_keyword_detected(&mut self, keyword: &str) {
        if self.is_mode_control_keyword(keyword) {
            self.asr.reset_stream();
            self.handle_mode_keyword(keyword);
            return;
        }

        if self.mode == Mode::Normal && kws_keyword_matches(keyword, &self.hotword) {
            let audio = self.router.recent_audio(2.0);
            if self.speaker.is_enabled() && self.speaker.is_enrolled() {
                let verified = self.speaker.verify(&audio);
                if verified {
                    if let Some(cb) = &self.on_keyword {
                        cb("command");
                    }
                } else if let Some(cb) = &self.on_speaker_fail {
                    cb("unrecognized speaker".into());
                }
            } else if let Some(cb) = &self.on_keyword {
                cb("command");
            }
            return;
        }

        if self.mode == Mode::Command {
            self.asr.reset_stream();
            if let Some(cb) = &self.on_keyword {
                cb(keyword);
            }
        }
    }

    pub fn on_transcription_result(&mut self, result: TranscriptionResult) {
        if let Some(cb) = &self.on_transcription {
            cb(result.clone());
        }

        if self.mode == Mode::Command && !result.is_endpoint {
            let pending = self.resolver.process_partial(&result.text);
            if pending.pending {
                if let Some(cb) = &self.on_command_pending {
                    cb(
                        pending.matched_phrase.clone(),
                        pending.blocked_by_prefix,
                    );
                }
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

    fn handle_mode_keyword(&self, keyword: &str) {
        let cb = match &self.on_keyword {
            Some(cb) => cb,
            None => return,
        };
        if self
            .config
            .typing_mode
            .exit_phrases
            .iter()
            .chain(self.config.normal_mode_phrases().iter())
            .any(|p| kws_keyword_matches(keyword, p))
        {
            cb("normal");
            return;
        }
        if self
            .config
            .typing_mode_phrases()
            .iter()
            .any(|p| kws_keyword_matches(keyword, p))
        {
            cb("typing");
        }
    }
}
