use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;

use crate::commands::phrase_index::{normalize, CommandPhraseIndex};
use crate::commands::CommandExecutor;
use crate::types::Command;

#[derive(Debug, Clone)]
pub enum IntentAction {
    ExitCommandMode,
    StartTypingMode,
    RunCommand {
        name: String,
        action: String,
        phrase: String,
        confidence: f64,
    },
    SmartOpen {
        app: String,
        phrase: String,
    },
    SmartSearch {
        engine: String,
        query: String,
        phrase: String,
    },
    /// Waiting for more speech (prefix / incomplete slots).
    Pending {
        kind: PendingKind,
        prompt: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingKind {
    OpenApp,
    SearchQuery,
    IncompletePhrase,
}

#[derive(Debug, Clone)]
pub struct IntentDecision {
    pub action: Option<IntentAction>,
    /// Ready to fire now (exact match / complete slots).
    pub fire: bool,
    pub confidence: f64,
    pub needs_llm: bool,
}

impl Default for IntentDecision {
    fn default() -> Self {
        Self {
            action: None,
            fire: false,
            confidence: 0.0,
            needs_llm: false,
        }
    }
}

pub struct IntentEngine {
    phrase_index: CommandPhraseIndex,
    commands: Vec<Command>,
    executor: Arc<CommandExecutor>,
    threshold: f64,
    early_fire: bool,
    search_re_for: Regex,
    search_re_short: Regex,
    cancel_phrases: Vec<String>,
}

impl IntentEngine {
    pub fn new(executor: Arc<CommandExecutor>) -> Self {
        Self {
            phrase_index: CommandPhraseIndex::new(),
            commands: Vec::new(),
            executor,
            threshold: 0.8,
            early_fire: true,
            search_re_for: Regex::new(r"search\s+(\w+)\s+for\s+(.+)")
                .expect("valid search regex"),
            search_re_short: Regex::new(r"search\s+(\w+)\s+(.+)").expect("valid search regex"),
            cancel_phrases: vec![
                "cancel".into(),
                "never mind".into(),
                "nevermind".into(),
                "forget it".into(),
            ],
        }
    }

    pub fn set_commands(&mut self, commands: Vec<Command>) {
        self.commands = commands;
        self.phrase_index.build(&self.commands);
    }

    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
    }

    pub fn set_early_fire(&mut self, early_fire: bool) {
        self.early_fire = early_fire;
    }

    pub fn is_cancel(&self, text: &str) -> bool {
        let norm = normalize(text);
        self.cancel_phrases.iter().any(|p| norm == normalize(p) || norm.contains(&normalize(p)))
    }

    /// Match a streaming partial or Whisper/final transcript.
    /// `from_whisper`: search intents only fire from Whisper (VAD-segmented) finals.
    pub fn on_partial(&self, text: &str, is_endpoint: bool, from_whisper: bool) -> IntentDecision {
        let norm = normalize(text);
        if norm.is_empty() {
            return IntentDecision::default();
        }

        if self.is_cancel(&norm) {
            return IntentDecision {
                action: Some(IntentAction::ExitCommandMode),
                fire: true,
                confidence: 1.0,
                needs_llm: false,
            };
        }

        // Built-in mode phrases via command list.
        if let Some(d) = self.match_exact_command(&norm, is_endpoint) {
            return d;
        }

        // Hold while ASR is still completing a registered phrase ("open termin" → "open terminal").
        if !is_endpoint && self.phrase_index.is_prefix_of_registered_phrase(&norm) {
            return IntentDecision {
                action: Some(IntentAction::Pending {
                    kind: PendingKind::IncompletePhrase,
                    prompt: format!("Continue: {norm} …"),
                }),
                fire: false,
                confidence: 0.6,
                needs_llm: false,
            };
        }

        if let Some(d) = self.match_search(&norm, is_endpoint, from_whisper) {
            return d;
        }

        if let Some(d) = self.match_smart_open(&norm, is_endpoint) {
            return d;
        }

        if is_incomplete_prefix(&norm) {
            return IntentDecision {
                action: Some(IntentAction::Pending {
                    kind: PendingKind::IncompletePhrase,
                    prompt: format!("Continue: {norm} …"),
                }),
                fire: false,
                confidence: 0.5,
                needs_llm: false,
            };
        }

        if is_endpoint {
            // Fuzzy fallback for finals only.
            if let Some(d) = self.match_fuzzy(&norm) {
                return d;
            }
            return IntentDecision {
                action: None,
                fire: false,
                confidence: 0.0,
                needs_llm: true,
            };
        }

        IntentDecision::default()
    }

    fn match_exact_command(&self, norm: &str, is_endpoint: bool) -> Option<IntentDecision> {
        let lookup = self.phrase_index.lookup(norm);
        if !lookup.exact_match {
            return None;
        }
        if lookup.blocked_by_prefix && !is_endpoint {
            // Longer phrase may still arrive ("copy" vs "copy text").
            if !self.early_fire {
                return Some(IntentDecision {
                    action: Some(IntentAction::Pending {
                        kind: PendingKind::IncompletePhrase,
                        prompt: format!("Heard: {norm}"),
                    }),
                    fire: false,
                    confidence: 0.7,
                    needs_llm: false,
                });
            }
            // Early fire still waits briefly when prefix-blocked unless endpoint.
            return Some(IntentDecision {
                action: Some(IntentAction::Pending {
                    kind: PendingKind::IncompletePhrase,
                    prompt: format!("Heard: {norm}"),
                }),
                fire: false,
                confidence: 0.75,
                needs_llm: false,
            });
        }
        let m = lookup.matches.first()?;
        let action = match m.command_action.as_str() {
            "exit_command_mode" => IntentAction::ExitCommandMode,
            "start_typing_mode" => IntentAction::StartTypingMode,
            _ => IntentAction::RunCommand {
                name: m.command_name.clone(),
                action: m.command_action.clone(),
                phrase: m.phrase.clone(),
                confidence: 1.0,
            },
        };
        let fire = (self.early_fire || is_endpoint) && (!lookup.blocked_by_prefix || is_endpoint);
        Some(IntentDecision {
            action: Some(action),
            fire,
            confidence: 1.0,
            needs_llm: false,
        })
    }

    fn match_search(
        &self,
        norm: &str,
        is_endpoint: bool,
        from_whisper: bool,
    ) -> Option<IntentDecision> {
        let engines = &self.executor.context().search_engines;
        if let Some((engine, query)) =
            parse_search(norm, engines, &self.search_re_for, &self.search_re_short)
        {
            // Search always waits for a Whisper+VAD final — never early-fire from streaming.
            let fire = from_whisper && is_endpoint && !query.is_empty();
            if !fire {
                return Some(IntentDecision {
                    action: Some(IntentAction::Pending {
                        kind: PendingKind::SearchQuery,
                        prompt: if query.is_empty() {
                            format!("Search {engine} for what?")
                        } else {
                            format!("Search {engine} for {query}…")
                        },
                    }),
                    fire: false,
                    confidence: 0.7,
                    needs_llm: false,
                });
            }
            return Some(IntentDecision {
                action: Some(IntentAction::SmartSearch {
                    engine,
                    query,
                    phrase: norm.to_string(),
                }),
                fire: true,
                confidence: 1.0,
                needs_llm: false,
            });
        }
        // "search youtube for" without query yet
        if let Some(caps) = Regex::new(r"^search\s+(\w+)\s+for\s*$")
            .ok()
            .and_then(|re| re.captures(norm))
        {
            let engine = caps[1].to_string();
            if self.executor.is_known_engine(&engine) {
                return Some(IntentDecision {
                    action: Some(IntentAction::Pending {
                        kind: PendingKind::SearchQuery,
                        prompt: format!("Search {engine} for what?"),
                    }),
                    fire: false,
                    confidence: 0.6,
                    needs_llm: false,
                });
            }
        }
        if norm == "search"
            || (norm.starts_with("search ") && norm.split_whitespace().count() <= 2)
        {
            return Some(IntentDecision {
                action: Some(IntentAction::Pending {
                    kind: PendingKind::SearchQuery,
                    prompt: "Search which engine for what?".into(),
                }),
                fire: false,
                confidence: 0.4,
                needs_llm: false,
            });
        }
        None
    }

    fn match_smart_open(&self, norm: &str, is_endpoint: bool) -> Option<IntentDecision> {
        if let Some(app) = parse_smart_open(norm) {
            let known = self.executor.is_known_app(&app);
            let fire = is_endpoint || (self.early_fire && known);
            if !fire {
                return Some(IntentDecision {
                    action: Some(IntentAction::Pending {
                        kind: PendingKind::OpenApp,
                        prompt: format!("Open {app}?"),
                    }),
                    fire: false,
                    confidence: 0.7,
                    needs_llm: false,
                });
            }
            return Some(IntentDecision {
                action: Some(IntentAction::SmartOpen {
                    app,
                    phrase: norm.to_string(),
                }),
                fire: true,
                confidence: 1.0,
                needs_llm: false,
            });
        }
        if matches!(norm, "open" | "launch" | "start")
            || ["open", "launch", "start"]
                .iter()
                .any(|t| norm == format!("{t} the") || norm == format!("{t} a"))
        {
            return Some(IntentDecision {
                action: Some(IntentAction::Pending {
                    kind: PendingKind::OpenApp,
                    prompt: "Open which app?".into(),
                }),
                fire: false,
                confidence: 0.5,
                needs_llm: false,
            });
        }
        None
    }

    fn match_fuzzy(&self, norm: &str) -> Option<IntentDecision> {
        let (best, confidence) =
            self.executor
                .find_best_match(norm, &self.commands, self.threshold);
        let cmd = best?;
        let phrase = cmd
            .phrases
            .iter()
            .find(|p| self.executor.match_phrase(norm, p) >= self.threshold)
            .cloned()
            .or_else(|| cmd.phrases.first().cloned())
            .unwrap_or_default();
        let action = match cmd.command.as_str() {
            "exit_command_mode" => IntentAction::ExitCommandMode,
            "start_typing_mode" => IntentAction::StartTypingMode,
            _ => IntentAction::RunCommand {
                name: cmd.name.clone(),
                action: cmd.command.clone(),
                phrase,
                confidence,
            },
        };
        Some(IntentDecision {
            action: Some(action),
            fire: true,
            confidence,
            needs_llm: false,
        })
    }
}

fn is_incomplete_prefix(norm: &str) -> bool {
    matches!(norm, "open" | "launch" | "start" | "search")
        || norm.ends_with(" for")
        || ["open", "launch", "start", "search"]
            .iter()
            .any(|t| norm == format!("{t} the") || norm == format!("{t} a"))
}

fn parse_search(
    text: &str,
    engines: &HashMap<String, String>,
    re_for: &Regex,
    re_short: &Regex,
) -> Option<(String, String)> {
    let norm = normalize_search(text);
    if let Some(caps) = re_for.captures(&norm) {
        return Some((caps[1].to_string(), caps[2].to_string()));
    }
    if let Some(caps) = re_short.captures(&norm) {
        if &caps[2] != "for" {
            return Some((caps[1].to_string(), caps[2].to_string()));
        }
    }
    for engine in engines.keys() {
        if norm.len() > engine.len() + 1 && norm.starts_with(engine) {
            let rest = norm[engine.len()..].trim_start();
            if let Some(query) = rest.strip_prefix(' ') {
                return Some((engine.clone(), query.to_string()));
            }
        }
    }
    None
}

fn parse_smart_open(text: &str) -> Option<String> {
    let norm = normalize(text);
    for trigger in ["open ", "launch ", "start "] {
        if let Some(pos) = norm.find(trigger) {
            let app = norm[pos + trigger.len()..].trim();
            let app = app
                .strip_prefix("the ")
                .or_else(|| app.strip_prefix("a "))
                .unwrap_or(app)
                .trim();
            if !app.is_empty() {
                if trigger == "start " && matches!(app, "typing" | "dictation") {
                    continue;
                }
                return Some(app.to_string());
            }
        }
    }
    None
}

fn normalize_search(text: &str) -> String {
    normalize(text).replace(" four ", " for ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with_cmds() -> IntentEngine {
        let exec = Arc::new(CommandExecutor::new());
        let mut eng = IntentEngine::new(exec);
        eng.set_commands(vec![
            Command {
                name: "Terminal".into(),
                command: "kgx".into(),
                phrases: vec!["open terminal".into()],
            },
            Command {
                name: "Exit".into(),
                command: "exit_command_mode".into(),
                phrases: vec!["exit".into(), "cancel".into()],
            },
            Command {
                name: "Typing".into(),
                command: "start_typing_mode".into(),
                phrases: vec!["start typing".into()],
            },
        ]);
        eng.set_early_fire(true);
        eng
    }

    #[test]
    fn early_fires_exact_phrase() {
        let eng = engine_with_cmds();
        let d = eng.on_partial("open terminal", false, false);
        assert!(d.fire);
        assert!(matches!(d.action, Some(IntentAction::RunCommand { .. })));
    }

    #[test]
    fn pending_on_bare_open() {
        let eng = engine_with_cmds();
        let d = eng.on_partial("open", false, false);
        assert!(!d.fire);
        assert!(matches!(
            d.action,
            Some(IntentAction::Pending {
                kind: PendingKind::OpenApp | PendingKind::IncompletePhrase,
                ..
            })
        ));
    }

    #[test]
    fn smart_open_partial() {
        let eng = engine_with_cmds();
        // firefox is a known browser alias — may early-fire
        let d = eng.on_partial("open firefox", false, false);
        assert!(d.fire);
        assert!(matches!(
            d.action,
            Some(IntentAction::SmartOpen { app, .. }) if app == "firefox"
        ));
    }

    #[test]
    fn phrase_prefix_holds_mid_word() {
        let eng = engine_with_cmds();
        let d = eng.on_partial("open termin", false, false);
        assert!(!d.fire);
        assert!(matches!(
            d.action,
            Some(IntentAction::Pending {
                kind: PendingKind::IncompletePhrase,
                ..
            })
        ));
    }

    #[test]
    fn unknown_app_waits_for_endpoint() {
        let eng = engine_with_cmds();
        let mid = eng.on_partial("open xyzzy", false, false);
        assert!(!mid.fire);
        let end = eng.on_partial("open xyzzy", true, false);
        assert!(end.fire);
        assert!(matches!(
            end.action,
            Some(IntentAction::SmartOpen { app, .. }) if app == "xyzzy"
        ));
    }

    #[test]
    fn search_waits_for_whisper() {
        let eng = engine_with_cmds();
        let stream = eng.on_partial("search youtube for jazz", true, false);
        assert!(!stream.fire);
        assert!(matches!(
            stream.action,
            Some(IntentAction::Pending {
                kind: PendingKind::SearchQuery,
                ..
            })
        ));

        let whisper = eng.on_partial("search youtube for jazz", true, true);
        assert!(whisper.fire);
        assert!(matches!(
            whisper.action,
            Some(IntentAction::SmartSearch {
                engine,
                query,
                ..
            }) if engine == "youtube" && query == "jazz"
        ));
    }

    #[test]
    fn cancel_exits_command_mode() {
        let eng = engine_with_cmds();
        let d = eng.on_partial("never mind", true, false);
        assert!(d.fire);
        assert!(matches!(d.action, Some(IntentAction::ExitCommandMode)));
    }

    #[test]
    fn early_fire_disabled_waits_for_endpoint() {
        let mut eng = engine_with_cmds();
        eng.set_early_fire(false);
        let mid = eng.on_partial("open terminal", false, false);
        assert!(!mid.fire);
        let end = eng.on_partial("open terminal", true, false);
        assert!(end.fire);
    }
}
