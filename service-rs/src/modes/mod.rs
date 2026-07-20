use std::time::{Duration, Instant};

use crate::commands::phrase_index::normalize;
use crate::commands::CommandWorker;
use crate::config::WillowConfig;
use crate::pipeline::SpeechPipeline;
use crate::types::{CommandDispatchResult, Mode, TranscriptionResult};

/// How long to wait for the rest of an incomplete command like "open …".
const DEFAULT_INCOMPLETE_HOLD: Duration = Duration::from_millis(1500);
/// Return to Normal after this much silence with no speech/commands.
const DEFAULT_SESSION_IDLE: Duration = Duration::from_secs(12);

pub struct ModeStateMachine {
    mode: Mode,
    buffer: String,
    /// Prefix held when ASR endpointed mid-command (e.g. just "open").
    incomplete_prefix: Option<String>,
    incomplete_at: Option<Instant>,
    last_activity: Option<Instant>,
    typing_last_typed: String,
    worker: CommandWorker,
    typing_realtime: bool,
    typing_max_backspace: i32,
    typing_exit_phrases: Vec<String>,
    incomplete_hold: Duration,
    session_idle: Duration,
}

impl ModeStateMachine {
    pub fn new(worker: CommandWorker) -> Self {
        Self {
            mode: Mode::Normal,
            buffer: String::new(),
            incomplete_prefix: None,
            incomplete_at: None,
            last_activity: None,
            typing_last_typed: String::new(),
            worker,
            typing_realtime: false,
            typing_max_backspace: 80,
            typing_exit_phrases: vec![
                "stop typing".into(),
                "exit typing".into(),
                "normal mode".into(),
                "go to normal mode".into(),
            ],
            incomplete_hold: DEFAULT_INCOMPLETE_HOLD,
            session_idle: DEFAULT_SESSION_IDLE,
        }
    }

