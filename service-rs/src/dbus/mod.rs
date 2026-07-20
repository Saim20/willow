use std::sync::Arc;

use zbus::object_server::SignalEmitter;
use zbus::zvariant;
use zbus::interface;

use crate::service::{ServiceCore, ServiceEvent};

#[derive(Clone)]
pub struct WillowDBus {
    pub core: Arc<ServiceCore>,
    iface_ref: Arc<tokio::sync::Mutex<Option<zbus::object_server::InterfaceRef<WillowDBus>>>>,
}

impl WillowDBus {
    pub fn new(core: Arc<ServiceCore>) -> Self {
        Self {
            core,
            iface_ref: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub async fn set_iface_ref(&self, iface: zbus::object_server::InterfaceRef<WillowDBus>) {
        *self.iface_ref.lock().await = Some(iface);
    }

    pub async fn handle_event(&self, event: ServiceEvent) {
        let iface = self.iface_ref.lock().await.clone();
        let Some(iface) = iface else {
            return;
        };
        let emitter = iface.signal_emitter();

        match event {
            ServiceEvent::ModeChanged { new_mode, old_mode } => {
                let _ = Self::mode_changed(&emitter, &new_mode, &old_mode).await;
            }
            ServiceEvent::BufferChanged(buffer) => {
                let _ = Self::buffer_changed(&emitter, &buffer).await;
            }
            ServiceEvent::PartialBufferChanged { partial, is_final } => {
                let _ = Self::partial_buffer_changed(&emitter, &partial, is_final).await;
            }
            ServiceEvent::CommandPending { phrase, blocked } => {
                let _ = Self::command_pending(&emitter, &phrase, blocked).await;
            }
            ServiceEvent::SpeakerVerificationFailed(reason) => {
                let _ = Self::speaker_verification_failed(&emitter, &reason).await;
            }
            ServiceEvent::CommandExecuted {
                command,
                phrase,
                confidence,
            } => {
                let _ = Self::command_executed(&emitter, &command, &phrase, confidence).await;
            }
            ServiceEvent::StatusChanged => {
                let status = self.core.get_status();
                let _ = Self::status_changed(&emitter, status).await;
            }
            ServiceEvent::Error { message, details } => {
                let _ = Self::error_occurred(&emitter, &message, &details).await;
            }
            ServiceEvent::Notification {
                title,
                message,
                urgency,
            } => {
                let _ = Self::notification(&emitter, &title, &message, &urgency).await;
            }
            ServiceEvent::ConfigChanged(config) => {
                let _ = Self::config_changed(&emitter, &config).await;
            }
        }
    }
}

#[interface(name = "com.github.saim.Willow")]
impl WillowDBus {
    async fn set_mode(&self, mode: &str) {
        if let Err(e) = self.core.set_mode(mode) {
            tracing::error!("SetMode failed: {e}");
        }
    }

    #[zbus(name = "GetMode")]
    async fn get_mode(&self) -> String {
        self.core.get_mode()
    }

    #[zbus(name = "GetStatus")]
    async fn get_status(&self) -> std::collections::HashMap<String, zvariant::OwnedValue> {
        self.core.get_status()
    }

    #[zbus(name = "GetConfig")]
    async fn get_config(&self) -> String {
        self.core.get_config()
    }

    #[zbus(name = "UpdateConfig")]
    async fn update_config(&self, config: &str) {
        if let Err(e) = self.core.update_config(config) {
            tracing::error!("UpdateConfig failed: {e}");
        }
    }

    #[zbus(name = "SetConfigValue")]
    async fn set_config_value(&self, key: &str, value: zvariant::Value<'_>) {
        if let Err(e) = self.core.set_config_value(key, value) {
            tracing::error!("SetConfigValue failed: {e}");
        }
    }

    #[zbus(name = "GetCommands")]
    async fn get_commands(&self) -> String {
        self.core.get_commands()
    }

    #[zbus(name = "AddCommand")]
    async fn add_command(&self, name: &str, command: &str, phrases: Vec<String>) {
        if let Err(e) = self.core.add_command(name, command, phrases) {
            tracing::error!("AddCommand failed: {e}");
        }
    }

    #[zbus(name = "RemoveCommand")]
    async fn remove_command(&self, name: &str) {
        if let Err(e) = self.core.remove_command(name) {
            tracing::error!("RemoveCommand failed: {e}");
        }
    }

    async fn start(&self) {
        if let Err(e) = self.core.start() {
            tracing::error!("Start failed: {e}");
        }
    }

    async fn stop(&self) {
        self.core.stop();
    }

    async fn restart(&self) {
        if let Err(e) = self.core.restart() {
            tracing::error!("Restart failed: {e}");
        }
    }

    #[zbus(name = "GetBuffer")]
    async fn get_buffer(&self) -> String {
        self.core.get_buffer()
    }

    #[zbus(name = "StartSpeakerEnrollment")]
    async fn start_speaker_enrollment(&self) {
        if let Err(e) = self.core.start_speaker_enrollment() {
            tracing::error!("StartSpeakerEnrollment failed: {e}");
        }
    }

    #[zbus(name = "CancelSpeakerEnrollment")]
    async fn cancel_speaker_enrollment(&self) {
        self.core.cancel_speaker_enrollment();
    }

    #[zbus(name = "GetSpeakerEnrollmentStatus")]
    async fn get_speaker_enrollment_status(
        &self,
    ) -> std::collections::HashMap<String, zvariant::OwnedValue> {
        self.core.enrollment_status()
    }

    #[zbus(name = "RemoveSpeakerProfile")]
    async fn remove_speaker_profile(&self) {
        self.core.remove_speaker_profile();
    }

    #[zbus(property, name = "IsRunning")]
    async fn is_running(&self) -> bool {
        self.core.is_running()
    }

    #[zbus(property, name = "CurrentMode")]
    async fn current_mode(&self) -> String {
        self.core.get_mode()
    }

    #[zbus(property, name = "CurrentBuffer")]
    async fn current_buffer(&self) -> String {
        self.core.get_buffer()
    }

    #[zbus(property, name = "Version")]
    async fn version(&self) -> String {
        "3.0.0".into()
    }

    #[zbus(signal, name = "ModeChanged")]
    async fn mode_changed(
        emitter: &SignalEmitter<'_>,
        new_mode: &str,
        old_mode: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "BufferChanged")]
    async fn buffer_changed(emitter: &SignalEmitter<'_>, buffer: &str) -> zbus::Result<()>;

    #[zbus(signal, name = "PartialBufferChanged")]
    async fn partial_buffer_changed(
        emitter: &SignalEmitter<'_>,
        partial: &str,
        is_final: bool,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "CommandPending")]
    async fn command_pending(
        emitter: &SignalEmitter<'_>,
        phrase: &str,
        blocked_by_prefix: bool,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "SpeakerVerificationFailed")]
    async fn speaker_verification_failed(
        emitter: &SignalEmitter<'_>,
        reason: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "CommandExecuted")]
    async fn command_executed(
        emitter: &SignalEmitter<'_>,
        command: &str,
        phrase: &str,
        confidence: f64,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "StatusChanged")]
    async fn status_changed(
        emitter: &SignalEmitter<'_>,
        status: std::collections::HashMap<String, zvariant::OwnedValue>,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "Error")]
    async fn error_occurred(
        emitter: &SignalEmitter<'_>,
        message: &str,
        details: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "Notification")]
    async fn notification(
        emitter: &SignalEmitter<'_>,
        title: &str,
        message: &str,
        urgency: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal, name = "ConfigChanged")]
    async fn config_changed(emitter: &SignalEmitter<'_>, config: &str) -> zbus::Result<()>;
}
