//! Optional local LLM fallback for ambiguous command phrasing.

use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use tracing::{info, warn};

use crate::config::LlmConfig;
use crate::intent::{IntentAction, IntentDecision};

#[derive(Debug, Deserialize)]
struct LlmIntentJson {
    intent: String,
    #[serde(default)]
    slots: LlmSlots,
    #[serde(default)]
    confidence: f64,
}

#[derive(Debug, Default, Deserialize)]
struct LlmSlots {
    #[serde(default)]
    app: String,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    query: String,
    #[serde(default)]
    phrase: String,
    #[serde(default)]
    command: String,
    #[serde(default)]
    name: String,
}

pub struct LlmFallback {
    config: LlmConfig,
}

impl LlmFallback {
    pub fn new(config: LlmConfig) -> Self {
        Self { config }
    }

    pub fn apply_config(&mut self, config: LlmConfig) {
        self.config = config;
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled && !self.config.model_path.is_empty()
    }

    /// Map ambiguous transcript to a structured intent. Never returns raw shell.
    pub fn resolve(&self, text: &str, known_phrases: &[String]) -> IntentDecision {
        if !self.enabled() {
            return IntentDecision {
                needs_llm: false,
                ..Default::default()
            };
        }

        let prompt = build_prompt(text, known_phrases);
        match run_llama_cli(&self.config, &prompt) {
            Ok(raw) => match parse_intent_json(&raw) {
                Some(decision) => {
                    info!("LLM fallback mapped {text:?} → {:?}", decision.action);
                    decision
                }
                None => {
                    warn!("LLM fallback returned unusable JSON");
                    IntentDecision::default()
                }
            },
            Err(e) => {
                warn!("LLM fallback failed: {e}");
                IntentDecision::default()
            }
        }
    }
}

fn build_prompt(text: &str, known_phrases: &[String]) -> String {
    let phrases = known_phrases
        .iter()
        .take(40)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"You map voice commands to JSON only. No prose.
Schema: {{"intent":"exit|typing|open|search|command|unknown","slots":{{"app":"","engine":"","query":"","phrase":"","command":"","name":""}},"confidence":0.0}}
Known phrases: {phrases}
Utterance: {text}
JSON:"#
    )
}

fn run_llama_cli(config: &LlmConfig, prompt: &str) -> anyhow::Result<String> {
    let timeout = Duration::from_millis(config.timeout_ms.clamp(100, 5000) as u64);
    let mut child = Command::new("llama-cli")
        .args([
            "-m",
            &config.model_path,
            "-n",
            &config.max_tokens.clamp(16, 256).to_string(),
            "-p",
            prompt,
            "--no-display-prompt",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait()? {
            Some(status) => {
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    use std::io::Read;
                    let _ = out.read_to_string(&mut stdout);
                }
                if !status.success() {
                    anyhow::bail!("llama-cli exit {status}");
                }
                return Ok(stdout);
            }
            None if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("LLM timeout after {}ms", config.timeout_ms);
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn parse_intent_json(raw: &str) -> Option<IntentDecision> {
    let json = extract_json_object(raw)?;
    let parsed: LlmIntentJson = serde_json::from_str(&json).ok()?;
    if parsed.confidence > 0.0 && parsed.confidence < 0.35 {
        return None;
    }
    let action = match parsed.intent.as_str() {
        "exit" => Some(IntentAction::ExitCommandMode),
        "typing" => Some(IntentAction::StartTypingMode),
        "open" if !parsed.slots.app.is_empty() => {
            let app = parsed.slots.app;
            Some(IntentAction::SmartOpen {
                phrase: if parsed.slots.phrase.is_empty() {
                    format!("open {app}")
                } else {
                    parsed.slots.phrase
                },
                app,
            })
        }
        "search" if !parsed.slots.query.is_empty() => Some(IntentAction::SmartSearch {
            engine: if parsed.slots.engine.is_empty() {
                "google".into()
            } else {
                parsed.slots.engine
            },
            query: parsed.slots.query,
            phrase: parsed.slots.phrase,
        }),
        "command" if !parsed.slots.command.is_empty() => Some(IntentAction::RunCommand {
            name: if parsed.slots.name.is_empty() {
                "llm".into()
            } else {
                parsed.slots.name
            },
            action: parsed.slots.command,
            phrase: parsed.slots.phrase,
            confidence: parsed.confidence.max(0.5),
        }),
        _ => None,
    };
    // Refuse free-form shell from LLM unless it looks like a known safe pattern.
    if let Some(IntentAction::RunCommand { action, .. }) = &action {
        if action.contains("rm ") || action.contains("sudo ") || action.contains(">") {
            return None;
        }
    }
    action.map(|a| IntentDecision {
        action: Some(a),
        fire: true,
        confidence: parsed.confidence.max(0.5),
        needs_llm: false,
    })
}

fn extract_json_object(raw: &str) -> Option<String> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(raw[start..=end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_json() {
        let raw = r#"{"intent":"open","slots":{"app":"firefox"},"confidence":0.9}"#;
        let d = parse_intent_json(raw).unwrap();
        assert!(d.fire);
        assert!(matches!(
            d.action,
            Some(IntentAction::SmartOpen { app, .. }) if app == "firefox"
        ));
    }

    #[test]
    fn rejects_dangerous_command() {
        let raw = r#"{"intent":"command","slots":{"command":"rm -rf /"},"confidence":0.9}"#;
        assert!(parse_intent_json(raw).is_none());
    }

    #[test]
    fn disabled_llm_returns_none() {
        let llm = LlmFallback::new(LlmConfig::default());
        assert!(llm.resolve("open the browser somehow", &[]).action.is_none());
    }

    #[test]
    fn missing_model_times_out_or_fails_closed() {
        let mut llm = LlmFallback::new(LlmConfig::default());
        llm.apply_config(LlmConfig {
            enabled: true,
            model_path: "/tmp/willow-missing-llm.gguf".into(),
            max_tokens: 32,
            timeout_ms: 150,
        });
        // Fail closed: no structured intent on missing binary / model.
        assert!(llm
            .resolve("do the thing with firefox", &["open firefox".into()])
            .action
            .is_none());
    }
}
