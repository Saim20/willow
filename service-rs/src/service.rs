use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use tracing::error;

use crate::audio::MicCapture;
use crate::commands::{CommandExecutor, CommandWorker};
use crate::config::{load_config, save_config, WillowConfig};
use crate::intent::IntentEngine;
use crate::llm::LlmFallback;
use crate::modes::ModeStateMachine;
use crate::models::{keyword_encoding_available, ModelPaths};
use crate::pipeline::SpeechPipeline;
use crate::types::{EnrollmentState, Mode, TranscriptionResult};

fn dbus_str(s: &str) -> zvariant::OwnedValue {
    zvariant::OwnedValue::from(zvariant::Str::from(s))
}

pub type EventCallback = Arc<dyn Fn(ServiceEvent) + Send + Sync>;

#[derive(Debug, Clone)]
pub enum ServiceEvent {
    ModeChanged { new_mode: String, old_mode: String },
    BufferChanged(String),
    PartialBufferChanged { partial: String, is_final: bool },
    CommandPending { phrase: String, blocked: bool },
    SpeakerVerificationFailed(String),
    CommandExecuted {
        command: String,
        phrase: String,
        confidence: f64,
    },
    StatusChanged,
    Error { message: String, details: String },
    #[allow(dead_code)]
    Notification {
        title: String,
        message: String,
        urgency: String,
    },
    ConfigChanged(String),
}

pub struct ServiceCore {
    inner: Arc<Mutex<ServiceInner>>,
    event_cb: Mutex<Option<EventCallback>>,
    audio_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
    audio_stop: Mutex<Option<Arc<AtomicBool>>>,
    command_executed_cb: Arc<Mutex<Option<Arc<dyn Fn(String, String, f64) + Send + Sync>>>>,
}

struct ServiceInner {
    config: WillowConfig,
    pipeline: SpeechPipeline,
    mode_machine: ModeStateMachine,
    listening_desired: bool,
    audio_active: bool,
    is_running: bool,
    asr_unavailable_notified: bool,
}

impl ServiceInner {
    fn set_mode(&mut self, new_mode: Mode) {
        self.mode_machine.set_mode(new_mode);
        self.pipeline.set_mode(new_mode);
    }

    fn handle_keyword(&mut self, keyword: &str) -> Option<crate::modes::ModeChange> {
        self.mode_machine.handle_keyword(keyword, &mut self.pipeline)
    }

    fn handle_transcription(
        &mut self,
        result: &TranscriptionResult,
    ) -> crate::modes::TranscriptionOutcome {
        self.mode_machine
            .handle_transcription(result, &mut self.pipeline)
    }

    /// Fast path under the service lock: AGC, KWS, streaming ASR / VAD — no Whisper.
    fn process_audio_pre_whisper(
        &mut self,
        chunk: &[f32],
    ) -> (
        Vec<ServiceEvent>,
        Vec<Vec<f32>>,
        Vec<TranscriptionResult>,
        Arc<crate::pipeline::WhisperEngine>,
        f32,
    ) {
        let mut events = Vec::new();
        let pipeline_events = self.pipeline.process_audio(chunk);
        let whisper = self.pipeline.whisper_handle();
        let pre_pad = self.pipeline.whisper_pre_pad();
        let segments = pipeline_events.speech_segments;
        let stream_results = pipeline_events.stream_results;

        for keyword in &pipeline_events.keywords {
            let asr_ok = match keyword.as_str() {
                "command" => self.pipeline.ready_for_command(),
                "typing" => self.pipeline.ready_for_typing(),
                _ => true,
            };
            if !asr_ok {
                self.push_asr_unavailable_once(&mut events);
                continue;
            }
            if let Some(change) = self.handle_keyword(keyword) {
                events.push(ServiceEvent::ModeChanged {
                    new_mode: change.new_mode.as_str().to_string(),
                    old_mode: change.old_mode.as_str().to_string(),
                });
                events.push(ServiceEvent::StatusChanged);
            }
        }

        for reason in pipeline_events.speaker_fails {
            events.push(ServiceEvent::SpeakerVerificationFailed(reason));
        }

        for (phrase, blocked) in pipeline_events.command_pending {
            events.push(ServiceEvent::CommandPending { phrase, blocked });
        }

        // Streaming command partials / endpoints (handled under lock — no GPU Whisper).
        for result in &stream_results {
            events.push(ServiceEvent::PartialBufferChanged {
                partial: result.text.clone(),
                is_final: result.is_final || result.is_endpoint,
            });
            if matches!(self.mode_machine.mode(), Mode::Command) {
                events.push(ServiceEvent::BufferChanged(result.text.clone()));
            }
            let outcome = self.handle_transcription(result);
            if let Some(change) = outcome.mode_change {
                events.push(ServiceEvent::ModeChanged {
                    new_mode: change.new_mode.as_str().to_string(),
                    old_mode: change.old_mode.as_str().to_string(),
                });
            }
            if let Some(prompt) = outcome.prompt {
                events.push(ServiceEvent::CommandPending {
                    phrase: prompt,
                    blocked: true,
                });
            }
            if let Some(pending) = outcome.pending_phrase {
                events.push(ServiceEvent::CommandPending {
                    phrase: pending,
                    blocked: true,
                });
            }
            events.push(ServiceEvent::StatusChanged);
        }

        self.collect_enrollment_events(&mut events);

        if let Some(change) = self.mode_machine.tick_idle(&mut self.pipeline) {
            events.push(ServiceEvent::ModeChanged {
                new_mode: change.new_mode.as_str().to_string(),
                old_mode: change.old_mode.as_str().to_string(),
            });
            events.push(ServiceEvent::StatusChanged);
        }

        (events, segments, stream_results, whisper, pre_pad)
    }

