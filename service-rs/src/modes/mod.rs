use std::time::{Duration, Instant};

use crate::commands::phrase_index::normalize;
use crate::commands::CommandWorker;
use crate::config::WillowConfig;
use crate::intent::{IntentAction, IntentEngine};
use crate::llm::LlmFallback;
use crate::pipeline::SpeechPipeline;
use crate::types::{Mode, TranscriptionResult};
use crate::workflows::{WorkflowEvent, WorkflowRuntime};

const DEFAULT_SESSION_IDLE: Duration = Duration::from_secs(12);

pub struct ModeStateMachine {
    mode: Mode,
    buffer: String,
    last_activity: Option<Instant>,
    typing_last_typed: String,
    worker: CommandWorker,
    intent: IntentEngine,
    workflows: WorkflowRuntime,
    llm: LlmFallback,
    typing_realtime: bool,
    typing_max_backspace: i32,
    typing_exit_phrases: Vec<String>,
    typing_auto_revert: bool,
    session_idle: Duration,
    llm_fallback: bool,
    /// Prompt to surface on the HUD.
    last_prompt: Option<String>,
    /// Phrases kept for LLM prompting (updated via apply_config).
    known_command_phrases: Vec<String>,
}

impl ModeStateMachine {
    pub fn new(worker: CommandWorker, intent: IntentEngine, llm: LlmFallback) -> Self {
        Self {
            mode: Mode::Normal,
            buffer: String::new(),
            last_activity: None,
            typing_last_typed: String::new(),
            worker,
            intent,
            workflows: WorkflowRuntime::new(),
            llm,
            typing_realtime: false,
            typing_max_backspace: 80,
            typing_exit_phrases: vec![
                "stop typing".into(),
                "exit typing".into(),
                "normal mode".into(),
                "go to normal mode".into(),
            ],
            typing_auto_revert: false,
            session_idle: DEFAULT_SESSION_IDLE,
            llm_fallback: false,
            last_prompt: None,
            known_command_phrases: Vec::new(),
        }
    }

