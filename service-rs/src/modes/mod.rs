use std::sync::Arc;

use crate::commands::phrase_index::normalize;
use crate::commands::CommandWorker;
use crate::config::WillowConfig;
use crate::pipeline::SpeechPipeline;
use crate::types::{CommandDispatchResult, Mode, TranscriptionResult, TtsConfig};
use crate::tts::TtsEngine;

pub struct ModeStateMachine {
    mode: Mode,
    buffer: String,
    typing_last_typed: String,
    worker: CommandWorker,
    tts: Arc<TtsEngine>,
    tts_config: TtsConfig,
    typing_realtime: bool,
    typing_max_backspace: i32,
    typing_exit_phrases: Vec<String>,
}

impl ModeStateMachine {
    pub fn new(worker: CommandWorker, tts: Arc<TtsEngine>) -> Self {
        Self {
            mode: Mode::Normal,
            buffer: String::new(),
            typing_last_typed: String::new(),
            worker,
            tts,
            tts_config: TtsConfig::default(),
            typing_realtime: false,
            typing_max_backspace: 80,
            typing_exit_phrases: vec![
                "stop typing".into(),
                "exit typing".into(),
                "normal mode".into(),
                "go to normal mode".into(),
            ],
        }
    }

    pub fn apply_config(&mut self, config: &WillowConfig) {
        self.tts_config = config.tts_config();
        self.typing_realtime = config.typing_mode.realtime;
        self.typing_max_backspace = config.typing_mode.max_backspace;
        self.typing_exit_phrases = config.typing_mode.exit_phrases.clone();
        self.tts.update_config(self.tts_config.clone());
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.buffer.clear();
        self.typing_last_typed.clear();
    }

