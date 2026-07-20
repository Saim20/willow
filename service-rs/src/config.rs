use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::types::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WillowConfig {
    pub hotword: String,
    pub command_threshold: f64,
    pub speaker_verification: SpeakerConfig,
    pub kws: KwsConfig,
    pub streaming_asr: StreamingConfig,
    pub command_mode: CommandModeConfig,
    pub typing_mode: TypingModeConfig,
    #[serde(default)]
    pub inference: InferenceConfig,
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerConfig {
    pub enabled: bool,
    pub threshold: f32,
    pub enrolled_user: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KwsConfig {
    pub threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    pub endpoint_silence_command: f32,
    pub endpoint_silence_typing: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandModeConfig {
    /// Silence (seconds) after speech before the utterance is closed and executed.
    #[serde(default = "default_endpoint_silence")]
    pub endpoint_silence: f32,
    /// How long to wait for the rest of a bare prefix like "open …".
    #[serde(default = "default_incomplete_hold")]
    pub incomplete_hold: f32,
    /// Return to Normal after this many idle seconds in Command/Typing.
    #[serde(default = "default_session_idle")]
    pub session_idle: f32,
    /// Minimum speech length for Silero VAD (lower = catch short leading words).
    #[serde(default = "default_min_speech_duration")]
    pub min_speech_duration: f32,
    /// Silero speech probability threshold (lower = more sensitive onset).
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: f32,
    /// Seconds of silence prepended before Whisper (helps first-word accuracy).
    #[serde(default = "default_whisper_pre_pad")]
    pub whisper_pre_pad: f32,
    /// Extra audio from before VAD onset, prepended to each segment.
    #[serde(default = "default_preroll")]
    pub preroll: f32,
}

fn default_endpoint_silence() -> f32 {
    // Long enough that natural pauses between words don't split "open firefox".
    0.5
}
fn default_incomplete_hold() -> f32 {
    1.5
}
fn default_session_idle() -> f32 {
    12.0
}
fn default_min_speech_duration() -> f32 {
    0.1
}
fn default_vad_threshold() -> f32 {
    0.45
}
fn default_whisper_pre_pad() -> f32 {
    0.15
}
fn default_preroll() -> f32 {
    0.15
}

impl Default for CommandModeConfig {
    fn default() -> Self {
        Self {
            endpoint_silence: default_endpoint_silence(),
            incomplete_hold: default_incomplete_hold(),
            session_idle: default_session_idle(),
            min_speech_duration: default_min_speech_duration(),
            vad_threshold: default_vad_threshold(),
            whisper_pre_pad: default_whisper_pre_pad(),
            preroll: default_preroll(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// `auto` (prefer CUDA), `cuda`, or `cpu`.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// `0` = auto (up to 4 threads).
    #[serde(default)]
    pub num_threads: i32,
}

fn default_provider() -> String {
    "auto".into()
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            num_threads: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingModeConfig {
    pub realtime: bool,
    pub max_backspace: i32,
    pub check_recent_chars: i32,
    pub exit_phrases: Vec<String>,
}

impl Default for WillowConfig {
    fn default() -> Self {
        Self {
            hotword: "hey willow".into(),
            command_threshold: 80.0,
            speaker_verification: SpeakerConfig {
                enabled: false,
                threshold: 0.65,
                enrolled_user: "owner".into(),
            },
            kws: KwsConfig { threshold: 0.25 },
            streaming_asr: StreamingConfig {
                endpoint_silence_command: default_endpoint_silence(),
                endpoint_silence_typing: 0.45,
            },
            command_mode: CommandModeConfig::default(),
            typing_mode: TypingModeConfig {
                realtime: false,
                max_backspace: 80,
                check_recent_chars: 100,
                exit_phrases: vec![
                    "stop typing".into(),
                    "exit typing".into(),
                    "normal mode".into(),
                    "go to normal mode".into(),
                ],
            },
            inference: InferenceConfig::default(),
            commands: default_commands(),
        }
    }
}

impl WillowConfig {
    pub fn command_threshold_fraction(&self) -> f64 {
        if self.command_threshold > 1.0 {
            self.command_threshold / 100.0
        } else {
            self.command_threshold
        }
    }

    pub fn normal_mode_phrases(&self) -> Vec<String> {
        self.commands
            .iter()
            .filter(|c| c.command == "exit_command_mode")
            .flat_map(|c| c.phrases.clone())
            .collect()
    }

    pub fn typing_mode_phrases(&self) -> Vec<String> {
        self.commands
            .iter()
            .filter(|c| c.command == "start_typing_mode")
            .flat_map(|c| c.phrases.clone())
            .collect()
    }

    pub fn kws_phrases(&self) -> Vec<String> {
        let mut phrases = vec![self.hotword.clone()];
        phrases.extend(self.typing_mode.exit_phrases.clone());
        for cmd in &self.commands {
            if cmd.command == "exit_command_mode" || cmd.command == "start_typing_mode" {
                phrases.extend(cmd.phrases.clone());
            }
        }
        phrases
    }
}

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".config/willow/config.json"))
        .unwrap_or_else(|| PathBuf::from("/tmp/willow/config.json"))
}

pub fn load_config() -> Result<WillowConfig> {
    let path = config_path();
    if !path.exists() {
        let system = PathBuf::from("/usr/share/willow/config.json");
        if system.exists() {
            fs::create_dir_all(path.parent().unwrap())?;
            fs::copy(&system, &path)?;
        } else {
            return Ok(WillowConfig::default());
        }
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let mut cfg: WillowConfig = serde_json::from_str(&text)?;
    if cfg.commands.is_empty() {
        cfg.commands = default_commands();
    }
    Ok(cfg)
}

pub fn save_config(cfg: &WillowConfig) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(cfg)?;
    fs::write(path, text)?;
    Ok(())
}

fn default_commands() -> Vec<Command> {
    serde_json::from_value(serde_json::json!([
        {"name": "Terminal", "command": "kgx", "phrases": ["open terminal", "start terminal", "launch terminal"]},
        {"name": "Firefox", "command": "firefox", "phrases": ["open firefox", "launch firefox", "start web browser"]},
        {"name": "Copy", "command": "ydotool key 29:1 46:1 46:0 29:0", "phrases": ["copy", "copy text"]},
        {"name": "Paste", "command": "ydotool key 29:1 47:1 47:0 29:0", "phrases": ["paste", "paste text"]},
        {"name": "Move Left Workspace", "command": "ydotool key 125:1 42:1 30:1 30:0 42:0 125:0", "phrases": ["move left", "go left", "left desktop"]},
        {"name": "Move Right Workspace", "command": "ydotool key 125:1 42:1 32:1 32:0 42:0 125:0", "phrases": ["move right", "go right", "right desktop"]},
        {"name": "Exit Command Mode", "command": "exit_command_mode", "phrases": ["exit", "cancel", "stop listening", "normal mode", "go back"]},
        {"name": "Start Typing Mode", "command": "start_typing_mode", "phrases": ["start typing", "typing mode", "begin typing", "dictation mode", "start dictation"]}
    ]))
    .unwrap_or_default()
}
