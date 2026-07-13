use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use regex::Regex;

use super::executor::CommandExecutor;
use super::phrase_index::{normalize, CommandPhraseIndex};
use crate::types::{Command, CommandDispatchResult};

pub struct CommandIntentResolver {
    executor: Arc<CommandExecutor>,
    phrase_index: CommandPhraseIndex,
    commands: Vec<Command>,
    threshold: f64,
    history: Mutex<Vec<(String, Instant)>>,
    search_re_for: Regex,
    search_re_short: Regex,
}

impl CommandIntentResolver {
    pub fn new(executor: Arc<CommandExecutor>) -> Self {
        Self {
            executor,
            phrase_index: CommandPhraseIndex::new(),
            commands: Vec::new(),
            threshold: 0.8,
            history: Mutex::new(Vec::new()),
            search_re_for: Regex::new(r"search\s+(\w+)\s+for\s+(.+)")
                .expect("valid search regex"),
            search_re_short: Regex::new(r"search\s+(\w+)\s+(.+)").expect("valid search regex"),
        }
    }

    pub fn set_commands(&mut self, commands: Vec<Command>) {
        self.commands = commands;
        self.phrase_index.build(&self.commands);
    }

    pub fn set_threshold(&mut self, threshold: f64) {
        self.threshold = threshold;
    }

    pub fn process_partial(&self, text: &str) -> CommandDispatchResult {
        let norm = normalize(text);
        if norm.starts_with("search") {
            return CommandDispatchResult {
                pending: true,
                ..Default::default()
            };
        }
        let lookup = self.phrase_index.lookup(&norm);
        if lookup.exact_match {
            let mut result = CommandDispatchResult {
                pending: true,
                blocked_by_prefix: lookup.blocked_by_prefix,
                ..Default::default()
            };
            if let Some(m) = lookup.matches.first() {
                result.matched_phrase = m.phrase.clone();
                result.command_name = m.command_name.clone();
                result.command_action = m.command_action.clone();
            }
            return result;
        }
        CommandDispatchResult::default()
    }

    pub fn process_endpoint(&self, text: &str) -> CommandDispatchResult {
        if let Some((engine, query)) =
            parse_search(text, &self.executor.context().search_engines, &self.search_re_for, &self.search_re_short)
        {
            let key = format!("smart_search_{engine}_{query}");
            if self.is_duplicate(&key) {
                return CommandDispatchResult::default();
            }
            self.record_execution(&key);
            return CommandDispatchResult {
                handled: true,
                is_search: true,
                search_engine: engine,
                search_query: query,
                matched_phrase: text.to_string(),
                confidence: 1.0,
                ..Default::default()
            };
        }

        if let Some(app) = parse_smart_open(text) {
            let key = format!("smart_open_{app}");
            if self.is_duplicate(&key) {
                return CommandDispatchResult::default();
            }
            self.record_execution(&key);
            return CommandDispatchResult {
                handled: true,
                is_smart_open: true,
                app_name: app.clone(),
                matched_phrase: format!("open {app}"),
                confidence: 1.0,
                ..Default::default()
            };
        }

        let norm = normalize(text);
        let lookup = self.phrase_index.lookup(&norm);
        if lookup.exact_match && !lookup.blocked_by_prefix {
            if let Some(m) = lookup.matches.first() {
                if self.is_duplicate(&m.command_name) {
                    return CommandDispatchResult::default();
                }
                self.record_execution(&m.command_name);
                return CommandDispatchResult {
                    handled: true,
                    matched_phrase: m.phrase.clone(),
                    command_name: m.command_name.clone(),
                    command_action: m.command_action.clone(),
                    confidence: 1.0,
                    ..Default::default()
                };
            }
        }

        self.match_fuzzy(&norm)
    }

    fn match_fuzzy(&self, text: &str) -> CommandDispatchResult {
        let (best, confidence) = self.executor.find_best_match(text, &self.commands, self.threshold);
        if let Some(cmd) = best {
            if self.is_duplicate(&cmd.name) {
                return CommandDispatchResult::default();
            }
            let matched_phrase = cmd
                .phrases
                .iter()
                .find(|p| self.executor.match_phrase(text, p) >= self.threshold)
                .cloned()
                .or_else(|| cmd.phrases.first().cloned())
                .unwrap_or_default();
            self.record_execution(&cmd.name);
            return CommandDispatchResult {
                handled: true,
                command_name: cmd.name.clone(),
                command_action: cmd.command.clone(),
                matched_phrase,
                confidence,
                ..Default::default()
            };
        }
        CommandDispatchResult::default()
    }

    fn is_duplicate(&self, key: &str) -> bool {
        self.clean_history();
        let history = self.history.lock().unwrap();
        history
            .iter()
            .any(|(name, ts)| name == key && ts.elapsed() < Duration::from_secs(2))
    }

    fn record_execution(&self, key: &str) {
        self.history.lock().unwrap().push((key.to_string(), Instant::now()));
    }

    fn clean_history(&self) {
        let mut history = self.history.lock().unwrap();
        history.retain(|(_, ts)| ts.elapsed() < Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_returns_not_handled() {
        let executor = Arc::new(CommandExecutor::new());
        let mut resolver = CommandIntentResolver::new(executor);
        resolver.set_commands(vec![crate::types::Command {
            name: "Terminal".into(),
            command: "kgx".into(),
            phrases: vec!["open terminal".into()],
        }]);
        let first = resolver.process_endpoint("open terminal");
        assert!(first.handled);
        let dup = resolver.process_endpoint("open terminal");
        assert!(!dup.handled);
    }
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
            if !app.is_empty() {
                return Some(app.to_string());
            }
        }
    }
    None
}

fn normalize_search(text: &str) -> String {
    let norm = normalize(text);
    norm.replace(" four ", " for ")
}
