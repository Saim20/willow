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

    /// True when `text` is a proper prefix of a registered phrase (mid-word OK).
    /// Prevents SmartOpen from stealing `"open termin"` before `"open terminal"`.
    pub fn is_prefix_of_registered_phrase(&self, text: &str) -> bool {
        let norm = normalize(text);
        if norm.is_empty() {
            return false;
        }
        self.entries.iter().any(|e| {
            e.phrase.len() > norm.len() && e.phrase.starts_with(&norm)
        })
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

/// Match a sherpa KWS token against a plain-text phrase (hotword or command phrase).
pub fn kws_keyword_matches(detected: &str, phrase: &str) -> bool {
    let det = normalize_kws_keyword(detected);
    let reference = normalize(phrase);
    if det.is_empty() || reference.is_empty() {
        return false;
    }
    if det == reference {
        return true;
    }
    // @hey_willow → "@hey willow" after normalization; strip leading @
    if let Some(stripped) = det.strip_prefix('@') {
        if normalize(stripped) == reference {
            return true;
        }
    }
    // Sherpa @TAG alias for this phrase (e.g. @HEY_WILLOW).
    let tag = phrase_to_kws_tag(phrase);
    if !tag.is_empty() {
        let tag_norm = normalize_kws_keyword(&tag);
        if det == tag_norm || det.ends_with(&tag_norm) {
            return true;
        }
    }
    // Token-split detections: "HE Y WILL OW" vs "hey willow"
    if collapse_alnum(&det) == collapse_alnum(&reference) {
        return true;
    }
    if !tag.is_empty() && collapse_alnum(&det) == collapse_alnum(&tag) {
        return true;
    }
    false
}

fn collapse_alnum(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Build the sherpa @TAG form from a plain phrase (matches generate-keywords.py convention).
fn phrase_to_kws_tag(phrase: &str) -> String {
    let norm = normalize(phrase);
    if norm.is_empty() {
        return String::new();
    }
    format!(
        "@{}",
        norm.replace(' ', "_").to_uppercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kws_matches_exact_phrase() {
        assert!(kws_keyword_matches("hey willow", "hey willow"));
        assert!(kws_keyword_matches("@hey_willow", "hey willow"));
        assert!(kws_keyword_matches("HEY_WILLOW", "hey willow"));
    }

    #[test]
    fn kws_matches_bpe_tokens() {
        assert!(kws_keyword_matches("▁HE Y ▁WILL OW @HEY_WILLOW", "hey willow"));
    }

    #[test]
    fn kws_matches_token_split_hotword() {
        assert!(kws_keyword_matches("HE Y WILL OW", "hey willow"));
        assert!(kws_keyword_matches("HEY WILLOW", "hey willow"));
        assert!(kws_keyword_matches("@HEY_WILLOW", "hey willow"));
    }

    #[test]
    fn kws_rejects_partial_overlap() {
        assert!(!kws_keyword_matches("willow", "hey willow"));
        assert!(!kws_keyword_matches("hey", "hey willow"));
        assert!(!kws_keyword_matches("exit typing mode", "exit"));
    }

    #[test]
    fn kws_matches_mode_phrases() {
        assert!(kws_keyword_matches("@EXIT", "exit"));
        assert!(kws_keyword_matches("stop typing", "stop typing"));
    }
}
