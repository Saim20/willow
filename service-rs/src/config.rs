use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::types::{Command, TtsConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WillowConfig {
    pub hotword: String,
    pub command_threshold: f64,
    pub speaker_verification: SpeakerConfig,
    pub kws: KwsConfig,
    pub streaming_asr: StreamingConfig,
    pub command_mode: CommandModeConfig,
    pub typing_mode: TypingModeConfig,
    pub tts: TtsSection,
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
    pub endpoint_silence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypingModeConfig {
    pub realtime: bool,
    pub max_backspace: i32,
    pub check_recent_chars: i32,
    pub exit_phrases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsSection {
    pub enabled: bool,
    pub events: TtsEvents,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsEvents {
    pub command_executed: bool,
    pub mode_changed: bool,
    pub search_executed: bool,
    pub errors: bool,
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
                endpoint_silence_command: 0.3,
                endpoint_silence_typing: 0.45,
            },
            command_mode: CommandModeConfig {
                endpoint_silence: 0.3,
            },
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
            tts: TtsSection {
                enabled: false,
                events: TtsEvents {
                    command_executed: false,
                    mode_changed: false,
                    search_executed: false,
                    errors: false,
                },
            },
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

    pub fn tts_config(&self) -> TtsConfig {
        TtsConfig {
            enabled: self.tts.enabled,
            command_executed: self.tts.events.command_executed,
            mode_changed: self.tts.events.mode_changed,
            search_executed: self.tts.events.search_executed,
            errors: self.tts.events.errors,
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
