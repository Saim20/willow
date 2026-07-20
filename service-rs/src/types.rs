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
}

#[derive(Debug, Clone)]
pub struct CommandDispatchResult {
    pub handled: bool,
    pub pending: bool,
    pub blocked_by_prefix: bool,
    pub matched_phrase: String,
    pub command_action: String,
    pub command_name: String,
    pub confidence: f64,
    pub is_search: bool,
    pub search_engine: String,
    pub search_query: String,
    pub is_smart_open: bool,
    pub app_name: String,
}

impl Default for CommandDispatchResult {
    fn default() -> Self {
        Self {
            handled: false,
            pending: false,
            blocked_by_prefix: false,
            matched_phrase: String::new(),
            command_action: String::new(),
            command_name: String::new(),
            confidence: 0.0,
            is_search: false,
            search_engine: String::new(),
            search_query: String::new(),
            is_smart_open: false,
            app_name: String::new(),
        }
    }
}
