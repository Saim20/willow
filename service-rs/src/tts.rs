use std::process::Command;
use std::sync::mpsc;
use std::sync::Mutex;
use std::thread;

use crate::types::TtsConfig;

pub struct TtsEngine {
    config: Mutex<TtsConfig>,
    sender: Mutex<Option<mpsc::Sender<String>>>,
}

impl TtsEngine {
    pub fn new() -> Self {
        let use_spd = Command::new("sh")
            .args(["-c", "which spd-say >/dev/null 2>&1"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        let (tx, rx) = mpsc::channel::<String>();
        let worker_use_spd = use_spd;
        thread::spawn(move || {
            while let Ok(text) = rx.recv() {
                if text.is_empty() {
                    continue;
                }
                if worker_use_spd {
                    let _ = Command::new("spd-say").arg(&text).status();
                } else {
                    let _ = Command::new("espeak").arg(&text).status();
                }
            }
        });

        Self {
            config: Mutex::new(TtsConfig::default()),
            sender: Mutex::new(Some(tx)),
        }
    }

    pub fn update_config(&self, config: TtsConfig) {
        *self.config.lock().unwrap() = config;
    }

    pub fn speak(&self, text: &str) {
        let config = self.config.lock().unwrap().clone();
        if !config.enabled || text.is_empty() {
            return;
        }
        if let Some(tx) = self.sender.lock().unwrap().as_ref() {
            let _ = tx.send(text.to_string());
        }
    }
}

impl Drop for TtsEngine {
    fn drop(&mut self) {
        *self.sender.lock().unwrap() = None;
    }
}