    pub fn apply_config(&mut self, config: &WillowConfig) {
        self.typing_realtime = config.typing_mode.realtime;
        self.typing_max_backspace = config.typing_mode.max_backspace;
        self.typing_exit_phrases = config.typing_mode.exit_phrases.clone();
        self.incomplete_hold = Duration::from_secs_f32(
            config.command_mode.incomplete_hold.clamp(0.3, 8.0),
        );
        self.session_idle =
            Duration::from_secs_f32(config.command_mode.session_idle.clamp(3.0, 120.0));
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    fn touch(&mut self) {
        self.last_activity = Some(Instant::now());
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.buffer.clear();
        self.typing_last_typed.clear();
        self.clear_incomplete();
        if matches!(mode, Mode::Command | Mode::Typing) {
            self.touch();
        } else {
            self.last_activity = None;
        }
    }

    pub fn set_mode_with_pipeline(&mut self, mode: Mode, pipeline: &mut SpeechPipeline) {
        self.set_mode(mode);
        pipeline.set_mode(mode);
    }

    /// Exit Command/Typing after prolonged inactivity.
    pub fn tick_idle(&mut self, pipeline: &mut SpeechPipeline) -> Option<ModeChange> {
        if !matches!(self.mode, Mode::Command | Mode::Typing) {
            return None;
        }
        let idle = self
            .last_activity
            .is_some_and(|t| t.elapsed() > self.session_idle);
        if !idle {
            return None;
        }
        let old = self.mode;
        self.set_mode_with_pipeline(Mode::Normal, pipeline);
        Some(ModeChange {
            new_mode: Mode::Normal,
            old_mode: old,
        })
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
                if matches!(keyword, "command" | "typing") && !pipeline.ready_for_command() {
                    return None;
                }
                let new_mode = Mode::from_str(keyword);
                let old = self.mode;
                if new_mode != old {
                    self.set_mode_with_pipeline(new_mode, pipeline);
                    return Some(ModeChange {
                        new_mode,
                        old_mode: old,
                    });
                }
                self.touch();
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
        self.touch();

        match self.mode {
            Mode::Command => {
                if result.is_endpoint {
                    let text = self.merge_with_incomplete(&result.text);
                    if text.is_empty() {
                        pipeline.reset_listening();
                        return None;
                    }
                    // Hold bare prefixes before resolving — process_endpoint records history.
                    if is_incomplete_command(&text) {
                        self.incomplete_prefix = Some(text);
                        self.incomplete_at = Some(Instant::now());
                        pipeline.reset_listening();
                        return None;
                    }
                    let dispatch = pipeline.resolver.process_endpoint(&text);
                    // Hold 1–2 word scraps that didn't match so the next chunk can merge.
                    if should_hold_short_fragment(&text, dispatch.handled) {
                        self.incomplete_prefix = Some(text);
                        self.incomplete_at = Some(Instant::now());
                        pipeline.reset_listening();
                        return None;
                    }
                    self.clear_incomplete();
                    self.dispatch_command(dispatch, pipeline);
                    pipeline.reset_listening();
                }
                None
            }
            Mode::Typing => {
                let text = format_for_typing(&result.text);
                if self.check_exit_phrases(&text) {
                    let old = self.mode;
                    self.set_mode_with_pipeline(Mode::Normal, pipeline);
                    return Some(ModeChange {
                        new_mode: Mode::Normal,
                        old_mode: old,
                    });
                }
                if result.is_final || result.is_endpoint {
                    self.commit_typing_phrase(&text);
                    self.typing_last_typed.clear();
                    pipeline.reset_listening();
                } else if self.typing_realtime {
                    self.apply_typing_delta(&text);
                }
                None
            }
            Mode::Normal => None,
        }
    }

    fn merge_with_incomplete(&mut self, text: &str) -> String {
        let expired = self
            .incomplete_at
            .is_some_and(|t| t.elapsed() > self.incomplete_hold);
        if expired {
            self.clear_incomplete();
        }
        let norm = normalize(text);
        match self.incomplete_prefix.take() {
            Some(prefix) if !norm.is_empty() => {
                self.incomplete_at = None;
                if norm.starts_with(&prefix) {
                    norm
                } else {
                    format!("{prefix} {norm}")
                }
            }
            Some(prefix) => {
                self.incomplete_prefix = Some(prefix.clone());
                prefix
            }
            None => norm,
        }
    }

    fn clear_incomplete(&mut self) {
        self.incomplete_prefix = None;
        self.incomplete_at = None;
    }

    fn dispatch_command(&mut self, result: CommandDispatchResult, pipeline: &mut SpeechPipeline) {
        if !result.handled {
            return;
        }

        if result.command_action.is_empty() && !result.is_search && !result.is_smart_open {
            return;
        }

        if result.is_search {
            self.worker.smart_search(
                result.search_engine,
                result.search_query,
                result.matched_phrase,
                result.confidence,
            );
            return;
        }

        if result.is_smart_open {
            self.worker.smart_open(
                result.app_name,
                result.matched_phrase,
                result.confidence,
            );
            return;
        }

        if result.command_action == "exit_command_mode" {
            self.set_mode_with_pipeline(Mode::Normal, pipeline);
            return;
        }
        if result.command_action == "start_typing_mode" {
            self.set_mode_with_pipeline(Mode::Typing, pipeline);
            return;
        }

        self.worker.execute(
            result.command_action,
            result.matched_phrase,
            result.confidence,
        );
    }

    fn check_exit_phrases(&self, text: &str) -> bool {
        self.typing_exit_phrases.iter().any(|phrase| {
            let p = normalize(phrase);
            text.contains(&p)
        })
    }

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

fn format_for_typing(text: &str) -> String {
    normalize(text)
}

fn is_incomplete_command(text: &str) -> bool {
    let norm = normalize(text);
    if norm.is_empty() {
        return false;
    }
    matches!(norm.as_str(), "open" | "launch" | "start" | "search")
        || norm.ends_with(" for")
        || ["open", "launch", "start", "search"]
            .iter()
            .any(|t| norm == format!("{t} the") || norm == format!("{t} a"))
}

/// Hold 1–2 word scraps that didn't match a command so the next VAD chunk can merge.
fn should_hold_short_fragment(text: &str, handled: bool) -> bool {
    if handled {
        return false;
    }
    let norm = normalize(text);
    let words = norm.split_whitespace().count();
    matches!(words, 1 | 2)
}

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
        assert_eq!(typing_suffix_after("hello wor", "hello world"), "ld");
    }

    #[test]
    fn incomplete_open_detected() {
        assert!(is_incomplete_command("open"));
        assert!(is_incomplete_command("Open"));
        assert!(is_incomplete_command("launch"));
        assert!(is_incomplete_command("search google for"));
        assert!(!is_incomplete_command("open firefox"));
        assert!(!is_incomplete_command("start typing"));
        assert!(!is_incomplete_command("exit"));
    }
}
