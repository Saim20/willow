use std::process::Command;
use std::sync::Mutex;
use std::thread;

use crate::types::TtsConfig;

pub struct TtsEngine {
    config: Mutex<TtsConfig>,
    queue: Mutex<Vec<String>>,
    use_spd: bool,
}

impl TtsEngine {
    pub fn new() -> Self {
        let use_spd = Command::new("sh")
            .args(["-c", "which spd-say >/dev/null 2>&1"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        Self {
            config: Mutex::new(TtsConfig::default()),
            queue: Mutex::new(Vec::new()),
            use_spd,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.use_spd
            || Command::new("sh")
                .args(["-c", "which espeak >/dev/null 2>&1"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
    }

    pub fn update_config(&self, config: TtsConfig) {
        *self.config.lock().unwrap() = config;
    }

    pub fn speak(&self, text: &str) {
        let config = self.config.lock().unwrap().clone();
        if !config.enabled || text.is_empty() {
            return;
        }
        let text = text.to_string();
        let use_spd = self.use_spd;
        thread::spawn(move || {
            if use_spd {
                let _ = Command::new("spd-say").arg(&text).status();
            } else {
                let _ = Command::new("espeak").arg(&text).status();
            }
        });
    }
}