    fn push_asr_unavailable_once(&mut self, events: &mut Vec<ServiceEvent>) {
        if self.asr_unavailable_notified {
            return;
        }
        self.asr_unavailable_notified = true;
        events.push(ServiceEvent::Error {
            message: "ASR Unavailable".into(),
            details: "Streaming ASR / Whisper models missing — run ./deploy-dev.sh or willow-download-model"
                .into(),
        });
    }

    fn finish_with_transcripts(&mut self, texts: Vec<String>) -> Vec<ServiceEvent> {
        let mut events = Vec::new();
        for text in texts {
            let result = TranscriptionResult {
                text: text.clone(),
                is_final: true,
                is_endpoint: true,
                from_whisper: true,
                is_stable: true,
            };
            events.push(ServiceEvent::PartialBufferChanged {
                partial: text.clone(),
                is_final: true,
            });
            if matches!(self.mode_machine.mode(), Mode::Command | Mode::Typing) {
                events.push(ServiceEvent::BufferChanged(text));
            }
            let outcome = self.handle_transcription(&result);
            if let Some(change) = outcome.mode_change {
                events.push(ServiceEvent::ModeChanged {
                    new_mode: change.new_mode.as_str().to_string(),
                    old_mode: change.old_mode.as_str().to_string(),
                });
            }
            if let Some(prompt) = outcome.prompt {
                events.push(ServiceEvent::CommandPending {
                    phrase: prompt,
                    blocked: true,
                });
            }
            events.push(ServiceEvent::StatusChanged);
        }
        events
    }

    fn collect_enrollment_events(&mut self, events: &mut Vec<ServiceEvent>) {
        if self.pipeline.speaker.enrollment_state() != EnrollmentState::Recording {
            return;
        }
        let before = self.pipeline.speaker.enrollment_progress();
        let state_before = self.pipeline.speaker.enrollment_state();
        let normalized = self.pipeline.last_normalized_chunk().to_vec();
        self.pipeline.speaker.add_enrollment_audio(&normalized);
        let after = self.pipeline.speaker.enrollment_progress();
        let state_after = self.pipeline.speaker.enrollment_state();

        if state_before == EnrollmentState::Recording
            && state_after == EnrollmentState::Failed
            && after < 3
        {
            events.push(ServiceEvent::Error {
                message: "Enrollment Error".into(),
                details: if after == 0 {
                    "Enrollment timed out — speak steadily right after pressing Start".into()
                } else {
                    "Enrollment failed — speak louder and try again".into()
                },
            });
        } else if after > before {
            events.push(ServiceEvent::StatusChanged);
        } else if self.pipeline.speaker.should_prompt_for_speech() {
            self.pipeline.speaker.mark_speech_prompt_sent();
            events.push(ServiceEvent::StatusChanged);
        }

        if self.pipeline.speaker.enrollment_progress() >= 3 {
            let ok = self.pipeline.speaker.finish_enrollment();
            events.push(ServiceEvent::StatusChanged);
            if !ok {
                events.push(ServiceEvent::Error {
                    message: "Enrollment Error".into(),
                    details: "Could not build voice profile — speak louder and try again".into(),
                });
            }
        }
    }
}

