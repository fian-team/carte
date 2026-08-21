//! The write channel.
//!
//! `update`, `event` and `escape` take `&Cx` and an `&mut Outbox<A>`; only
//! `draw` takes `&mut Cx`. Three things follow, and the first is the
//! reason:
//!
//! - **Axiom 7 becomes a type.** "The session is not a route" stops being
//!   a rule somebody has to remember and becomes something that does not
//!   compile. The bug where opening Pause stopped the client reading the
//!   wire is unrepresentable rather than fixed again.
//! - **The split borrow never appears.** The commonest thing a route does
//!   is read the snapshot and send a command; with `cx.send()` taking
//!   `&mut self` that is two conflicting borrows, and the alternatives
//!   were leaking `Cx`'s field layout into every route or reaching for
//!   interior mutability (which `ROUTING.md` §13.1 forbids).
//! - **Ordering has one owner.** Two drains on one channel, each dropping
//!   what the other wanted, is a bug this shape cannot have: one queue,
//!   drained once per frame, by the host.
//!
//! Not an invention — `app::action_queue` and `shell`'s `ShellAction` are
//! two existing copies of it, and this is their consolidation.

/// Actions a route asks the host to perform, in the order it asked.
///
/// Deliberately not a channel: a route emits during a frame and the host
/// drains at a known point in that same frame, so there is nothing to
/// synchronize and nothing to buffer across frames.
pub struct Outbox<A> {
    actions: Vec<A>,
}

impl<A> Outbox<A> {
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
        }
    }

    /// Ask the host to do something. Order is preserved.
    pub fn push(&mut self, action: A) {
        self.actions.push(action);
    }

    /// Take everything asked for so far, leaving the box empty.
    pub fn drain(&mut self) -> Vec<A> {
        std::mem::take(&mut self.actions)
    }

    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.actions.len()
    }

    /// Read without draining. For tests and for a host that wants to
    /// inspect before it acts.
    pub fn peek(&self) -> &[A] {
        &self.actions
    }
}

impl<A> Default for Outbox<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum Act {
        Save,
        Leave,
    }

    #[test]
    fn actions_arrive_in_the_order_they_were_asked_for() {
        let mut out = Outbox::new();
        out.push(Act::Save);
        out.push(Act::Leave);
        assert_eq!(out.drain(), vec![Act::Save, Act::Leave]);
    }

    #[test]
    fn draining_empties_the_box() {
        let mut out = Outbox::new();
        out.push(Act::Save);
        assert_eq!(out.len(), 1);
        let _ = out.drain();
        assert!(out.is_empty(), "a second drain must not repeat the action");
        assert!(out.drain().is_empty());
    }
}
