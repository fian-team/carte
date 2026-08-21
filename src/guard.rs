//! §3.4's data shape: a node guards its own door, and a refusal is a
//! sentence written once.
//!
//! The form is pinned (decided 2026-08-20, do not revisit): a guard is a
//! **plain Rust function per node**, `fn(&Store) -> Result<(), Refusal>`,
//! called by the framework at ask-time. The table will never grow a
//! predicate DSL. Expressiveness stays in Rust, where the store's fields
//! are; the framework owns only where guards live and how a refusal
//! travels.
//!
//! Evaluation lands with the store (WP-2.2) and lives in the walk —
//! [`Router::walk`](crate::Router) asks a node's guard before it descends
//! onto it, so entering really is a request the guard rules on rather than
//! a fait accompli. The same evaluation is reachable ahead of time through
//! [`Router::ask`](crate::Router::ask), which is what a panel row that
//! grays itself calls: one sentence, one evaluation, two surfaces.
//!
//! What is *not* here is a `Store`. The store is [`crate::store`]'s, and
//! this module names it only in the signature above — which is the whole
//! of the coupling between §3.4 and §5, and is deliberately one line.

pub use crate::store::Store;

/// A node's precondition, ruled on at ask-time: entering is a request
/// the guard rules on, never a fait accompli (§3.4).
///
/// A plain function pointer rather than a boxed closure — the pinned
/// form — which also keeps the table `Clone` and registration free of
/// allocation. A guard needing configuration reads it from the store,
/// where the facts it rules over already live.
pub type Guard = fn(&Store) -> Result<(), Refusal>;

/// Why a door stayed shut: a sentence, written once, read everywhere it
/// surfaces — the panel row that grays, the toast that explains (§3.4).
///
/// Machine-readable means **one authored value travels**: no consumer
/// ever composes its own refusal sentence, because a consumer composing
/// one is the drift indicator this type exists to remove.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    sentence: String,
}

impl Refusal {
    /// An owned `String` rather than a `&'static str`, because the best
    /// refusals name the store's facts ("Needs two more players").
    pub fn new(sentence: impl Into<String>) -> Self {
        Self {
            sentence: sentence.into(),
        }
    }

    /// The sentence, verbatim. Every surface shows exactly this.
    pub fn sentence(&self) -> &str {
        &self.sentence
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.sentence)
    }
}
