use std::time::{Duration, Instant};

use crate::intent::{IntentAction, IntentDecision, PendingKind};

#[derive(Debug, Clone)]
pub struct WorkflowSession {
    pub kind: SessionKind,
    pub prompt: String,
    pub prefix: String,
    pub started_at: Instant,
    pub last_partial: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    OpenApp,
    SearchQuery,
    IncompletePhrase,
}

#[derive(Debug, Clone)]
pub enum WorkflowEvent {
    Prompt(String),
    Cleared,
}

pub struct WorkflowRuntime {
    session: Option<WorkflowSession>,
    timeout: Duration,
}

impl WorkflowRuntime {
    pub fn new() -> Self {
        Self {
            session: None,
            timeout: Duration::from_secs(12),
        }
    }

    pub fn set_timeout_secs(&mut self, secs: f32) {
        self.timeout = Duration::from_secs_f32(secs.clamp(2.0, 120.0));
    }

    pub fn clear(&mut self) {
        self.session = None;
    }

    pub fn session(&self) -> Option<&WorkflowSession> {
        self.session.as_ref()
    }

    pub fn tick_timeout(&mut self) -> bool {
        if self
            .session
            .as_ref()
            .is_some_and(|s| s.started_at.elapsed() > self.timeout)
        {
            self.session = None;
            return true;
        }
        false
    }

    /// Merge an intent decision with any active session; may rewrite the decision.
    /// `from_whisper`: SearchQuery slots only complete from Whisper finals.
    /// `can_commit`: hypothesis is stable or endpoint — required before clearing a session
    /// or accepting a completion (avoids dropping the slot when early-fire is held).
    pub fn integrate(
        &mut self,
        text: &str,
        decision: IntentDecision,
        from_whisper: bool,
        can_commit: bool,
    ) -> (IntentDecision, Option<WorkflowEvent>) {
        if self.tick_timeout() {
            // fall through with fresh decision
        }

        // Cancel clears session.
        if matches!(decision.action, Some(IntentAction::ExitCommandMode)) && decision.fire {
            self.clear();
            return (decision, Some(WorkflowEvent::Cleared));
        }

        // Active session: try to complete from new text.
        if let Some(session) = self.session.clone() {
            // Search multi-turn: wait for Whisper so the engine + query finish accurately.
            if session.kind == SessionKind::SearchQuery && !from_whisper {
                if let Some(s) = self.session.as_mut() {
                    s.last_partial = text.to_string();
                }
                return (
                    IntentDecision {
                        action: Some(IntentAction::Pending {
                            kind: PendingKind::SearchQuery,
                            prompt: session.prompt.clone(),
                        }),
                        fire: false,
                        confidence: decision.confidence,
                        needs_llm: false,
                    },
                    None,
                );
            }

            if let Some(completed) = complete_session(&session, text) {
                if !can_commit {
                    // Keep the slot alive until ASR marks the fill stable / endpoint.
                    if let Some(s) = self.session.as_mut() {
                        s.last_partial = text.to_string();
                    }
                    return (
                        IntentDecision {
                            action: Some(IntentAction::Pending {
                                kind: match session.kind {
                                    SessionKind::OpenApp => PendingKind::OpenApp,
                                    SessionKind::SearchQuery => PendingKind::SearchQuery,
                                    SessionKind::IncompletePhrase => PendingKind::IncompletePhrase,
                                },
                                prompt: session.prompt.clone(),
                            }),
                            fire: false,
                            confidence: decision.confidence,
                            needs_llm: false,
                        },
                        None,
                    );
                }
                self.clear();
                return (
                    IntentDecision {
                        action: Some(completed),
                        fire: true,
                        confidence: 1.0,
                        needs_llm: false,
                    },
                    Some(WorkflowEvent::Cleared),
                );
            }
            // Update last partial / prompt
            if let Some(s) = self.session.as_mut() {
                s.last_partial = text.to_string();
            }
            if decision.fire {
                if !can_commit {
                    return (
                        IntentDecision {
                            action: Some(IntentAction::Pending {
                                kind: match session.kind {
                                    SessionKind::OpenApp => PendingKind::OpenApp,
                                    SessionKind::SearchQuery => PendingKind::SearchQuery,
                                    SessionKind::IncompletePhrase => PendingKind::IncompletePhrase,
                                },
                                prompt: session.prompt.clone(),
                            }),
                            fire: false,
                            confidence: decision.confidence,
                            needs_llm: false,
                        },
                        None,
                    );
                }
                // New complete intent supersedes session.
                self.clear();
                return (decision, Some(WorkflowEvent::Cleared));
            }
            return (
                IntentDecision {
                    action: Some(IntentAction::Pending {
                        kind: match session.kind {
                            SessionKind::OpenApp => PendingKind::OpenApp,
                            SessionKind::SearchQuery => PendingKind::SearchQuery,
                            SessionKind::IncompletePhrase => PendingKind::IncompletePhrase,
                        },
                        prompt: session.prompt.clone(),
                    }),
                    fire: false,
                    confidence: decision.confidence,
                    needs_llm: false,
                },
                None,
            );
        }

        // Start session from pending.
        if let Some(IntentAction::Pending { kind, prompt }) = decision.action.clone() {
            let kind = match kind {
                PendingKind::OpenApp => SessionKind::OpenApp,
                PendingKind::SearchQuery => SessionKind::SearchQuery,
                PendingKind::IncompletePhrase => SessionKind::IncompletePhrase,
            };
            // Refresh an existing same-kind session's partial, or start fresh.
            self.session = Some(WorkflowSession {
                kind,
                prompt: prompt.clone(),
                prefix: text.to_string(),
                started_at: Instant::now(),
                last_partial: text.to_string(),
            });
            return (decision, Some(WorkflowEvent::Prompt(prompt)));
        }

        if decision.fire && can_commit {
            self.clear();
        }
        (decision, None)
    }
}