    pub fn apply_config(&mut self, config: &WillowConfig) {
        self.typing_realtime = config.typing_mode.realtime;
        self.typing_max_backspace = config.typing_mode.max_backspace;
        self.typing_exit_phrases = config.typing_mode.exit_phrases.clone();
        self.typing_auto_revert = config.typing_mode.auto_revert;
        self.session_idle =
            Duration::from_secs_f32(config.command_mode.session_idle.clamp(3.0, 120.0));
        self.intent
            .set_threshold(config.command_threshold_fraction());
        self.intent.set_commands(config.commands.clone());
        self.intent.set_early_fire(config.intent.early_fire);
        self.llm_fallback = config.intent.llm_fallback && config.inference.llm.enabled;
        self.llm.apply_config(config.inference.llm.clone());
        self.workflows
            .set_timeout_secs(config.workflows.session_timeout);
        self.known_command_phrases = config
            .commands
            .iter()
            .flat_map(|c| c.phrases.clone())
            .collect();
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn last_prompt(&self) -> Option<&str> {
        self.last_prompt.as_deref()
    }

    fn touch(&mut self) {
        self.last_activity = Some(Instant::now());
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.buffer.clear();
        self.typing_last_typed.clear();
        self.workflows.clear();
        self.last_prompt = None;
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

    pub fn tick_idle(&mut self, pipeline: &mut SpeechPipeline) -> Option<ModeChange> {
        if self.workflows.tick_timeout() {
            self.last_prompt = None;
        }
        let should_idle = match self.mode {
            Mode::Command => true,
            Mode::Typing => self.typing_auto_revert,
            Mode::Normal => false,
        };
        if !should_idle {
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
                let ready = match keyword {
                    "command" => pipeline.ready_for_command(),
                    "typing" => pipeline.ready_for_typing(),
                    _ => true,
                };
                if !ready {
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
    ) -> TranscriptionOutcome {
        if self.mode == Mode::Normal {
            return TranscriptionOutcome::default();
        }

        self.buffer = result.text.clone();
        self.touch();

        match self.mode {
            Mode::Command => self.handle_command_transcript(result, pipeline),
            Mode::Typing => {
                let text = format_for_typing(&result.text);
                if self.check_exit_phrases(&text) {
                    let old = self.mode;
                    self.set_mode_with_pipeline(Mode::Normal, pipeline);
                    return TranscriptionOutcome {
                        mode_change: Some(ModeChange {
                            new_mode: Mode::Normal,
                            old_mode: old,
                        }),
                        ..Default::default()
                    };
                }
                if result.is_final || result.is_endpoint {
                    self.commit_typing_phrase(&text);
                    self.typing_last_typed.clear();
                    pipeline.reset_listening();
                } else if self.typing_realtime {
                    self.apply_typing_delta(&text);
                }
                TranscriptionOutcome::default()
            }
            Mode::Normal => TranscriptionOutcome::default(),
        }
    }

    fn handle_command_transcript(
        &mut self,
        result: &TranscriptionResult,
        pipeline: &mut SpeechPipeline,
    ) -> TranscriptionOutcome {
        // Whisper in Command mode is reserved for search accuracy (VAD-segmented).
        // Other commands use streaming ASR early-fire / endpoints.
        if result.from_whisper && !self.is_search_whisper_candidate(&result.text) {
            return TranscriptionOutcome::default();
        }

        let mut outcome = TranscriptionOutcome::default();
        let mut decision =
            self.intent
                .on_partial(&result.text, result.is_endpoint, result.from_whisper);

        // Incomplete-phrase session: re-match merged text when we get more words.
        if let Some(session) = self.workflows.session() {
            if session.kind == crate::workflows::SessionKind::IncompletePhrase {
                let merged = if normalize(&result.text).starts_with(&session.prefix) {
                    normalize(&result.text)
                } else {
                    format!("{} {}", session.prefix, normalize(&result.text))
                };
                let rematch =
                    self.intent
                        .on_partial(&merged, result.is_endpoint, result.from_whisper);
                if rematch.fire || rematch.action.is_some() {
                    decision = rematch;
                }
            }
        }

        // Hold early-fire until the stream marks the hypothesis stable (or endpoint).
        // Must happen before workflow integrate so sessions aren't cleared on a speculative fire.
        let can_commit = result.is_endpoint || result.is_stable;
        if decision.fire && !can_commit {
            decision.fire = false;
            if outcome.pending_phrase.is_none() {
                if let Some(IntentAction::Pending { prompt, .. }) = &decision.action {
                    outcome.pending_phrase = Some(prompt.clone());
                } else if !result.text.is_empty() {
                    outcome.pending_phrase = Some(result.text.clone());
                }
            }
        }

        let (decision, wf_event) =
            self.workflows
                .integrate(&result.text, decision, result.from_whisper, can_commit);
        match wf_event {
            Some(WorkflowEvent::Prompt(p)) => {
                self.last_prompt = Some(p.clone());
                outcome.prompt = Some(p);
            }
            Some(WorkflowEvent::Cleared) => {
                self.last_prompt = None;
            }
            None => {}
        }

        if decision.fire {
            if let Some(action) = decision.action {
                if let Some(change) = self.dispatch_intent(action, pipeline) {
                    outcome.mode_change = Some(change);
                }
                pipeline.reset_listening();
            }
            return outcome;
        }

        if decision.needs_llm && result.is_endpoint && self.llm_fallback {
            let known = self.known_phrases();
            let llm_decision = self.llm.resolve(&result.text, &known);
            if llm_decision.fire {
                if let Some(action) = llm_decision.action {
                    if let Some(change) = self.dispatch_intent(action, pipeline) {
                        outcome.mode_change = Some(change);
                    }
                    pipeline.reset_listening();
                }
            }
        }

        if !result.is_endpoint {
            if let Some(IntentAction::Pending { prompt, .. }) = &decision.action {
                outcome.pending_phrase = Some(prompt.clone());
            }
        }

        outcome
    }

    fn is_search_whisper_candidate(&self, text: &str) -> bool {
        let norm = normalize(text);
        if norm.starts_with("search") {
            return true;
        }
        self.workflows
            .session()
            .is_some_and(|s| s.kind == crate::workflows::SessionKind::SearchQuery)
    }

    fn known_phrases(&self) -> Vec<String> {
        self.known_command_phrases.clone()
    }

    fn dispatch_intent(
        &mut self,
        action: IntentAction,
        pipeline: &mut SpeechPipeline,
    ) -> Option<ModeChange> {
        match action {
            IntentAction::ExitCommandMode => {
                let old = self.mode;
                self.set_mode_with_pipeline(Mode::Normal, pipeline);
                Some(ModeChange {
                    new_mode: Mode::Normal,
                    old_mode: old,
                })
            }
            IntentAction::StartTypingMode => {
                let old = self.mode;
                self.set_mode_with_pipeline(Mode::Typing, pipeline);
                Some(ModeChange {
                    new_mode: Mode::Typing,
                    old_mode: old,
                })
            }
            IntentAction::RunCommand {
                name,
                action,
                phrase,
                confidence,
            } => {
                tracing::info!("Running command '{name}' for phrase {phrase:?}");
                self.worker.execute(action, phrase, confidence);
                None
            }
            IntentAction::SmartOpen { app, phrase } => {
                self.worker.smart_open(app, phrase, 1.0);
                None
            }
            IntentAction::SmartSearch {
                engine,
                query,
                phrase,
            } => {
                self.worker.smart_search(engine, query, phrase, 1.0);
                None
            }
            IntentAction::Pending { .. } => None,
        }
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

#[derive(Debug, Default)]
pub struct TranscriptionOutcome {
    pub mode_change: Option<ModeChange>,
    pub prompt: Option<String>,
    pub pending_phrase: Option<String>,
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
    fn commit_suffix_after_partial() {
        assert_eq!(typing_suffix_after("hello wor", "hello world"), "ld");
    }
}
