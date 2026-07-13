use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::process::Command;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Deserialize;
use tracing::{error, info, warn};

#[derive(Debug, Clone, Default)]
pub struct ContextConfig {
    pub default_apps: HashMap<String, String>,
    pub search_engines: HashMap<String, String>,
    pub app_aliases: HashMap<String, Vec<String>>,
}

pub struct CommandExecutor {
    log_file: String,
    context: ContextConfig,
    ydotool_available: bool,
    log_mutex: Mutex<()>,
}

#[derive(Deserialize)]
struct ContextFile {
    default_apps: Option<HashMap<String, String>>,
    search_engines: Option<HashMap<String, String>>,
    app_aliases: Option<HashMap<String, Vec<String>>>,
}

impl CommandExecutor {
    pub fn new() -> Self {
        let mut exec = Self {
            log_file: "/tmp/willow.log".into(),
            context: ContextConfig::default(),
            ydotool_available: Command::new("sh")
                .args(["-c", "which ydotool >/dev/null 2>&1"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false),
            log_mutex: Mutex::new(()),
        };
        exec.apply_defaults();
        if let Some(home) = dirs::home_dir() {
            let _ = exec.load_context(&home.join(".config/willow/context.json"));
        }
        exec
    }

    fn apply_defaults(&mut self) {
        self.context.default_apps = HashMap::from([
            ("browser".into(), "firefox".into()),
            ("terminal".into(), "kgx".into()),
            ("file_manager".into(), "nautilus".into()),
        ]);
        self.context.search_engines = HashMap::from([
            ("youtube".into(), "https://www.youtube.com/results?search_query=".into()),
            ("google".into(), "https://www.google.com/search?q=".into()),
            ("facebook".into(), "https://www.facebook.com/search/top?q=".into()),
            ("reddit".into(), "https://www.reddit.com/search/?q=".into()),
            ("wikipedia".into(), "https://en.wikipedia.org/wiki/Special:Search?search=".into()),
            ("github".into(), "https://github.com/search?q=".into()),
        ]);
        self.context.app_aliases = HashMap::from([
            ("browser".into(), vec!["firefox", "chromium", "google-chrome", "brave-browser"].into_iter().map(String::from).collect()),
            ("spotify".into(), vec!["spotify".into()]),
            ("vscode".into(), vec!["code", "code-oss", "vscodium"].into_iter().map(String::from).collect()),
        ]);
    }

    pub fn context(&self) -> &ContextConfig {
        &self.context
    }

    pub fn load_context(&mut self, path: &std::path::Path) -> Result<()> {
        if !path.is_file() {
            return Ok(());
        }
        let text = std::fs::read_to_string(path)?;
        let file: ContextFile = serde_json::from_str(&text)?;
        if let Some(v) = file.default_apps {
            self.context.default_apps = v;
        }
        if let Some(v) = file.search_engines {
            self.context.search_engines = v;
        }
        if let Some(v) = file.app_aliases {
            self.context.app_aliases = v;
        }
        Ok(())
    }

    pub fn log(&self, level: &str, message: &str) {
        let _guard = self.log_mutex.lock().unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&self.log_file) {
            let _ = writeln!(file, "{now} [{level}] {message}");
        }
        match level {
            "ERROR" => error!("{message}"),
            "WARNING" => warn!("{message}"),
            _ => info!("{message}"),
        }
    }