impl Default for WorkflowRuntime {
    fn default() -> Self {
        Self::new()
    }
}

fn complete_session(session: &WorkflowSession, text: &str) -> Option<IntentAction> {
    let norm = crate::commands::phrase_index::normalize(text);
    if norm.is_empty() {
        return None;
    }
    match session.kind {
        SessionKind::OpenApp => {
            // Bare app name or "open X"
            let app = norm
                .strip_prefix("open ")
                .or_else(|| norm.strip_prefix("launch "))
                .or_else(|| norm.strip_prefix("start "))
                .unwrap_or(&norm)
                .trim();
            let app = app
                .strip_prefix("the ")
                .or_else(|| app.strip_prefix("a "))
                .unwrap_or(app)
                .trim();
            if app.is_empty() || matches!(app, "open" | "launch" | "start") {
                return None;
            }
            Some(IntentAction::SmartOpen {
                app: app.to_string(),
                phrase: format!("open {app}"),
            })
        }
        SessionKind::SearchQuery => {
            // Full "search X for Y" or just the query continuation
            if let Some(rest) = norm.strip_prefix("search ") {
                return Some(IntentAction::SmartSearch {
                    engine: "google".into(),
                    query: rest.to_string(),
                    phrase: norm.clone(),
                });
            }
            let prefix = crate::commands::phrase_index::normalize(&session.prefix);
            if prefix.contains(" for") || prefix.ends_with("for") {
                let engine = prefix
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or("google")
                    .to_string();
                Some(IntentAction::SmartSearch {
                    engine,
                    query: norm,
                    phrase: format!("{} {}", session.prefix, text),
                })
            } else {
                Some(IntentAction::SmartSearch {
                    engine: "google".into(),
                    query: norm,
                    phrase: format!("search google for {text}"),
                })
            }
        }
        SessionKind::IncompletePhrase => {
            // Modes rematches merged text via IntentEngine (known-app / phrase checks).
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent::PendingKind;

    #[test]
    fn open_session_completes_with_app_name() {
        let mut rt = WorkflowRuntime::new();
        let pending = IntentDecision {
            action: Some(IntentAction::Pending {
                kind: PendingKind::OpenApp,
                prompt: "Open which app?".into(),
            }),
            fire: false,
            confidence: 0.5,
            needs_llm: false,
        };
        let (d, ev) = rt.integrate("open", pending, false, true);
        assert!(!d.fire);
        assert!(matches!(ev, Some(WorkflowEvent::Prompt(_))));

        let next = IntentDecision::default();
        // Unstable fill must keep the OpenApp slot.
        let (held, _) = rt.integrate("firefox", next.clone(), false, false);
        assert!(!held.fire);
        assert!(rt.session().is_some());

        let (d2, _) = rt.integrate("firefox", next, false, true);
        assert!(d2.fire);
        assert!(matches!(
            d2.action,
            Some(IntentAction::SmartOpen { app, .. }) if app == "firefox"
        ));
    }

    #[test]
    fn cancel_clears_active_session() {
        let mut rt = WorkflowRuntime::new();
        let pending = IntentDecision {
            action: Some(IntentAction::Pending {
                kind: PendingKind::OpenApp,
                prompt: "Open which app?".into(),
            }),
            fire: false,
            confidence: 0.5,
            needs_llm: false,
        };
        let _ = rt.integrate("open", pending, false, true);
        assert!(rt.session().is_some());

        let cancel = IntentDecision {
            action: Some(IntentAction::ExitCommandMode),
            fire: true,
            confidence: 1.0,
            needs_llm: false,
        };
        let (d, ev) = rt.integrate("cancel", cancel, false, true);
        assert!(d.fire);
        assert!(matches!(ev, Some(WorkflowEvent::Cleared)));
        assert!(rt.session().is_none());
    }

    #[test]
    fn search_session_fills_query_slot() {
        let mut rt = WorkflowRuntime::new();
        let pending = IntentDecision {
            action: Some(IntentAction::Pending {
                kind: PendingKind::SearchQuery,
                prompt: "Search for what?".into(),
            }),
            fire: false,
            confidence: 0.5,
            needs_llm: false,
        };
        let _ = rt.integrate("search youtube for", pending, false, true);
        // Streaming fragment must not complete search.
        let (held, _) = rt.integrate("rust ownership", IntentDecision::default(), false, true);
        assert!(!held.fire);
        assert!(rt.session().is_some());

        let (d, _) = rt.integrate("rust ownership", IntentDecision::default(), true, true);
        assert!(d.fire);
        assert!(matches!(
            d.action,
            Some(IntentAction::SmartSearch { query, .. }) if query.contains("rust")
        ));
    }
}