impl ServiceCore {
    pub fn new() -> Result<Self> {
        let config = load_config().unwrap_or_default();
        let models_path = ModelPaths::from_home();
        let executor = Arc::new(CommandExecutor::new());
        let intent = IntentEngine::new(executor.clone());
        let llm = LlmFallback::new(config.inference.llm.clone());
        let command_executed_cb: Arc<Mutex<Option<Arc<dyn Fn(String, String, f64) + Send + Sync>>>> =
            Arc::new(Mutex::new(None));
        let cb_slot = command_executed_cb.clone();
        let worker = CommandWorker::new(
            executor,
            Arc::new(move |command, phrase, confidence| {
                if let Some(cb) = cb_slot.lock().unwrap().as_ref() {
                    cb(command, phrase, confidence);
                }
            }),
        );
        let mut pipeline = SpeechPipeline::new(models_path);
        let mut init_errors = Vec::new();
        if let Err(e) = pipeline.initialize(&config) {
            init_errors.push(format!("Pipeline init: {e}"));
            error!("Pipeline init: {e}");
        }
        if let Some(err) = pipeline.init_error() {
            init_errors.push(err.to_string());
        }

        let core = Self {
            inner: Arc::new(Mutex::new(ServiceInner {
                config: config.clone(),
                pipeline,
                mode_machine: ModeStateMachine::new(worker, intent, llm),
                listening_desired: true,
                audio_active: false,
                is_running: false,
                asr_unavailable_notified: false,
            })),
            event_cb: Mutex::new(None),
            audio_thread: Mutex::new(None),
            audio_stop: Mutex::new(None),
            command_executed_cb,
        };

        {
            let mut st = core.inner.lock().unwrap();
            st.mode_machine.apply_config(&config);
            st.pipeline.set_mode(Mode::Normal);
        }

        if !init_errors.is_empty() {
            core.emit(ServiceEvent::Error {
                message: "Initialization Warning".into(),
                details: init_errors.join("; "),
            });
        }

        Ok(core)
    }

    pub fn set_event_callback(&self, cb: EventCallback) {
        *self.event_cb.lock().unwrap() = Some(cb);
    }

    fn emit(&self, event: ServiceEvent) {
        if let Some(cb) = self.event_cb.lock().unwrap().as_ref() {
            cb(event);
        }
    }