    pub fn execute_command(&self, command: &str) -> bool {
        self.log("INFO", &format!("Executing command: {command}"));
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return false;
        }
        let mut args = vec!["systemd-run", "--user", "--scope", "--slice=app.slice", "--"];
        args.extend(parts);
        let status = Command::new(args[0]).args(&args[1..]).status();
        if status.as_ref().map(|s| s.success()).unwrap_or(false) {
            self.log("INFO", "Command executed successfully");
            true
        } else {
            self.log("ERROR", &format!("Command failed: {command}"));
            false
        }
    }

    pub fn open_url(&self, url: &str) -> bool {
        self.log("INFO", &format!("Opening URL: {url}"));
        Command::new("systemd-run")
            .args(["--user", "--scope", "--slice=app.slice", "--", "xdg-open", url])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn type_text(&self, text: &str) {
        if text.is_empty() || !self.ydotool_available {
            return;
        }
        let _ = Command::new("ydotool").args(["type", text]).status();
    }

    pub fn press_key(&self, key: &str) {
        if !self.ydotool_available {
            return;
        }
        let _ = Command::new("ydotool").args(["key", key]).status();
    }

    pub fn match_phrase(&self, text: &str, phrase: &str) -> f64 {
        let lower_phrase = phrase.to_lowercase();
        if text.contains(&lower_phrase) {
            return 1.0;
        }
        let distance = levenshtein(text, &lower_phrase);
        let max_len = text.len().max(lower_phrase.len());
        if max_len == 0 {
            return 0.0;
        }
        let similarity = 1.0 - (distance as f64 / max_len as f64);
        similarity.max(token_overlap(text, &lower_phrase))
    }

    pub fn find_best_match<'a>(
        &self,
        text: &str,
        commands: &'a [crate::types::Command],
        threshold: f64,
    ) -> (Option<&'a crate::types::Command>, f64) {
        let mut best_cmd = None;
        let mut best_conf = 0.0;
        for cmd in commands {
            for phrase in &cmd.phrases {
                let conf = self.match_phrase(text, phrase);
                if conf > best_conf {
                    best_conf = conf;
                    best_cmd = Some(cmd);
                }
            }
        }
        if best_conf < threshold {
            (None, best_conf)
        } else {
            (best_cmd, best_conf)
        }
    }

    pub fn execute_smart_open(&self, app_name: &str) -> bool {
        let command = self.find_app(app_name);
        if command.is_empty() {
            self.log("WARNING", &format!("Application not found: {app_name}"));
            return false;
        }
        self.execute_command(&command);
        true
    }

    pub fn execute_smart_search(&self, engine: &str, query: &str) -> bool {
        let engine = engine.to_lowercase();
        let Some(base) = self.context.search_engines.get(&engine) else {
            return false;
        };
        let url = format!("{}{}", base, url_encode(query));
        self.open_url(&url)
    }

    fn find_app(&self, app_name: &str) -> String {
        let lower = app_name.to_lowercase();
        if self.is_available(&lower) {
            return lower;
        }
        if let Some(aliases) = self.context.app_aliases.get(&lower) {
            for alias in aliases {
                if self.is_available(alias) {
                    return alias.clone();
                }
            }
        }
        if let Some(default) = self.context.default_apps.get(&lower) {
            if self.is_available(default) {
                return default.clone();
            }
        }
        String::new()
    }

    fn is_available(&self, command: &str) -> bool {
        let name = command.split_whitespace().next().unwrap_or(command);
        Command::new("sh")
            .args(["-c", &format!("which {name} >/dev/null 2>&1")])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in dp.iter_mut().enumerate().take(a.len() + 1) {
        row[0] = i;
    }
    for (j, val) in dp[0].iter_mut().enumerate().take(b.len() + 1) {
        *val = j;
    }
    for (i, ca) in a.iter().enumerate() {
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            dp[i + 1][j + 1] = (dp[i][j + 1] + 1)
                .min(dp[i + 1][j] + 1)
                .min(dp[i][j] + cost);
        }
    }
    dp[a.len()][b.len()]
}

fn token_overlap(text: &str, phrase: &str) -> f64 {
    let text_tokens: Vec<_> = text.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    let phrase_tokens: Vec<_> = phrase.split(|c: char| !c.is_alphanumeric()).filter(|s| !s.is_empty()).collect();
    if phrase_tokens.is_empty() {
        return 0.0;
    }
    let matches = phrase_tokens
        .iter()
        .filter(|pt| text_tokens.contains(pt))
        .count();
    matches as f64 / phrase_tokens.len() as f64
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
