use std::sync::Arc;

use crate::commands::phrase_index::normalize;
use crate::commands::CommandExecutor;
use crate::config::WillowConfig;
use crate::pipeline::SpeechPipeline;
use crate::types::{CommandDispatchResult, Mode, TranscriptionResult, TtsConfig};
use crate::tts::TtsEngine;

pub struct ModeStateMachine {
    mode: Mode,
    buffer: String,
    typing_last_partial: String,
    typing_committed: String,
    executor: Arc<CommandExecutor>,
    tts: Arc<TtsEngine>,
    tts_config: TtsConfig,
    typing_realtime: bool,
    typing_max_backspace: i32,
    typing_check_recent: i32,
    typing_exit_phrases: Vec<String>,
    on_command_executed: Option<Arc<dyn Fn(String, String, f64) + Send + Sync>>,
}

impl ModeStateMachine {
    pub fn new(executor: Arc<CommandExecutor>, tts: Arc<TtsEngine>) -> Self {
        Self {
            mode: Mode::Normal,
            buffer: String::new(),
            typing_last_partial: String::new(),
            typing_committed: String::new(),
            executor,
            tts,
            tts_config: TtsConfig::default(),
            typing_realtime: true,
            typing_max_backspace: 20,
            typing_check_recent: 100,
            typing_exit_phrases: vec![
                "stop typing".into(),
                "exit typing".into(),
                "normal mode".into(),
                "go to normal mode".into(),
            ],
            on_command_executed: None,
        }
    }

    pub fn set_command_executed_callback<F>(&mut self, f: F)
    where
        F: Fn(String, String, f64) + Send + Sync + 'static,
    {
        self.on_command_executed = Some(Arc::new(f));
    }

    fn notify_command_executed(&self, command: &str, phrase: &str, confidence: f64) {
        if let Some(cb) = &self.on_command_executed {
            cb(command.to_string(), phrase.to_string(), confidence);
        }
    }

    pub fn apply_config(&mut self, config: &WillowConfig) {
        self.tts_config = config.tts_config();
        self.typing_realtime = config.typing_mode.realtime;
        self.typing_max_backspace = config.typing_mode.max_backspace;
        self.typing_check_recent = config.typing_mode.check_recent_chars;
        self.typing_exit_phrases = config.typing_mode.exit_phrases.clone();
        self.tts.update_config(self.tts_config.clone());
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.buffer.clear();
        self.typing_last_partial.clear();
    }

    pub fn apply_mode_to_pipeline(&self, mode: Mode, pipeline: &mut SpeechPipeline) {
        pipeline.set_mode(mode);
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
                let new_mode = Mode::from_str(keyword);
                let old = self.mode;
                if new_mode != old {
                    self.set_mode_with_pipeline(new_mode, pipeline);
                    self.speak_mode(new_mode);
                    return Some(ModeChange { new_mode, old_mode: old });
                }
                None
            }
            _ => {
                if self.mode == Mode::Command {
                    let dispatch = pipeline.resolver.process_keyword(keyword);
                    self.dispatch_command(dispatch, pipeline);
                }
                None
            }
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
                if self.check_exit_phrases(&result.text) {
                    let old = self.mode;
                    self.set_mode_with_pipeline(Mode::Normal, pipeline);
                    self.speak_mode(Mode::Normal);
                    return Some(ModeChange {
                        new_mode: Mode::Normal,
                        old_mode: old,
                    });
                }
                if result.is_final || result.is_endpoint {
                    self.process_typing_final(&result.text);
                    pipeline.asr.reset_stream();
                } else if self.typing_realtime {
                    self.process_typing_partial(&result.text);
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

        if result.is_search {
            if self
                .executor
                .execute_smart_search(&result.search_engine, &result.search_query)
            {
                if self.tts_config.search_executed {
                    self.tts.speak(&format!(
                        "Searching {} for {}",
                        result.search_engine, result.search_query
                    ));
                }
                self.notify_command_executed(
                    &result.search_engine,
                    &result.matched_phrase,
                    result.confidence,
                );
            }
            return;
        }

        if result.is_smart_open {
            if self.executor.execute_smart_open(&result.app_name) {
                if self.tts_config.command_executed {
                    self.tts.speak(&format!("Opening {}", result.app_name));
                }
                self.notify_command_executed(
                    &result.app_name,
                    &result.matched_phrase,
                    result.confidence,
                );
            }
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

        self.executor.execute_command(&result.command_action);
        if self.tts_config.command_executed {
            self.tts.speak("Done");
        }
        self.notify_command_executed(
            &result.command_action,
            &result.matched_phrase,
            result.confidence,
        );
    }

    fn speak_mode(&self, mode: Mode) {
        if self.tts_config.mode_changed {
            self.tts.speak(&format!("{} mode", mode.as_str()));
        }
    }

    fn check_exit_phrases(&self, text: &str) -> bool {
        let norm = normalize(text);
        self.typing_exit_phrases.iter().any(|phrase| {
            let p = normalize(phrase);
            norm.contains(&p)
        })
    }

    fn process_typing_partial(&mut self, partial: &str) {
        self.type_delta(&self.typing_last_partial.clone(), partial);
        self.typing_last_partial = partial.to_string();
    }

    fn process_typing_final(&mut self, final_text: &str) {
        if !final_text.is_empty() {
            self.type_delta(&self.typing_last_partial, final_text);
            self.executor.type_text(" ");
            self.typing_committed.push_str(final_text);
            self.typing_committed.push(' ');
        }
        self.typing_last_partial.clear();
    }

    fn type_delta(&self, old_text: &str, new_text: &str) {
        if new_text == old_text {
            return;
        }
        let mut common = 0usize;
        let min_len = old_text.len().min(new_text.len());
        while common < min_len
            && old_text.as_bytes().get(common) == new_text.as_bytes().get(common)
        {
            common += 1;
        }
        let to_delete = old_text.len().saturating_sub(common);
        if to_delete > 0 {
            let capped = to_delete.min(self.typing_max_backspace as usize);
            for _ in 0..capped {
                self.executor.press_key("14:1 14:0");
            }
        }
        if common < new_text.len() {
            self.executor.type_text(&new_text[common..]);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ModeChange {
    pub new_mode: Mode,
    pub old_mode: Mode,
}
