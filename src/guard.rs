//! §3.4's data shape: a node guards its own door, and a refusal is a
//! sentence written once.
//!
//! Evaluation is not here. Ask-then-mount — the framework calling a
//! guard before the walk commits to a node — activates when the store
//! lands (`APPLICATION_BUILD.md` WP-2.2). What lands now is the part
//! every game otherwise keeps hand-rolled on its own side of the seam
//! (untold_lore's rooms table, with its `Requires`/`Refusal` columns):
//! the **one-list property** — a guard registers with its node, in the
//! table — and the refusal's form.
//!
//! The form is pinned (decided 2026-08-20, do not revisit): a guard is
//! a **plain Rust function per node**, `fn(&Store) -> Result<(),
//! Refusal>`, called by the framework at ask-time. The table will never
//! grow a predicate DSL. Expressiveness stays in Rust, where the
//! store's fields are; the framework owns only where guards live and
//! how a refusal travels.

/// The structure framework's state store (`APPLICATION.md` §5).
///
/// A placeholder: WP-2.2 lands the real store **in this crate**, which
/// is why guards can name it concretely today with no generic parameter
/// infecting [`RouteTable`](crate::RouteTable) — the type fills in under
/// this name, and every registered guard keeps compiling unchanged.
/// Unconstructible outside the crate on purpose: nothing can call a
/// guard before the framework can, so no host grows an ask-then-mount
/// of its own against a store that holds nothing.
pub struct Store {
    _not_yet: (),
}

#[cfg(test)]
impl Store {
    /// Tests exercise the refusal path before WP-2.2 exists.
    pub(crate) fn placeholder() -> Self {
        Store { _not_yet: () }
    }
}

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
