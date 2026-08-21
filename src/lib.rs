//! The structure framework's routing half: the table (tiers, areas,
//! occlusion, guards), the walk, depth arbitration, escape, departure,
//! and the outbox.
//!
//! Extracted from `ogham/src/route` per `APPLICATION.md` §2, whose
//! engineering guarantee this crate carries: **the structure framework
//! depends on nothing of the surface framework**, and the empty
//! `[dependencies]` section is the proof — `cargo tree -p structure`
//! shows nothing at all.
//!
//! What did not move stays behind in `ogham::route` as scaffolding,
//! because its signatures name surface types: the `Route` trait
//! (`read_state`'s values, `own_ui`'s instance), `RouteEvent` (a widget
//! event rides in it), `Chrome`, and the router's `event`/`draw`
//! dispatch. Per `APPLICATION_BUILD.md`, those dissolve into the store
//! (P2) and the driver (P4); the [`Node`] seam below is the bridge that
//! lets the walk live here in the meantime.
//!
//! The store's half begins in [`schema`]: §4.3's derived reflection, the
//! vocabulary a scope's schema struct describes itself in and the thing a
//! document's selection validates against. It is here rather than in the
//! surface framework for the same reason the table is — a schema is a fact
//! about what exists, and the document that selects from one is a consumer.
//!
//! `lorekeeper/docs/ROUTING.md` remains the record of the routing axioms
//! cited throughout; `APPLICATION.md` is the record of the split.

pub mod guard;
pub mod outbox;
pub mod router;
pub mod schema;
pub mod table;

pub use guard::{Guard, Refusal, Store};
pub use outbox::Outbox;
pub use router::{EscapeOutcome, Node, Router};
pub use schema::{
    reflect_of, Difference, Field, Grain, Initial, Kind, Lit, Mismatch, Presence, Schema, Variant,
};
pub use table::{RouteTable, TableError, Tier};

/// A route's id: the name a table registers and a `screen` block declares.
///
/// A `&'static str` rather than a `String` because the table is static and
/// built once at startup (axiom 10) — and because comparing ids is on the
/// per-frame path, where the walk asks every node on the path for its
/// child.
pub type RouteId = &'static str;

/// The ground an instance root stands on — which music plays, which
/// backdrop stands (`APPLICATION.md` §3.2).
///
/// Game vocabulary, so a name rather than an enum: untold_lore's four
/// roots share three areas, and the table cannot know what a game calls
/// its grounds. `&'static str` for the same reasons as [`RouteId`].
pub type Area = &'static str;

/// One argument of a named raise.
///
/// Not the surface runtime's `Value`, and the reason is a real constraint
/// rather than taste: an event handler must be `Send + Sync`, and a
/// `Value` may hold an `Rc<VMClosure>`, so it cannot cross the handler
/// boundary at all. What can cross is data — which is also the only thing
/// a host has any business acting on. (The conversion *from* the runtime's
/// `Value` lives on the surface side of the seam, where the type does.)
#[derive(Clone, Debug, PartialEq)]
pub enum RaiseArg {
    Str(String),
    Int(i32),
    Float(f64),
    Bool(bool),
    /// A closure, a widget, or void — nothing a host can act on.
    Opaque,
}

impl RaiseArg {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            RaiseArg::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            RaiseArg::Float(f) => Some(*f as f32),
            RaiseArg::Int(i) => Some(*i as f32),
            _ => None,
        }
    }
}

/// Whether a route consumed an event.
///
/// Depth arbitration offers an event to the deepest active route first and
/// stops at the first `Handled::Yes` (axiom 6). One rule, replacing five
/// hand-ordered callbacks in one game and a two-layer special case in
/// `app`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

impl Handled {
    pub fn is_handled(self) -> bool {
        matches!(self, Handled::Yes)
    }

    /// `true` → `Yes`. For routes whose inner call already returns a bool.
    pub fn from_bool(consumed: bool) -> Self {
        match consumed {
            true => Handled::Yes,
            false => Handled::No,
        }
    }
}

/// What a route hides beneath it.
///
/// Ordered, and `Surface` implies `View`. That it is one ladder rather than
/// two independent bits is an *empirical* claim about the three consumers,
/// not a simplification: nothing in any of them wants an ancestor's 3D
/// hidden while its HUD still draws. `ROUTING.md` §7.2 is the record, and
/// this is the line to attack first if that turns out wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Occlusion {
    /// Ancestors draw and their screens render behind this one. What the
    /// exit prompts want and cannot express today: a scrim and a card
    /// authored to sit *over* the workspace it is leaving.
    None,
    /// Ancestors' `draw` runs; their screens do not. Every ordinary child
    /// — the journal, the wardrobe, the sea and sky stances, pause — and
    /// the default, because it is what a `mode` swap does today.
    #[default]
    View,
    /// Neither. A route that brings its own world: the map editor, the
    /// backstory workspace, the library.
    Surface,
}

/// What Escape means here (axiom 9).
///
/// The deepest active route decides, and no layer above it intercepts.
/// Today the shell claims Escape unconditionally while `InGame`, which is
/// why two games' Escape handlers are dead code that reads as correct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escape {
    /// Leave this route; the path shortens by one. The ordinary case.
    Pop,
    /// This route has unsaved work and raises its own prompt instead. The
    /// prompt is a child route; the router does not pop.
    Prompt,
    /// Escape means nothing here and must not reach anyone else — a text
    /// field clearing itself, a stance that owns the key.
    Ignore,
    /// Not this route's key. Offer it to the parent.
    Fall,
}

/// How the outgoing route leaves the frame.
///
/// Two of the three consumers already implement cross-screen transitions
/// by re-rendering the *outgoing* surface offscreen and bleeding it over
/// the incoming one. A router that swaps the top of the stack and drops
/// the old route breaks both, so departure is part of the contract rather
/// than something a game bolts on beside it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Departure {
    /// Gone this frame. The default, and it costs nothing.
    #[default]
    Cut,
    /// Hold the outgoing route's last frame for `seconds`, composited
    /// under the incoming one. The router keeps the frame; what is done
    /// with it is the host's (a page turn, a cross-fade, a bloom).
    Bleed { seconds: f32 },
}
