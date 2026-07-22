use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Normal,
    Command,
    Typing,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Command => "command",
            Mode::Typing => "typing",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "command" => Mode::Command,
            "typing" => Mode::Typing,
            _ => Mode::Normal,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub command: String,
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentState {
    Idle,
    Recording,
    Complete,
    Failed,
}

impl EnrollmentState {
    pub fn as_str(self) -> &'static str {
        match self {
            EnrollmentState::Idle => "idle",
            EnrollmentState::Recording => "recording",
            EnrollmentState::Complete => "complete",
            EnrollmentState::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub is_final: bool,
    pub is_endpoint: bool,
    /// Whisper offline finals (Command search / Typing). Streaming must not fire search.
    pub from_whisper: bool,
    /// Streaming hypothesis held long enough to early-fire (set by StreamAsrEngine).
    pub is_stable: bool,
}
