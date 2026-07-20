use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use super::CommandExecutor;

type ExecutedCallback = Arc<dyn Fn(String, String, f64) + Send + Sync>;

enum CommandJob {
    Execute {
        action: String,
        phrase: String,
        confidence: f64,
    },
    SmartOpen {
        app: String,
        phrase: String,
        confidence: f64,
    },
    SmartSearch {
        engine: String,
        query: String,
        phrase: String,
        confidence: f64,
    },
    TypeText(String),
    PressKey(String),
}

pub struct CommandWorker {
    sender: mpsc::Sender<CommandJob>,
}

impl CommandWorker {
    pub fn new(executor: Arc<CommandExecutor>, on_executed: ExecutedCallback) -> Self {
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
                        } => {
                            if executor.execute_command(&action) {
                                on_executed(action, phrase, confidence);
                            }
                        }
                        CommandJob::SmartOpen {
                            app,
                            phrase,
                            confidence,
                        } => {
                            if executor.execute_smart_open(&app) {
                                on_executed(app.clone(), phrase, confidence);
                            }
                        }
                        CommandJob::SmartSearch {
                            engine,
                            query,
                            phrase,
                            confidence,
                        } => {
                            if executor.execute_smart_search(&engine, &query) {
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

    pub fn execute(&self, action: String, phrase: String, confidence: f64) {
        let _ = self.sender.send(CommandJob::Execute {
            action,
            phrase,
            confidence,
        });
    }

    pub fn smart_open(&self, app: String, phrase: String, confidence: f64) {
        let _ = self.sender.send(CommandJob::SmartOpen {
            app,
            phrase,
            confidence,
        });
    }

    pub fn smart_search(
        &self,
        engine: String,
        query: String,
        phrase: String,
        confidence: f64,
    ) {
        let _ = self.sender.send(CommandJob::SmartSearch {
            engine,
            query,
            phrase,
            confidence,
        });
    }

    pub fn type_text(&self, text: &str) {
        let _ = self.sender.send(CommandJob::TypeText(text.to_string()));
    }

    pub fn press_key(&self, key: &str) {
        let _ = self.sender.send(CommandJob::PressKey(key.to_string()));
    }
}
