use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use tracing::error;

use crate::audio::MicCapture;
use crate::commands::{CommandExecutor, CommandIntentResolver};
use crate::config::{load_config, save_config, WillowConfig};
use crate::modes::ModeStateMachine;
use crate::models::{keyword_encoding_available, ModelPaths};
use crate::pipeline::SpeechPipeline;
use crate::tts::TtsEngine;
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
}

struct ServiceInner {
    config: WillowConfig,
    pipeline: SpeechPipeline,
    mode_machine: ModeStateMachine,
    listening_desired: bool,
    audio_active: bool,
    is_running: bool,
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
    ) -> Option<crate::modes::ModeChange> {
        self.mode_machine
            .handle_transcription(result, &mut self.pipeline)
    }
}

impl ServiceCore {
    pub fn new() -> Result<Self> {
        let config = load_config().unwrap_or_default();
        let models_path = ModelPaths::from_home();
        let executor = Arc::new(CommandExecutor::new());
        let resolver = CommandIntentResolver::new(CommandExecutor::new());
        let tts = Arc::new(TtsEngine::new());
        let mut pipeline = SpeechPipeline::new(models_path, resolver);
        if let Err(e) = pipeline.initialize(&config) {
            error!("Pipeline init: {e}");
        }

        let mut mode_machine = ModeStateMachine::new(executor, tts);
        mode_machine.apply_config(&config);
        pipeline.set_mode(Mode::Normal);

        Ok(Self {
            inner: Arc::new(Mutex::new(ServiceInner {
                config,
                pipeline,
                mode_machine,
                listening_desired: true,
                audio_active: false,
                is_running: false,
            })),
            event_cb: Mutex::new(None),
            audio_thread: Mutex::new(None),
        })
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
            zvariant::OwnedValue::from(st.pipeline.is_ready()),
        );
        map.insert(
            "whisper_loaded".into(),
            zvariant::OwnedValue::from(st.pipeline.is_ready()),
        );
        map.insert(
            "kws_active".into(),
            zvariant::OwnedValue::from(st.pipeline.kws.is_loaded()),
        );
        map.insert(
            "speaker_enrolled".into(),
            zvariant::OwnedValue::from(st.pipeline.speaker.is_enrolled()),
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
            "streaming_active".into(),
            zvariant::OwnedValue::from(st.pipeline.asr.is_loaded()),
        );
        map.insert(
            "tts_enabled".into(),
            zvariant::OwnedValue::from(st.config.tts.enabled),
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
                self.emit(ServiceEvent::Notification {
                    title: "Hotword Updated".into(),
                    message: format!("Now listening for: {hotword}"),
                    urgency: "normal".into(),
                });
            }
            "command_threshold" => {
                let mut threshold: f64 = value.try_into()?;
                if threshold > 1.0 {
                    threshold /= 100.0;
                }
                st.config.command_threshold = threshold * 100.0;
                let threshold_frac = st.config.command_threshold_fraction();
                st.pipeline.resolver.set_threshold(threshold_frac);
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
        let commands = st.config.commands.clone();
        st.pipeline.resolver.set_commands(commands);
        let cfg = st.config.clone();
        st.pipeline.apply_config(&cfg)?;
        save_config(&st.config)?;
        drop(st);
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn remove_command(&self, name: &str) -> Result<()> {
        let mut st = self.inner.lock().unwrap();
        st.config.commands.retain(|c| c.name != name);
        let commands = st.config.commands.clone();
        st.pipeline.resolver.set_commands(commands);
        save_config(&st.config)?;
        drop(st);
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn start(&self) -> Result<()> {
        if !self.inner.lock().unwrap().pipeline.is_ready() {
            self.emit(ServiceEvent::Error {
                message: "Start Error".into(),
                details: "Speech models not loaded".into(),
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
        if let Some(handle) = self.audio_thread.lock().unwrap().take() {
            let _ = handle.join();
        }
        self.emit(ServiceEvent::Notification {
            title: "Voice Assistant".into(),
            message: "Service stopped".into(),
            urgency: "normal".into(),
        });
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
        if !self.inner.lock().unwrap().pipeline.is_ready() {
            self.emit(ServiceEvent::Error {
                message: "Enrollment Error".into(),
                details: "Speech models are not loaded".into(),
            });
            return Ok(());
        }
        self.try_start_listening()?;
        let reenroll;
        {
            let mut st = self.inner.lock().unwrap();
            if st.pipeline.speaker.enrollment_state() == EnrollmentState::Recording {
                self.emit(ServiceEvent::Notification {
                    title: "Voice Enrollment".into(),
                    message: "Enrollment is already in progress".into(),
                    urgency: "low".into(),
                });
                return Ok(());
            }
            reenroll = st.pipeline.speaker.is_enrolled();
            let user = st.config.speaker_verification.enrolled_user.clone();
            st.pipeline.speaker.start_enrollment(&user);
        }
        self.emit(ServiceEvent::Notification {
            title: "Voice Enrollment".into(),
            message: if reenroll {
                "Re-enrollment started — speak clearly for about 6 seconds total".into()
            } else {
                "Speak naturally — Willow will capture 3 short samples (~2 seconds each)".into()
            },
            urgency: "normal".into(),
        });
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    pub fn cancel_speaker_enrollment(&self) {
        self.inner.lock().unwrap().pipeline.speaker.cancel_enrollment();
        self.emit(ServiceEvent::Notification {
            title: "Voice Enrollment".into(),
            message: "Enrollment cancelled".into(),
            urgency: "low".into(),
        });
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
            st.listening_desired && st.pipeline.is_ready() && !st.audio_active
        };
        if should {
            let _ = self.try_start_listening();
        }
    }

    fn try_start_listening(&self) -> Result<()> {
        if self.inner.lock().unwrap().audio_active {
            return Ok(());
        }
        if !self.inner.lock().unwrap().pipeline.is_ready() {
            return Ok(());
        }

        if self.audio_thread.lock().unwrap().is_some() {
            let mut st = self.inner.lock().unwrap();
            st.audio_active = true;
            st.is_running = true;
            drop(st);
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
        let handle = MicCapture::start(move |chunk| {
            let mut events = Vec::new();
            {
                let mut st = inner.lock().unwrap();
                st.pipeline.process_audio(chunk);

                if st.pipeline.speaker.enrollment_state() == EnrollmentState::Recording {
                    let before = st.pipeline.speaker.enrollment_progress();
                    let state_before = st.pipeline.speaker.enrollment_state();
                    st.pipeline.speaker.add_enrollment_audio(chunk);
                    let after = st.pipeline.speaker.enrollment_progress();
                    let state_after = st.pipeline.speaker.enrollment_state();

                    if state_before == EnrollmentState::Recording
                        && state_after == EnrollmentState::Failed
                        && after < 3
                    {
                        events.push(ServiceEvent::Error {
                            message: "Enrollment Error".into(),
                            details: if after == 0 {
                                "Enrollment timed out — speak steadily right after pressing Start"
                                    .into()
                            } else {
                                "Enrollment failed — speak louder and try again".into()
                            },
                        });
                    } else if after > before {
                        events.push(ServiceEvent::Notification {
                            title: "Voice Enrollment".into(),
                            message: format!("Sample {after} of 3 captured"),
                            urgency: "normal".into(),
                        });
                        events.push(ServiceEvent::StatusChanged);
                    } else if st.pipeline.speaker.should_prompt_for_speech() {
                        let sample = st.pipeline.speaker.enrollment_progress();
                        events.push(ServiceEvent::Notification {
                            title: "Voice Enrollment".into(),
                            message: if sample == 0 {
                                "Enrollment listening — start speaking now".into()
                            } else {
                                format!("Keep speaking — sample {} of 3", sample + 1)
                            },
                            urgency: "low".into(),
                        });
                        st.pipeline.speaker.mark_speech_prompt_sent();
                    }

                    if st.pipeline.speaker.enrollment_progress() >= 3 {
                        let ok = st.pipeline.speaker.finish_enrollment();
                        events.push(ServiceEvent::StatusChanged);
                        events.push(if ok {
                            ServiceEvent::Notification {
                                title: "Voice Enrollment".into(),
                                message: "Voice profile enrolled successfully".into(),
                                urgency: "normal".into(),
                            }
                        } else {
                            ServiceEvent::Error {
                                message: "Enrollment Error".into(),
                                details:
                                    "Could not build voice profile — speak louder and try again"
                                        .into(),
                            }
                        });
                    }
                }
            }
            for ev in events {
                let _ = tx_audio.send(ev);
            }
        })?;

        *self.audio_thread.lock().unwrap() = Some(handle);

        let mut st = self.inner.lock().unwrap();
        st.audio_active = true;
        st.is_running = true;
        drop(st);

        self.emit(ServiceEvent::Notification {
            title: "Voice Assistant".into(),
            message: "Service started".into(),
            urgency: "normal".into(),
        });
        self.emit(ServiceEvent::StatusChanged);
        Ok(())
    }

    fn setup_callbacks(&self, tx: std::sync::mpsc::Sender<ServiceEvent>) -> Result<()> {
        let inner = self.inner.clone();

        {
            let mut st = inner.lock().unwrap();
            let tx_cmd = tx.clone();
            st.mode_machine.set_command_executed_callback(move |command, phrase, confidence| {
                let _ = tx_cmd.send(ServiceEvent::CommandExecuted {
                    command,
                    phrase,
                    confidence,
                });
            });
        }

        {
            let mut st = inner.lock().unwrap();
            st.pipeline.set_action_callback({
                let inner = inner.clone();
                let tx = tx.clone();
                move |keyword| {
                    if let Some(change) = inner.lock().unwrap().handle_keyword(keyword) {
                        let _ = tx.send(ServiceEvent::ModeChanged {
                            new_mode: change.new_mode.as_str().to_string(),
                            old_mode: change.old_mode.as_str().to_string(),
                        });
                    }
                    let _ = tx.send(ServiceEvent::StatusChanged);
                }
            });
        }

        {
            let st = inner.lock().unwrap();
            st.pipeline.kws.set_callback({
                let inner = inner.clone();
                let tx = tx.clone();
                move |keyword| {
                    let mut st = inner.lock().unwrap();
                    st.pipeline.on_keyword_detected(keyword);
                    drop(st);
                    let _ = tx.send(ServiceEvent::StatusChanged);
                }
            });
        }

        {
            let mut st = inner.lock().unwrap();
            st.pipeline.set_transcription_callback({
                let inner = inner.clone();
                let tx = tx.clone();
                move |result: TranscriptionResult| {
                    let mut events = Vec::new();
                    {
                        let mut st = inner.lock().unwrap();
                        st.pipeline.on_transcription_result(result.clone());
                    }

                    events.push(ServiceEvent::PartialBufferChanged {
                        partial: result.text.clone(),
                        is_final: result.is_final,
                    });
                    {
                        let st = inner.lock().unwrap();
                        if matches!(st.mode_machine.mode(), Mode::Command | Mode::Typing)
                            && !result.text.is_empty()
                        {
                            events.push(ServiceEvent::BufferChanged(result.text.clone()));
                        }
                    }

                    if let Some(change) = inner.lock().unwrap().handle_transcription(&result) {
                        events.push(ServiceEvent::ModeChanged {
                            new_mode: change.new_mode.as_str().to_string(),
                            old_mode: change.old_mode.as_str().to_string(),
                        });
                    }
                    events.push(ServiceEvent::StatusChanged);
                    for ev in events {
                        let _ = tx.send(ev);
                    }
                }
            });
        }

        {
            let mut st = inner.lock().unwrap();
            st.pipeline.set_speaker_fail_callback({
                let tx = tx.clone();
                move |reason| {
                    let _ = tx.send(ServiceEvent::SpeakerVerificationFailed(reason));
                }
            });
        }

        {
            let mut st = inner.lock().unwrap();
            st.pipeline.set_command_pending_callback({
                let tx = tx.clone();
                move |phrase, blocked| {
                    let _ = tx.send(ServiceEvent::CommandPending { phrase, blocked });
                }
            });
        }

        Ok(())
    }
}
