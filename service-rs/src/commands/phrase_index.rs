use std::collections::HashMap;

use crate::types::Command;

#[derive(Clone)]
pub struct PhraseEntry {
    pub phrase: String,
    pub command_name: String,
    pub command_action: String,
    pub has_prefix_extension: bool,
}

#[derive(Default)]
pub struct PhraseLookupResult {
    pub exact_match: bool,
    pub blocked_by_prefix: bool,
    pub matches: Vec<PhraseEntry>,
}

pub struct CommandPhraseIndex {
    entries: Vec<PhraseEntry>,
    phrase_index: HashMap<String, Vec<usize>>,
}

impl CommandPhraseIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            phrase_index: HashMap::new(),
        }
    }

    pub fn build(&mut self, commands: &[Command]) {
        self.entries.clear();
        self.phrase_index.clear();

        for cmd in commands {
            for phrase in &cmd.phrases {
                let norm = normalize(phrase);
                if norm.is_empty() {
                    continue;
                }
                let idx = self.entries.len();
                self.entries.push(PhraseEntry {
                    phrase: norm.clone(),
                    command_name: cmd.name.clone(),
                    command_action: cmd.command.clone(),
                    has_prefix_extension: false,
                });
                self.phrase_index.entry(norm).or_default().push(idx);
            }
        }

        for i in 0..self.entries.len() {
            let phrase = self.entries[i].phrase.clone();
            self.entries[i].has_prefix_extension = self.entries.iter().any(|other| {
                other.phrase != phrase
                    && other.phrase.len() > phrase.len()
                    && other.phrase.starts_with(&phrase)
                    && other.phrase.as_bytes().get(phrase.len()) == Some(&b' ')
            });
        }
    }

    pub fn lookup(&self, text: &str) -> PhraseLookupResult {
        let norm = normalize(text);
        let mut result = PhraseLookupResult::default();
        for entry in &self.entries {
            if norm == entry.phrase {
                result.exact_match = true;
                if entry.has_prefix_extension {
                    result.blocked_by_prefix = true;
                }
                result.matches.push(entry.clone());
            }
        }
        result
    }
}

impl Default for CommandPhraseIndex {
    fn default() -> Self {
        Self::new()
    }
}

pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn normalize_kws_keyword(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            result.push(' ');
            continue;
        }
        if c == '▁' {
            result.push(' ');
            continue;
        }
        result.push(c.to_ascii_lowercase());
    }
    normalize(&result)
}

pub fn kws_keyword_matches(detected: &str, phrase: &str) -> bool {
    let det = normalize_kws_keyword(detected);
    let reference = normalize(phrase);
    if det.is_empty() || reference.is_empty() {
        return false;
    }
    det == reference || det.contains(&reference) || reference.contains(&det)
}