    pub fn set_mode(&self, mode: &str) -> Result<()> {
        let new_mode = Mode::from_str(mode);
        let mut st = self.inner.lock().unwrap();
        let old = st.mode_machine.mode();
        if new_mode == old {
            return Ok(());
        }
        if matches!(new_mode, Mode::Command) && !st.pipeline.ready_for_command() {
            let mut events = Vec::new();
            st.push_asr_unavailable_once(&mut events);
            drop(st);
            for ev in events {
                self.emit(ev);
            }
            anyhow::bail!("Streaming ASR model not loaded — cannot enter command mode");
        }
        if matches!(new_mode, Mode::Typing) && !st.pipeline.ready_for_typing() {
            let mut events = Vec::new();
            st.push_asr_unavailable_once(&mut events);
            drop(st);
            for ev in events {
                self.emit(ev);
            }
            anyhow::bail!("Whisper model not loaded — cannot enter typing mode");
        }
        if st.pipeline.ready_for_command() || st.pipeline.ready_for_typing() {
            st.asr_unavailable_notified = false;
        }
        st.set_mode(new_mode);
        drop(st);
        self.emit(ServiceEvent::ModeChanged {
            new_mode: new_mode.as_str().to_string(),
            old_mode: old.as_str().to_string(),
        });
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn get_mode(&self) -> String {
        self.inner
            .lock()
            .unwrap()
            .mode_machine
            .mode()
            .as_str()
            .to_string()
    }

    pub fn get_status(&self) -> std::collections::HashMap<String, zvariant::OwnedValue> {
        let st = self.inner.lock().unwrap();
        let mut map = std::collections::HashMap::new();
        map.insert(
            "is_running".into(),
            zvariant::OwnedValue::from(st.is_running && st.audio_active),
        );
        map.insert(
            "audio_active".into(),
            zvariant::OwnedValue::from(st.audio_active),
        );
        map.insert(
            "current_mode".into(),
            dbus_str(st.mode_machine.mode().as_str()),
        );
        map.insert(
            "current_buffer".into(),
            dbus_str(st.mode_machine.buffer()),
        );
        map.insert(
            "command_count".into(),
            zvariant::OwnedValue::from(st.config.commands.len() as i32),
        );
        map.insert(
            "models_loaded".into(),
            zvariant::OwnedValue::from(
                st.pipeline.ready_for_command() || st.pipeline.ready_for_typing(),
            ),
        );
        map.insert(
            "kws_ready".into(),
            zvariant::OwnedValue::from(st.pipeline.kws_ready()),
        );
        map.insert(
            "asr_ready".into(),
            zvariant::OwnedValue::from(st.pipeline.asr_ready()),
        );
        map.insert(
            "stream_asr_ready".into(),
            zvariant::OwnedValue::from(st.pipeline.stream_ready()),
        );
        map.insert(
            "whisper_loaded".into(),
            zvariant::OwnedValue::from(st.pipeline.whisper_ready()),
        );
        map.insert(
            "whisper_ready".into(),
            zvariant::OwnedValue::from(st.pipeline.whisper_ready()),
        );
        map.insert(
            "workflow_prompt".into(),
            dbus_str(st.mode_machine.last_prompt().unwrap_or("")),
        );
        map.insert(
            "inference_provider".into(),
            dbus_str(st.pipeline.active_provider()),
        );
        map.insert(
            "kws_active".into(),
            zvariant::OwnedValue::from(st.pipeline.kws.is_loaded()),
        );
        map.insert(
            "speaker_verification_enabled".into(),
            zvariant::OwnedValue::from(st.config.speaker_verification.enabled),
        );
        map.insert(
            "speaker_enrolled".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.is_enrolled()),
        );
        map.insert(
            "enrollment_prompt".into(),
            dbus_str(st.pipeline.speaker.current_enrollment_prompt()),
        );
        map.insert("hotword".into(), dbus_str(&st.config.hotword));
        map.insert(
            "enrollment_state".into(),
            dbus_str(st.pipeline.speaker.enrollment_state().as_str()),
        );
        map.insert(
            "enrollment_samples".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.enrollment_progress()),
        );
        map.insert(
            "enrollment_buffer_fraction".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.enrollment_buffer_fraction() as f64),
        );
        map.insert(
            "enrollment_reenrolling".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.is_reenrolling()),
        );
        map.insert(
            "keyword_encoding_ready".into(),
            zvariant::OwnedValue::from(keyword_encoding_available()),
        );
        map.insert(
            "kws_keywords_source".into(),
            dbus_str(st.pipeline.keywords_source().as_str()),
        );
        map.insert(
            "init_error".into(),
            dbus_str(st.pipeline.init_error().unwrap_or("")),
        );
        map.insert(
            "speaker_verification_last_result".into(),
            zvariant::OwnedValue::from(
                st.pipeline
                    .speaker
                    .last_verify_result()
                    .map(|b| if b { 1i32 } else { 0 })
                    .unwrap_or(-1),
            ),
        );
        map.insert(
            "streaming_active".into(),
            zvariant::OwnedValue::from(st.pipeline.whisper_ready()),
        );
        map
    }

    pub fn get_config(&self) -> String {
        serde_json::to_string(&self.inner.lock().unwrap().config).unwrap_or_default()
    }

    pub fn update_config(&self, json: &str) -> Result<()> {
        let config: WillowConfig = serde_json::from_str(json)?;
        let mut st = self.inner.lock().unwrap();
        st.config = config.clone();
        st.mode_machine.apply_config(&config);
        st.pipeline.apply_config(&config)?;
        save_config(&config)?;
        let cfg_json = serde_json::to_string(&config)?;
        drop(st);
        self.emit(ServiceEvent::ConfigChanged(cfg_json));
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn set_config_value(&self, key: &str, value: zvariant::Value<'_>) -> Result<()> {
        let mut st = self.inner.lock().unwrap();
        match key {
            "hotword" => {
                let hotword: String = value.try_into()?;
                st.config.hotword = hotword.clone();
                let cfg = st.config.clone();
                st.pipeline.apply_config(&cfg)?;
            }
            "command_threshold" => {
                let mut threshold: f64 = value.try_into()?;
                if threshold > 1.0 {
                    threshold /= 100.0;
                }
                st.config.command_threshold = threshold * 100.0;
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "inference.provider" | "provider" => {
                let provider: String = value.try_into()?;
                st.config.inference.provider = provider;
                let cfg = st.config.clone();
                st.pipeline.apply_config(&cfg)?;
            }
            "command_mode.endpoint_silence"
            | "endpoint_silence" => {
                let secs: f64 = value.try_into()?;
                st.config.command_mode.endpoint_silence = (secs as f32).clamp(0.15, 2.0);
                st.config.streaming_asr.rule2_min_trailing_silence =
                    (st.config.command_mode.endpoint_silence * 2.0).clamp(0.3, 1.5);
                let cfg = st.config.clone();
                st.pipeline.apply_config(&cfg)?;
                st.mode_machine.apply_config(&cfg);
            }
            "workflows.session_timeout" | "session_timeout" => {
                let secs: f64 = value.try_into()?;
                st.config.workflows.session_timeout = (secs as f32).clamp(2.0, 120.0);
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "intent.early_fire" | "early_fire" => {
                let enabled: bool = value.try_into()?;
                st.config.intent.early_fire = enabled;
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "intent.llm_fallback" | "llm_fallback" | "llm-enabled" => {
                let enabled: bool = value.try_into()?;
                st.config.intent.llm_fallback = enabled;
                st.config.inference.llm.enabled = enabled;
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "inference.llm.enabled" | "llm.enabled" => {
                let enabled: bool = value.try_into()?;
                st.config.inference.llm.enabled = enabled;
                st.config.intent.llm_fallback = enabled;
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "inference.llm.model_path" | "llm.model_path" => {
                let path: String = value.try_into()?;
                st.config.inference.llm.model_path = path;
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "inference.llm.max_tokens" | "llm.max_tokens" => {
                let tokens: i32 = value.try_into()?;
                st.config.inference.llm.max_tokens = tokens.clamp(16, 256);
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "inference.llm.timeout_ms" | "llm.timeout_ms" => {
                let ms: i32 = value.try_into()?;
                st.config.inference.llm.timeout_ms = ms.clamp(100, 5000);
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            "typing_mode.auto_revert" | "typing_auto_revert" => {
                let enabled: bool = value.try_into()?;
                st.config.typing_mode.auto_revert = enabled;
                let cfg = st.config.clone();
                st.mode_machine.apply_config(&cfg);
            }
            _ => {}
        }
        save_config(&st.config)?;
        drop(st);
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn get_commands(&self) -> String {
        serde_json::to_string(&self.inner.lock().unwrap().config.commands)
            .unwrap_or_else(|_| "[]".into())
    }

    pub fn add_command(&self, name: &str, command: &str, phrases: Vec<String>) -> Result<()> {
        let mut st = self.inner.lock().unwrap();
        st.config.commands.retain(|c| c.name != name);
        st.config.commands.push(crate::types::Command {
            name: name.to_string(),
            command: command.to_string(),
            phrases,
        });
        let cfg = st.config.clone();
        st.mode_machine.apply_config(&cfg);
        save_config(&st.config)?;
        drop(st);
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn remove_command(&self, name: &str) -> Result<()> {
        let mut st = self.inner.lock().unwrap();
        st.config.commands.retain(|c| c.name != name);
        let cfg = st.config.clone();
        st.mode_machine.apply_config(&cfg);
        save_config(&st.config)?;
        drop(st);
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn start(&self) -> Result<()> {
        if !self.inner.lock().unwrap().pipeline.ready_for_normal() {
            let err = {
                let st = self.inner.lock().unwrap();
                st.pipeline
                    .init_error()
                    .unwrap_or("KWS model not loaded")
                    .to_string()
            };
            self.emit(ServiceEvent::Error {
                message: "Start Error".into(),
                details: err,
            });
            return Ok(());
        }
        {
            let mut st = self.inner.lock().unwrap();
            st.listening_desired = true;
        }
        self.try_start_listening()
    }

    pub fn stop(&self) {
        {
            let mut st = self.inner.lock().unwrap();
            st.listening_desired = false;
            st.is_running = false;
            st.audio_active = false;
        }
        if let Some(stop) = self.audio_stop.lock().unwrap().take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        if let Some(handle) = self.audio_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.emit(ServiceEvent::StatusChanged);
    }

    pub fn restart(&self) -> Result<()> {
        self.stop();
        thread::sleep(Duration::from_millis(500));
        self.start()
    }

    pub fn get_buffer(&self) -> String {
        self.inner.lock().unwrap().mode_machine.buffer().to_string()
    }

    pub fn is_running(&self) -> bool {
        let st = self.inner.lock().unwrap();
        st.is_running && st.audio_active
    }

    pub fn start_speaker_enrollment(&self) -> Result<()> {
        if !self.inner.lock().unwrap().config.speaker_verification.enabled {
            self.emit(ServiceEvent::Error {
                message: "Enrollment Error".into(),
                details: "Speaker verification is disabled — set speaker_verification.enabled to true in config.json"
                    .into(),
            });
            return Ok(());
        }

        if !self.inner.lock().unwrap().pipeline.ready_for_normal() {
            self.emit(ServiceEvent::Error {
                message: "Enrollment Error".into(),
                details: "KWS model is not loaded".into(),
            });
            return Ok(());
        }
        self.try_start_listening()?;
        {
            let mut st = self.inner.lock().unwrap();
            if st.pipeline.speaker.enrollment_state() == EnrollmentState::Recording {
                return Ok(());
            }
            let user = st.config.speaker_verification.enrolled_user.clone();
            st.pipeline.speaker.start_enrollment(&user);
        }
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn cancel_speaker_enrollment(&self) {
        self.inner.lock().unwrap().pipeline.speaker.cancel_enrollment();
        self.emit(ServiceEvent::StatusChanged);
    }

    pub fn remove_speaker_profile(&self) {
        self.inner.lock().unwrap().pipeline.speaker.remove_profile();
        self.emit(ServiceEvent::StatusChanged);
    }

    pub fn enrollment_status(&self) -> std::collections::HashMap<String, zvariant::OwnedValue> {
        let st = self.inner.lock().unwrap();
        let mut map = std::collections::HashMap::new();
        map.insert("state".into(), dbus_str(st.pipeline.speaker.enrollment_state().as_str()));
        map.insert(
            "samples".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.enrollment_progress()),
        );
        map.insert(
            "enrolled".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.is_enrolled()),
        );
        map
    }

    pub fn auto_start(&self) {
        let should = {
            let st = self.inner.lock().unwrap();
            st.listening_desired && st.pipeline.ready_for_normal() && !st.audio_active
        };
        if should {
            let _ = self.try_start_listening();
        }
    }

    fn try_start_listening(&self) -> Result<()> {
        if self.inner.lock().unwrap().audio_active {
            return Ok(());
        }
        if !self.inner.lock().unwrap().pipeline.ready_for_normal() {
            return Ok(());
        }

        if self.audio_thread.lock().unwrap().is_some() {
            {
                let mut st = self.inner.lock().unwrap();
                st.audio_active = true;
                st.is_running = true;
            }
            self.emit(ServiceEvent::StatusChanged);
            return Ok(());
        }

        let (tx, rx) = std::sync::mpsc::channel::<ServiceEvent>();
        {
            let event_cb = self.event_cb.lock().unwrap().clone();
            thread::spawn(move || {
                while let Ok(ev) = rx.recv() {
                    if let Some(cb) = &event_cb {
                        cb(ev);
                    }
                }
            });
        }

        self.setup_callbacks(tx.clone())?;

        let inner = self.inner.clone();
        let tx_audio = tx.clone();
        let (handle, stop) = MicCapture::start(move |chunk| {
            let (mut events, segments, _stream_results, whisper, pre_pad) = {
                let mut st = inner.lock().unwrap();
                st.process_audio_pre_whisper(chunk)
            };
            // Typing: Whisper outside the service mutex.
            let texts: Vec<String> = segments
                .into_iter()
                .filter_map(|seg| {
                    let text = whisper.transcribe(&seg, pre_pad);
                    if text.is_empty() {
                        None
                    } else {
                        tracing::info!("Whisper: {text}");
                        Some(text)
                    }
                })
                .collect();
            if !texts.is_empty() {
                let mut st = inner.lock().unwrap();
                events.extend(st.finish_with_transcripts(texts));
            }
            for ev in events {
                let _ = tx_audio.send(ev);
            }
        })?;

        *self.audio_thread.lock().unwrap() = Some(handle);
        *self.audio_stop.lock().unwrap() = Some(stop);

        {
            let mut st = self.inner.lock().unwrap();
            st.audio_active = true;
            st.is_running = true;
        }
        self.emit(ServiceEvent::StatusChanged);

        Ok(())
    }

    fn setup_callbacks(&self, tx: std::sync::mpsc::Sender<ServiceEvent>) -> Result<()> {
        *self.command_executed_cb.lock().unwrap() = Some(Arc::new(move |command, phrase, confidence| {
            let _ = tx.send(ServiceEvent::CommandExecuted {
                command,
                phrase,
                confidence,
            });
        }));
        Ok(())
    }
}
