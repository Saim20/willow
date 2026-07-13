use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use crate::tts::TtsEngine;

use super::CommandExecutor;

type ExecutedCallback = Arc<dyn Fn(String, String, f64) + Send + Sync>;

enum CommandJob {
    Execute {
        action: String,
        phrase: String,
        confidence: f64,
        speak_done: bool,
        speak_errors: bool,
    },
    SmartOpen {
        app: String,
        phrase: String,
        confidence: f64,
        speak_success: bool,
        speak_errors: bool,
    },
    SmartSearch {
        engine: String,
        query: String,
        phrase: String,
        confidence: f64,
        speak_success: bool,
    },
    TypeText(String),
    PressKey(String),
}

pub struct CommandWorker {
    sender: mpsc::Sender<CommandJob>,
}

impl CommandWorker {
    pub fn new(executor: Arc<CommandExecutor>, tts: Arc<TtsEngine>, on_executed: ExecutedCallback) -> Self {
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("willow-commands".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    match job {
                        CommandJob::Execute {
                            action,
                            phrase,
                            confidence,
                            speak_done,
                            speak_errors,
                        } => {
                            let ok = executor.execute_command(&action);
                            if ok {
                                if speak_done {
                                    tts.speak("Done");
                                }
                                on_executed(action, phrase, confidence);
                            } else if speak_errors {
                                tts.speak("Sorry, that command failed");
                            }
                        }
                        CommandJob::SmartOpen {
                            app,
                            phrase,
                            confidence,
                            speak_success,
                            speak_errors,
                        } => {
                            if executor.execute_smart_open(&app) {
                                if speak_success {
                                    tts.speak(&format!("Opening {app}"));
                                }
                                on_executed(app.clone(), phrase, confidence);
                            } else if speak_errors {
                                tts.speak(&format!("Could not find {app}"));
                            }
                        }
                        CommandJob::SmartSearch {
                            engine,
                            query,
                            phrase,
                            confidence,
                            speak_success,
                        } => {
                            if executor.execute_smart_search(&engine, &query) {
                                if speak_success {
                                    tts.speak(&format!("Searching {engine} for {query}"));
                                }
                                on_executed(engine, phrase, confidence);
                            }
                        }
                        CommandJob::TypeText(text) => executor.type_text(&text),
                        CommandJob::PressKey(key) => executor.press_key(&key),
                    }
                }
            })
            .expect("spawn command worker");
        Self { sender: tx }
    }

    pub fn execute(
        &self,
        action: String,
        phrase: String,
        confidence: f64,
        speak_done: bool,
        speak_errors: bool,
    ) {
        let _ = self.sender.send(CommandJob::Execute {
            action,
            phrase,
            confidence,
            speak_done,
            speak_errors,
        });
    }

    pub fn smart_open(
        &self,
        app: String,
        phrase: String,
        confidence: f64,
        speak_success: bool,
        speak_errors: bool,
    ) {
        let _ = self.sender.send(CommandJob::SmartOpen {
            app,
            phrase,
            confidence,
            speak_success,
            speak_errors,
        });
    }

    pub fn smart_search(
        &self,
        engine: String,
        query: String,
        phrase: String,
        confidence: f64,
        speak_success: bool,
    ) {
        let _ = self.sender.send(CommandJob::SmartSearch {
            engine,
            query,
            phrase,
            confidence,
            speak_success,
        });
    }

    pub fn type_text(&self, text: &str) {
        let _ = self.sender.send(CommandJob::TypeText(text.to_string()));
    }

    pub fn press_key(&self, key: &str) {
        let _ = self.sender.send(CommandJob::PressKey(key.to_string()));
    }
}