    pub fn set_mode_with_pipeline(&mut self, mode: Mode, pipeline: &mut SpeechPipeline) {
        self.set_mode(mode);
        pipeline.set_mode(mode);
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn handle_keyword(
        &mut self,
        keyword: &str,
        pipeline: &mut SpeechPipeline,
    ) -> Option<ModeChange> {
        match keyword {
            "command" | "normal" | "typing" => {
                if keyword == "command" && !pipeline.ready_for_command() {
                    if self.tts_config.errors {
                        self.tts
                            .speak("Speech recognition is not available");
                    }
                    return None;
                }
                let new_mode = Mode::from_str(keyword);
                let old = self.mode;
                if new_mode != old {
                    self.set_mode_with_pipeline(new_mode, pipeline);
                    self.speak_mode(new_mode);
                    return Some(ModeChange { new_mode, old_mode: old });
                }
                None
            }
            _ => None,
        }
    }

    pub fn handle_transcription(
        &mut self,
        result: &TranscriptionResult,
        pipeline: &mut SpeechPipeline,
    ) -> Option<ModeChange> {
        if self.mode == Mode::Normal {
            return None;
        }

        self.buffer = result.text.clone();

        match self.mode {
            Mode::Command => {
                if result.is_endpoint {
                    let dispatch = pipeline.resolver.process_endpoint(&result.text);
                    self.dispatch_command(dispatch, pipeline);
                    pipeline.asr.reset_stream();
                }
                None
            }
            Mode::Typing => {
                let text = format_for_typing(&result.text);
                if self.check_exit_phrases(&text) {
                    let old = self.mode;
                    self.set_mode_with_pipeline(Mode::Normal, pipeline);
                    self.speak_mode(Mode::Normal);
                    return Some(ModeChange {
                        new_mode: Mode::Normal,
                        old_mode: old,
                    });
                }
                if result.is_final || result.is_endpoint {
                    self.commit_typing_phrase(&text);
                    self.typing_last_typed.clear();
                    pipeline.asr.reset_stream();
                } else if self.typing_realtime {
                    self.apply_typing_delta(&text);
                }
                None
            }
            Mode::Normal => None,
        }
    }

    fn dispatch_command(&mut self, result: CommandDispatchResult, pipeline: &mut SpeechPipeline) {
        if !result.handled {
            if result.pending {
                return;
            }
            if self.tts_config.errors {
                self.tts.speak("Sorry, I didn't understand that");
            }
            return;
        }

        if result.command_action.is_empty()
            && !result.is_search
            && !result.is_smart_open
        {
            return;
        }

        if result.is_search {
            self.worker.smart_search(
                result.search_engine,
                result.search_query,
                result.matched_phrase,
                result.confidence,
                self.tts_config.search_executed,
            );
            return;
        }

        if result.is_smart_open {
            self.worker.smart_open(
                result.app_name,
                result.matched_phrase,
                result.confidence,
                self.tts_config.command_executed,
                self.tts_config.errors,
            );
            return;
        }

        if result.command_action == "exit_command_mode" {
            self.set_mode_with_pipeline(Mode::Normal, pipeline);
            self.speak_mode(Mode::Normal);
            return;
        }
        if result.command_action == "start_typing_mode" {
            self.set_mode_with_pipeline(Mode::Typing, pipeline);
            self.speak_mode(Mode::Typing);
            return;
        }

        self.worker.execute(
            result.command_action,
            result.matched_phrase,
            result.confidence,
            self.tts_config.command_executed,
            self.tts_config.errors,
        );
    }

    fn speak_mode(&self, mode: Mode) {
        if self.tts_config.mode_changed {
            self.tts.speak(&format!("{} mode", mode.as_str()));
        }
    }

    pub fn speak_speaker_rejected(&self) {
        if self.tts_config.errors {
            self.tts.speak("Voice not recognized");
        }
    }

    fn check_exit_phrases(&self, text: &str) -> bool {
        self.typing_exit_phrases.iter().any(|phrase| {
            let p = normalize(phrase);
            text.contains(&p)
        })
    }

    /// Type a completed phrase once (default typing path — no partial churn).
    fn commit_typing_phrase(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.typing_realtime {
            let suffix = typing_suffix_after(&self.typing_last_typed, text);
            if !suffix.is_empty() {
                self.worker.type_text(&suffix);
                self.worker.type_text(" ");
            }
        } else {
            self.worker.type_text(text);
            self.worker.type_text(" ");
        }
    }

    /// Realtime path: word-aligned diff so ASR revisions backspace correctly.
    fn apply_typing_delta(&mut self, new_text: &str) {
        if new_text == self.typing_last_typed {
            return;
        }
        let (chars_to_delete, to_type) =
            typing_word_delta(&self.typing_last_typed, new_text, self.typing_max_backspace);
        for _ in 0..chars_to_delete {
            self.worker.press_key("14:1 14:0");
        }
        if !to_type.is_empty() {
            self.worker.type_text(&to_type);
        }
        self.typing_last_typed = new_text.to_string();
    }
}

/// Lowercase + normalized whitespace for typed output.
fn format_for_typing(text: &str) -> String {
    normalize(text)
}

/// Characters to backspace and text to type after a word-aligned diff.
fn typing_word_delta(old: &str, new: &str, max_backspace: i32) -> (usize, String) {
    if new.is_empty() {
        return (0, String::new());
    }
    if old.is_empty() {
        return (0, new.to_string());
    }

    let old_words: Vec<&str> = old.split_whitespace().collect();
    let new_words: Vec<&str> = new.split_whitespace().collect();

    let mut shared = 0usize;
    while shared < old_words.len()
        && shared < new_words.len()
        && old_words[shared] == new_words[shared]
    {
        shared += 1;
    }

    let deleted_words = &old_words[shared..];
    let chars_to_delete: usize = if deleted_words.is_empty() {
        0
    } else {
        deleted_words.join(" ").len() + 1
    };
    let capped = chars_to_delete.min(max_backspace.max(0) as usize);

    let to_type = if shared < new_words.len() {
        let mut suffix = new_words[shared..].join(" ");
        if shared > 0 && !suffix.is_empty() {
            suffix.insert(0, ' ');
        }
        suffix
    } else {
        String::new()
    };

    (capped, to_type)
}

/// Suffix of `new` not already covered by `old` when both describe the same utterance.
fn typing_suffix_after(old: &str, new: &str) -> String {
    if old.is_empty() {
        return new.to_string();
    }
    if new.starts_with(old) {
        let rest = new[old.len()..].trim_start();
        return rest.to_string();
    }
    if old.starts_with(new) {
        return String::new();
    }
    let (chars_to_delete, to_type) = typing_word_delta(old, new, i32::MAX);
    let _ = chars_to_delete;
    to_type.trim_start().to_string()
}

#[derive(Debug, Clone, Copy)]
pub struct ModeChange {
    pub new_mode: Mode,
    pub old_mode: Mode,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_for_typing_lowercases() {
        assert_eq!(format_for_typing("  HELLO   WORLD  "), "hello world");
    }

    #[test]
    fn word_delta_extends_prefix() {
        let (del, typed) = typing_word_delta("hello wor", "hello world", 80);
        assert_eq!(del, 4);
        assert_eq!(typed, " world");
    }

    #[test]
    fn word_delta_revises_middle_word() {
        let (del, typed) = typing_word_delta("the quick brown", "the fast brown", 80);
        assert_eq!(del, 12);
        assert_eq!(typed, " fast brown");
    }

    #[test]
    fn commit_suffix_after_partial() {
        assert_eq!(
            typing_suffix_after("hello wor", "hello world"),
            "ld"
        );
    }
}
