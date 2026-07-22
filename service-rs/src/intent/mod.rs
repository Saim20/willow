//! Incremental intent matching on streaming ASR partials.

mod engine;

pub use engine::{IntentAction, IntentDecision, IntentEngine, PendingKind};
