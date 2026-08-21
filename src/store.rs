//! The store (`APPLICATION.md` §5): scoped facts about what should be
//! presented, who is told when they change, and the frame barrier that
//! makes "when" mean one moment.
//!
//! # What a scope is
//!
//! A scope is a Rust struct — the provider's schema type (§4.1) — keyed by
//! the [`Scope`] that owns it. There are three rungs, and they are the
//! three lifetimes a fact can have:
//!
//! - [`Scope::Process`], the app-global rung: mounted when it is provided
//!   and never dropped (untold_lore's `launch_fade`/`heading`/`status`).
//! - [`Scope::Node`] over a **structural** node: the fact outlives every
//!   child (celia's session holds the focused character precisely because
//!   it must outlive both the lobby and the arena).
//! - [`Scope::Node`] over an instance root or a view: the fact dies with
//!   the screen.
//!
//! The rule for the last two is one sentence: **scope lifetime is node
//! presence on the path.** Nothing else mounts a scope, which is why
//! [`mount`](Store::mount) and [`unmount`](Store::unmount) are not public —
//! the [`Router`](crate::Router)'s walk is the only caller, and a host that
//! could mount by hand would be a host that could keep a scope alive past
//! its node. That is what makes half of regency's `teardown` unwritable
//! rather than merely unnecessary.
//!
//! # Two verbs (§5.1)
//!
//! **Read** is pull, bulk, per-frame: [`read`](Store::read) hands back
//! `&T` and [`version`](Store::version) is the cheap "has this moved?"
//! check beside it. A typed consumer pays a downcast and then *direct
//! field reads* — there is no map on the read path at all, which is §5.1's
//! "protocol means contract, not serialization" written as an API.
//!
//! **Subscribe** is push, sparse, per-field: a [`SubscriberId`] registers
//! against a scope's field and appears in the [`Notice`] of every tick that
//! moved it. The store does not call anyone back. A callback would run
//! *inside* the commit, which is precisely the moment at which a consumer
//! must not be able to look at anything (§5.4), and would need the store
//! borrowed mutably while it re-entered. So the tick hands back the list of
//! who to wake and the binding does the waking, once, after commit.
//!
//! # The pinned order (§5.4), and why it is not a comment
//!
//! ```text
//! outbox drain → producer sets → barrier → notify → rerender
//! ```
//!
//! [`Store::tick`] *is* that order, and the type system holds it:
//!
//! - The drain happens inside `tick`, before the closure runs, and the
//!   closure receives the drained actions. A producer cannot run first,
//!   because it has not been called yet; an intent raised this tick is
//!   therefore applied this tick, and a click's echo costs zero frames.
//! - A set is only reachable through a [`Writer`], a `Writer` only through
//!   the [`Barrier`], and the `Barrier` only inside that closure. There is
//!   no way to write to the store outside the barrier.
//! - The `Barrier` borrows the store mutably for its whole life, so no
//!   consumer can [`read`](Store::read) while producers are running. The
//!   classic FRP glitch — field A new, field B old — is not guarded
//!   against, it is unreachable: working state has exactly one reader, the
//!   producers, and they are told to expect it.
//! - Commit is what `tick` does on the way out, and the [`Notice`] is its
//!   only return. Ten sets are one notice by construction, because there is
//!   one commit.
//!
//! # Equality, and what it replaces (§5.3)
//!
//! Every set is equality-checked against the value already there and a
//! no-op is swallowed. This is `Chrome::project_fields`' per-key compare —
//! "only changed keys reach the runtime" — generalised, minus its
//! `"{id}::{name}"` string keys and its map. It is also lorekeeper
//! INTENT §22 ("dirtiness is derived from values, never remembered by
//! flags") implemented once, in the place every hand-rolled diff idiom was
//! re-implementing it.
//!
//! # The declaration *is* the at-mount value (§4.1)
//!
//! [`provides`](Store::provides) takes a type and no value. A scope's first
//! value is assembled by [`Schema::at_mount`](crate::Schema::at_mount) from
//! the very declarations the reflection publishes, so "the schema says this
//! field mounts at 1.0" and "the scope mounted at 1.0" are one fact rather
//! than two that agree by convention. untold_lore's `launch_fade` — a
//! chrome that drew perfectly and showed nothing — is the lesson, and the
//! form this takes is the reason it cannot recur: there is no second value
//! for the declaration to disagree with.
//!
//! Single writer per field is enforced by claim: a producer names the
//! fields it sets ([`Store::producer`]) and two producers naming one field
//! is a **startup** error, not a race nobody notices. Setting a field
//! nobody claimed, or one another producer claimed, is refused at the set.
//!
//! # The fence (§5.2)
//!
//! The store carries facts about what should be presented and never the
//! presentation's bulk data. Nothing here can stop a provider declaring
//! `Vec<Vertex>`, but three things make it uncomfortable, on purpose: a
//! scope is cloned on every commit that touched it, so bulk data is paid
//! for per frame; the read verb hands out `&T` and nothing else, so a
//! renderer that wants geometry has to go and get it from the world; and
//! [`Kind::field_at`](crate::Kind::field_at) refuses to descend into a
//! collection, so bulk data cannot even be *selected* into. The fence is
//! still a fence — this makes leaning on it hurt.

use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

use crate::intent::{Intents, Raise, Refused, Vocabulary};
use crate::schema::{Field, Grain, Kind, Schema};
use crate::{Outbox, RouteId};

// --- keys and errors -------------------------------------------------------

/// Which scope a fact belongs to — the three-rung ladder of §5's scope
/// lifetimes, as a key.
///
/// A node's id rather than a node object, because everything the store
/// needs to know about a scope's lifetime is answerable from the table and
/// the path ([`RouteTable::tier_of`](crate::RouteTable::tier_of) and
/// friends). Structural nodes and views key the same way: the tier decides
/// how long the node is on the path, and the store only ever asks whether
/// it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    /// The app-global rung: provided once, mounted from that moment, never
    /// dropped. The measured remainder that does not partition per node —
    /// a launch fade, the window heading.
    Process,
    /// The scope a route node owns, alive exactly while that node is on
    /// the path.
    Node(RouteId),
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Process => f.write_str("the process scope"),
            Scope::Node(id) => write!(f, "`{id}`'s scope"),
        }
    }
}

/// What the store refused, and why.
///
/// Every variant names the scope, and every field variant names the field,
/// for the same reason §4.1 asks a selection refusal to: the failures this
/// design exists to move earlier are the ones that otherwise surface as a
/// screen that is quietly wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// A scope was provided for an id the route table does not register.
    /// The typo protection [`RouteTable::under`](crate::RouteTable::under)
    /// has, applied to scopes: a scope on a misspelt id would simply never
    /// mount, and the empty screen would be the diagnostic.
    UnknownNode(RouteId),
    /// Two providers for one scope. One scope, one schema.
    AlreadyProvided(Scope),
    /// Two intent vocabularies for one scope. One scope, one write side.
    AlreadyAccepting(Scope),
    /// One vocabulary publishes two intents under one name — two
    /// `#[serde(rename)]`s colliding where two Rust variants could not.
    /// One of the two would be permanently unreachable.
    DuplicateIntent { scope: Scope, intent: String },
    /// Nobody provides this scope, so there is nothing to read, claim or
    /// subscribe to.
    NotProvided(Scope),
    /// A schema type whose reflection is not a record. A scope is a set of
    /// named fields — that is what per-field selection, per-field claims
    /// and per-field notification all address.
    NotARecord { scope: Scope, kind: &'static str },
    /// The scope's node is not on the path, so the scope does not exist
    /// right now. §5's out-of-scope set.
    NotMounted(Scope),
    /// The schema declares no such field.
    NoSuchField { scope: Scope, field: String },
    /// A second producer claimed a field another one already writes.
    /// Single-writer, caught at startup (§5.3).
    AlreadyClaimed { scope: Scope, field: String },
    /// A producer set a field it did not claim.
    Unclaimed { scope: Scope, field: String },
    /// The type asked for is not the type the scope was provided with.
    WrongType {
        scope: Scope,
        want: &'static str,
        got: &'static str,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::UnknownNode(id) => write!(
                f,
                "`{id}` provides a scope but is not a registered route; a scope keyed on a \
                 misspelt id would never mount, and the empty screen would be the diagnostic"
            ),
            StoreError::AlreadyProvided(scope) => {
                write!(f, "{scope} is already provided; one scope, one schema")
            }
            StoreError::AlreadyAccepting(scope) => write!(
                f,
                "{scope} already publishes the intents it accepts; one scope, one write side"
            ),
            StoreError::DuplicateIntent { scope, intent } => write!(
                f,
                "{scope} publishes two intents named `{intent}`; one of them could never be \
                 raised"
            ),
            StoreError::NotProvided(scope) => write!(f, "nothing provides {scope}"),
            StoreError::NotARecord { scope, kind } => write!(
                f,
                "{scope}'s schema reflects as {kind}, not a record; a scope is a set of named \
                 fields, which is what selection, claims and notification all address"
            ),
            StoreError::NotMounted(scope) => write!(
                f,
                "{scope} is not on the path, so it does not exist to be written"
            ),
            StoreError::NoSuchField { scope, field } => {
                write!(f, "{scope} declares no field `{field}`")
            }
            StoreError::AlreadyClaimed { scope, field } => write!(
                f,
                "{scope}'s field `{field}` is already claimed by another producer; the store is \
                 single-writer per field"
            ),
            StoreError::Unclaimed { scope, field } => write!(
                f,
                "this producer did not claim {scope}'s field `{field}`, so it may not set it"
            ),
            StoreError::WrongType { scope, want, got } => {
                write!(f, "{scope} is provided as `{got}`, not `{want}`")
            }
        }
    }
}

impl std::error::Error for StoreError {}

/// Why a raise did not become an action on the outbox (§4.4).
///
/// It names the scope for the same reason every [`StoreError`] does, and
/// carries the [`Refused`] verbatim: a raise refused for the wrong reason is
/// a button that quietly reaches nobody, which is the failure the whole of
/// §4.4 exists to move to load time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaiseError {
    scope: Scope,
    at: Refused,
}

impl RaiseError {
    /// The scope the raise was offered to.
    pub fn scope(&self) -> Scope {
        self.scope
    }

    /// What the vocabulary said, for a consumer that wants to act rather
    /// than print.
    pub fn refusal(&self) -> &Refused {
        &self.at
    }
}

impl fmt::Display for RaiseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.scope, self.at)
    }
}

impl std::error::Error for RaiseError {}

// --- what a scope's type must be -------------------------------------------

/// What a scope's schema type has to be: a [`Schema`] (so a document can
/// validate a selection against its reflection, §4.3), [`Clone`] (so a
/// commit can publish working state and a mount can start from the declared
/// at-mount value), and `'static` (so the store can hold it type-erased and
/// hand it back typed).
///
/// Blanket-implemented; nothing implements it by hand. It exists to say
/// these three bounds once, in one place, with the reason for each.
pub trait ScopeSchema: Schema + Clone + 'static {}

impl<T: Schema + Clone + 'static> ScopeSchema for T {}

// --- registration ----------------------------------------------------------

/// What the store knows about one field with no value in hand: its name,
/// and the grain a set through it is floored to (§5.5).
struct FieldMeta {
    name: String,
    grain: Grain,
}

/// One provider's declaration: the reflection, the at-mount value, and the
/// two type-erased operations the store performs on values of that type.
struct Registration {
    kind: Kind,
    fields: Vec<FieldMeta>,
    index: BTreeMap<String, usize>,
    type_id: TypeId,
    type_name: &'static str,
    /// The value a mount starts from (§4.1: a field's at-mount value is
    /// what it holds before any producer runs), built by the schema from
    /// its own declarations and never handed in by a provider.
    at_mount: Box<dyn Any>,
    /// `T::clone` behind a fn pointer, monomorphised at registration. This
    /// and [`publish`](Registration::publish) are the only two things the
    /// store ever does to a value it cannot name.
    clone_box: fn(&dyn Any) -> Box<dyn Any>,
    /// `dst.clone_from(src)` — what a commit does to publish working state.
    publish: fn(&dyn Any, &mut dyn Any),
}

/// One provider's write-side declaration (§4.4): the vocabulary a document
/// validates against, and the type a raise is decoded into.
///
/// A map of its own rather than a field of [`Registration`], because the two
/// declarations are independent: a structural node that owns no facts may
/// still be the thing a menu raises at, and a scope full of facts may accept
/// nothing at all. One scope, two declarations, either of them optional.
struct IntentReg {
    vocabulary: Vocabulary,
    type_id: TypeId,
    type_name: &'static str,
}

/// A mounted scope: working state, committed state, and what has moved
/// between them.
///
/// Two copies rather than one journal, because §5.4 asks for both halves of
/// the sentence at once — producers read working state, consumers only ever
/// see committed state — and two copies is how you say that with no
/// bookkeeping to get wrong. The cost is one clone per scope per tick that
/// touched it, which is also the §5.2 fence's price tag on bulk data.
struct Live {
    working: Box<dyn Any>,
    committed: Box<dyn Any>,
    /// Fields whose working value differs from their committed one. A set
    /// that puts a field back to its committed value takes it *out* of
    /// here, so a producer that changes its mind mid-tick wakes nobody.
    dirty: BTreeSet<usize>,
    version: u64,
}

/// A producer's licence to write named fields of one scope (§5.3).
///
/// Minted by [`Store::producer`], which is where two producers claiming one
/// field becomes a startup error. Not `Clone`: a token that could be copied
/// would be a second writer that the startup check never saw.
pub struct Producer<T> {
    scope: Scope,
    fields: BTreeSet<usize>,
    _schema: PhantomData<fn() -> T>,
}

impl<T> Producer<T> {
    /// The scope this producer writes.
    pub fn scope(&self) -> Scope {
        self.scope
    }
}

/// A consumer that has asked to be told (§5.1's push verb).
///
/// An opaque id rather than a callback: see the module doc — a callback
/// would run inside the commit, and inside the commit is exactly where a
/// consumer must not be able to look.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubscriberId(u64);

// --- the store -------------------------------------------------------------

/// The application's facts (`APPLICATION.md` §5).
///
/// One per application. The [`Router`](crate::Router) holds it today and
/// the binding holds it after P4; either way there is one, because a second
/// store would be a second answer to "what is true", which is the whole
/// thing this replaces.
///
/// This is also the type a [`Guard`](crate::Guard) is handed (§3.4): a
/// guard is an ordinary Rust function over the facts, which is why the
/// table will never grow a predicate DSL.
pub struct Store {
    registry: BTreeMap<Scope, Registration>,
    /// The write side of the same contract, keyed the same way (§4.4).
    intents: BTreeMap<Scope, IntentReg>,
    live: BTreeMap<Scope, Live>,
    /// Which producer, if any, writes each field. Keyed by field rather
    /// than by producer because the question asked at startup is "does
    /// anyone else write this one?".
    claimed: BTreeSet<(Scope, usize)>,
    subscriptions: BTreeMap<(Scope, usize), BTreeSet<SubscriberId>>,
    /// Subscribers a mount or an unmount owes a waking, delivered with the
    /// next tick's [`Notice`]. A scope arriving or leaving changes what its
    /// fields read as, and a subscriber's contract is that it hears about
    /// that — but the path moves in the walk, not in the barrier, so the
    /// wake waits for the one delivery point there is.
    pending: BTreeSet<SubscriberId>,
    /// The ids the route table registers, installed by the router so a
    /// scope on a misspelt node fails at startup. `None` for a store with
    /// no router, which has nobody to ask.
    nodes: Option<BTreeSet<RouteId>>,
    next_subscriber: u64,
    /// Monotonic, and stamped onto scopes rather than counted per scope, so
    /// a version never repeats across an unmount and a remount — a consumer
    /// caching one cannot be fooled by a scope that came back.
    clock: u64,
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        Self {
            registry: BTreeMap::new(),
            intents: BTreeMap::new(),
            live: BTreeMap::new(),
            claimed: BTreeSet::new(),
            subscriptions: BTreeMap::new(),
            pending: BTreeSet::new(),
            nodes: None,
            next_subscriber: 0,
            clock: 0,
        }
    }

    /// Tell the store which node ids exist, so a scope provided for one
    /// that does not fails at startup. Called by [`Router::new`](crate::Router::new).
    pub(crate) fn knows_nodes(&mut self, ids: &[RouteId]) {
        self.nodes = Some(ids.iter().copied().collect());
    }

    // --- registration ------------------------------------------------------

    /// Declare that `scope` is provided by the schema type `T` (§4.1).
    ///
    /// It takes **no at-mount value**, and that absence is the point. The
    /// schema declares each field's at-mount value and the derive builds the
    /// struct from those same declarations
    /// ([`Schema::at_mount`](crate::Schema::at_mount)), so there is nowhere
    /// for a second opinion to come from: a provider cannot hand in a value
    /// that disagrees with what its schema publishes, because it hands in no
    /// value. That is §4.1's "a field's at-mount value is declared" held by
    /// construction rather than by a check — the `launch_fade` lesson with
    /// the last gap closed.
    ///
    /// [`Scope::Process`] mounts here and now. A [`Scope::Node`] mounts
    /// when its node joins the path and not before.
    pub fn provides<T: ScopeSchema>(&mut self, scope: Scope) -> Result<(), StoreError> {
        self.registered(scope)?;
        if self.registry.contains_key(&scope) {
            return Err(StoreError::AlreadyProvided(scope));
        }
        let kind = crate::schema::reflect_of::<T>();
        let Kind::Record(declared) = &kind else {
            return Err(StoreError::NotARecord {
                scope,
                kind: kind.name(),
            });
        };
        let fields: Vec<FieldMeta> = declared.iter().map(FieldMeta::of).collect();
        let index = fields
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect();
        self.registry.insert(
            scope,
            Registration {
                kind,
                fields,
                index,
                type_id: TypeId::of::<T>(),
                type_name: std::any::type_name::<T>(),
                // The schema's own declarations, assembled — the *only*
                // place a scope's first value comes from.
                at_mount: Box::new(T::at_mount(None)),
                clone_box: |value| match value.downcast_ref::<T>() {
                    Some(value) => Box::new(value.clone()),
                    None => unreachable!("a registration only ever holds its own type"),
                },
                publish: |src, dst| match (src.downcast_ref::<T>(), dst.downcast_mut::<T>()) {
                    (Some(src), Some(dst)) => dst.clone_from(src),
                    _ => unreachable!("a live scope only ever holds its own type"),
                },
            },
        );
        if matches!(scope, Scope::Process) {
            self.mount_scope(scope);
        }
        Ok(())
    }

    /// Claim the fields one producer writes (§5.3's single writer).
    ///
    /// Every name must be a field of the scope's schema, and no other
    /// producer may already claim it — both at **startup**, which is the
    /// whole point: two producers writing one fact is a bug that is
    /// invisible at runtime, because each one looks right on the frames it
    /// wins.
    pub fn producer<T: ScopeSchema>(
        &mut self,
        scope: Scope,
        fields: &[&str],
    ) -> Result<Producer<T>, StoreError> {
        let reg = self.registration::<T>(scope)?;
        let mut claimed = BTreeSet::new();
        for name in fields {
            let idx = reg.field(scope, name)?;
            if self.claimed.contains(&(scope, idx)) {
                return Err(StoreError::AlreadyClaimed {
                    scope,
                    field: (*name).to_string(),
                });
            }
            claimed.insert(idx);
        }
        for idx in &claimed {
            self.claimed.insert((scope, *idx));
        }
        Ok(Producer {
            scope,
            fields: claimed,
            _schema: PhantomData,
        })
    }

    /// Mint an id for a consumer that wants telling.
    pub fn subscriber(&mut self) -> SubscriberId {
        self.next_subscriber += 1;
        SubscriberId(self.next_subscriber)
    }

    /// Subscribe `who` to one field of one scope (§5.1: per-field, §4.2:
    /// per-field selection is what gives the finest invalidation).
    ///
    /// Validated against the *registration*, not the mount, so a consumer
    /// can subscribe to a scope whose node is not on the path yet — which
    /// is the ordinary case, since a subscription is set up when the
    /// consumer is built and the path moves afterwards.
    pub fn subscribe(
        &mut self,
        who: SubscriberId,
        scope: Scope,
        field: &str,
    ) -> Result<(), StoreError> {
        let reg = self
            .registry
            .get(&scope)
            .ok_or(StoreError::NotProvided(scope))?;
        let idx = reg.field(scope, field)?;
        self.subscriptions
            .entry((scope, idx))
            .or_default()
            .insert(who);
        Ok(())
    }

    /// Drop every subscription `who` holds.
    ///
    /// This is §5.6's verb: a departing instance is an unsubscribed-but-
    /// retained consumer, so the binding calls this at the start of a
    /// crossing and drops the tree when the sweep settles. Freezing stops
    /// being special machinery and becomes what unsubscription *is*.
    pub fn unsubscribe_all(&mut self, who: SubscriberId) {
        for subscribers in self.subscriptions.values_mut() {
            subscribers.remove(&who);
        }
        self.subscriptions
            .retain(|_, subscribers| !subscribers.is_empty());
        self.pending.remove(&who);
    }

    // --- reading (§5.1's pull verb) ----------------------------------------

    /// The committed value of a mounted scope. `None` while its node is off
    /// the path.
    ///
    /// The whole of the read verb: a typed consumer downcasts once and then
    /// reads fields directly, paying no dispatch of any kind (§5.1). A `T`
    /// that is not the type the scope was provided with reads as absent,
    /// and fails a `debug_assert` so it is loud where it is a mistake and
    /// silent where it would be a crash.
    pub fn read<T: ScopeSchema>(&self, scope: Scope) -> Option<&T> {
        let live = self.live.get(&scope)?;
        let value = live.committed.downcast_ref::<T>();
        debug_assert!(
            value.is_some(),
            "{scope} is provided as another type than the one read"
        );
        value
    }

    /// The version stamp of a scope's committed state: bumped by every
    /// commit that changed it, and by its mount and unmount. `0` when
    /// nothing is mounted there.
    ///
    /// §5.1's "a version check": a renderer reading in bulk holds the last
    /// stamp it drew and compares one integer. Stamps are globally
    /// monotonic, so a scope that left and came back never reuses one.
    pub fn version(&self, scope: Scope) -> u64 {
        self.live.get(&scope).map(|live| live.version).unwrap_or(0)
    }

    /// Whether the scope exists right now — its node is on the path.
    pub fn is_mounted(&self, scope: Scope) -> bool {
        self.live.contains_key(&scope)
    }

    /// A scope's reflection (§4.3): what a document's selection validates
    /// against, and what a refusal quotes when it says what shape it
    /// wanted.
    pub fn reflection(&self, scope: Scope) -> Option<&Kind> {
        self.registry.get(&scope).map(|reg| &reg.kind)
    }

    // --- the write side (§4.4) ---------------------------------------------

    /// Declare that `scope` accepts the intents `I` publishes.
    ///
    /// The mirror of [`provides`](Store::provides), and deliberately a
    /// second declaration rather than a second argument to the first: §4.4
    /// says a scope publishes the intents it accepts *exactly as* it
    /// publishes the fields it provides, and either half stands without the
    /// other.
    ///
    /// Two checks, both at startup for the same reason the claim check is:
    /// a scope keyed on an id the table does not register would accept
    /// nothing and say nothing, and a vocabulary with one name twice has one
    /// intent that can never be raised.
    pub fn accepts<I: Intents>(&mut self, scope: Scope) -> Result<(), StoreError> {
        self.registered(scope)?;
        if self.intents.contains_key(&scope) {
            return Err(StoreError::AlreadyAccepting(scope));
        }
        let vocabulary = I::vocabulary();
        if let Some(intent) = vocabulary.duplicate() {
            return Err(StoreError::DuplicateIntent {
                scope,
                intent: intent.to_string(),
            });
        }
        self.intents.insert(
            scope,
            IntentReg {
                vocabulary,
                type_id: TypeId::of::<I>(),
                type_name: std::any::type_name::<I>(),
            },
        );
        Ok(())
    }

    /// What a scope accepts (§4.4) — the write half of its contract,
    /// standing beside [`reflection`](Store::reflection)'s read half.
    ///
    /// This is what a document's `events {}` block is checked against at
    /// load and at every hot reload, through
    /// [`Vocabulary::check`](crate::Vocabulary::check).
    pub fn intents(&self, scope: Scope) -> Option<&Vocabulary> {
        self.intents.get(&scope).map(|reg| &reg.vocabulary)
    }

    /// Validate one raise against the scope that published it and land it on
    /// the outbox as an action (§4.4).
    ///
    /// The whole of the write path, in one call: a named raise arrives from
    /// a document, the vocabulary it validated against at load decides what
    /// it means, and the typed intent becomes an `A`. Because it goes on the
    /// outbox rather than anywhere else, §5.4's pinned order carries it —
    /// the next [`tick`](Store::tick) drains it *before* any producer runs,
    /// so a click raised this tick is applied this tick and its echo costs
    /// zero frames.
    ///
    /// `A: From<I>` is what keeps the seam typed end to end. A host whose
    /// action enum already is its intent enum writes nothing (`From<T>` for
    /// `T` is reflexive); a host with a wider action type writes one `From`
    /// impl, which is a total function over a closed enum and so is exactly
    /// the thing the compiler checks. What is *not* here is a string.
    pub fn raise<I, A>(
        &self,
        scope: Scope,
        raise: &Raise,
        out: &mut Outbox<A>,
    ) -> Result<(), RaiseError>
    where
        I: Intents,
        A: From<I>,
    {
        let refuse = |at| RaiseError { scope, at };
        let reg = self
            .intents
            .get(&scope)
            .ok_or_else(|| refuse(Refused::NothingPublished))?;
        if reg.type_id != TypeId::of::<I>() {
            return Err(refuse(Refused::WrongType {
                want: std::any::type_name::<I>(),
                got: reg.type_name,
            }));
        }
        out.push(A::from(I::accept(raise).map_err(refuse)?));
        Ok(())
    }

    // --- the tick (§5.4) ---------------------------------------------------

    /// One frame's transaction: drain, produce, commit — in that order,
    /// because the order is what the signature is.
    ///
    /// The outbox is drained *before* `produce` runs and the actions are
    /// handed to it, so an intent raised this tick is applied this tick and
    /// lands in this tick's commit (§5.4: a click's echo costs zero
    /// frames). `produce` is where the host's services apply those actions
    /// and where every producer runs, including the derived ones (§5.7) —
    /// they read working state through the [`Barrier`], so no consumer ever
    /// sees a torn derivation.
    ///
    /// Nothing outside `produce` can write, and nothing inside it can read
    /// committed state except through
    /// [`Barrier::committed`](Barrier::committed), which is the pre-commit
    /// value on purpose. The returned [`Notice`] is the single, batched
    /// notification for the whole tick.
    pub fn tick<A, F>(&mut self, outbox: &mut Outbox<A>, produce: F) -> Notice
    where
        F: FnOnce(&[A], &mut Barrier<'_>),
    {
        let actions = outbox.drain();
        let mut barrier = Barrier { store: self };
        produce(&actions, &mut barrier);
        self.commit()
    }

    fn commit(&mut self) -> Notice {
        let Store {
            registry,
            live,
            subscriptions,
            pending,
            clock,
            ..
        } = self;
        let mut changed = Vec::new();
        let mut woken: BTreeSet<SubscriberId> = std::mem::take(pending);
        for (scope, state) in live.iter_mut() {
            if state.dirty.is_empty() {
                continue;
            }
            let Some(reg) = registry.get(scope) else {
                continue;
            };
            for idx in &state.dirty {
                changed.push(Changed {
                    scope: *scope,
                    field: reg.fields[*idx].name.clone(),
                });
                if let Some(subscribers) = subscriptions.get(&(*scope, *idx)) {
                    woken.extend(subscribers.iter().copied());
                }
            }
            state.dirty.clear();
            (reg.publish)(&*state.working, &mut *state.committed);
            *clock += 1;
            state.version = *clock;
        }
        Notice {
            woken: woken.into_iter().collect(),
            changed,
        }
    }

    // --- scope lifetime, driven by the walk --------------------------------

    /// `id` joined the path: its scope, if it provides one, begins at the
    /// declared at-mount value.
    ///
    /// Deliberately not public — see the module doc. The walk is the only
    /// thing that may mount a scope, because "scope lifetime is node
    /// presence on the path" is an axiom and not a convention.
    pub(crate) fn mount(&mut self, id: RouteId) {
        self.mount_scope(Scope::Node(id));
    }

    /// `id` left the path: its scope, and everything in it, is dropped.
    pub(crate) fn unmount(&mut self, id: RouteId) {
        let scope = Scope::Node(id);
        if self.live.remove(&scope).is_some() {
            self.wake_scope(scope);
        }
    }

    fn mount_scope(&mut self, scope: Scope) {
        let Some(reg) = self.registry.get(&scope) else {
            return;
        };
        if self.live.contains_key(&scope) {
            return;
        }
        self.clock += 1;
        let live = Live {
            working: (reg.clone_box)(&*reg.at_mount),
            committed: (reg.clone_box)(&*reg.at_mount),
            dirty: BTreeSet::new(),
            version: self.clock,
        };
        self.live.insert(scope, live);
        self.wake_scope(scope);
    }

    /// Everyone subscribed to any field of `scope` is owed a waking: the
    /// scope arriving or leaving changes what those fields read as.
    fn wake_scope(&mut self, scope: Scope) {
        let Store {
            subscriptions,
            pending,
            ..
        } = self;
        let range = (scope, usize::MIN)..=(scope, usize::MAX);
        for (_, subscribers) in subscriptions.range(range) {
            pending.extend(subscribers.iter().copied());
        }
    }

    /// Whether the route table registers the node this scope is keyed on.
    /// Asked of every declaration a provider makes, so a scope or a
    /// vocabulary on a misspelt id fails at startup rather than never
    /// mounting and leaving the empty screen as the diagnostic.
    fn registered(&self, scope: Scope) -> Result<(), StoreError> {
        if let (Scope::Node(id), Some(nodes)) = (scope, &self.nodes) {
            if !nodes.contains(id) {
                return Err(StoreError::UnknownNode(id));
            }
        }
        Ok(())
    }

    fn registration<T: ScopeSchema>(&self, scope: Scope) -> Result<&Registration, StoreError> {
        let reg = self
            .registry
            .get(&scope)
            .ok_or(StoreError::NotProvided(scope))?;
        if reg.type_id != TypeId::of::<T>() {
            return Err(StoreError::WrongType {
                scope,
                want: std::any::type_name::<T>(),
                got: reg.type_name,
            });
        }
        Ok(reg)
    }
}

impl FieldMeta {
    fn of(field: &Field) -> Self {
        Self {
            name: field.name.clone(),
            grain: field.grain.clone(),
        }
    }
}

impl Registration {
    fn field(&self, scope: Scope, name: &str) -> Result<usize, StoreError> {
        self.index
            .get(name)
            .copied()
            .ok_or_else(|| StoreError::NoSuchField {
                scope,
                field: name.to_string(),
            })
    }
}

// --- the barrier -----------------------------------------------------------

/// The inside of the frame barrier: where producers run (§5.4).
///
/// It borrows the store for its whole life, which is the enforcement — no
/// consumer can read while this exists, and this exists only inside
/// [`Store::tick`]'s closure.
pub struct Barrier<'a> {
    store: &'a mut Store,
}

impl Barrier<'_> {
    /// Open this producer's scope for writing. Fails when the scope's node
    /// is not on the path — §5's out-of-scope set, refused once per scope
    /// per tick rather than once per field.
    pub fn writer<'p, T: ScopeSchema>(
        &'p mut self,
        producer: &'p Producer<T>,
    ) -> Result<Writer<'p, T>, StoreError> {
        let scope = producer.scope;
        let store = &mut *self.store;
        // Disjoint field borrows: the registration is read while the live
        // scope is written, and they are different maps.
        let registry = &store.registry;
        let live = &mut store.live;
        let reg = registry.get(&scope).ok_or(StoreError::NotProvided(scope))?;
        if reg.type_id != TypeId::of::<T>() {
            return Err(StoreError::WrongType {
                scope,
                want: std::any::type_name::<T>(),
                got: reg.type_name,
            });
        }
        let state = live.get_mut(&scope).ok_or(StoreError::NotMounted(scope))?;
        Ok(Writer {
            scope,
            reg,
            state,
            claimed: &producer.fields,
            _schema: PhantomData,
        })
    }

    /// Another scope's **working** state — what producers read (§5.4), and
    /// what a derived producer joins over (§5.7). `None` while that scope's
    /// node is off the path.
    pub fn working<T: ScopeSchema>(&self, scope: Scope) -> Option<&T> {
        self.store.live.get(&scope)?.working.downcast_ref::<T>()
    }

    /// Another scope's **committed** state — what a consumer would see if
    /// one could look right now.
    ///
    /// A producer wants [`working`](Barrier::working); this is here because
    /// it is the witness for the sentence that matters: inside the barrier,
    /// with some fields set and some not, this still reads the whole of
    /// last tick and never a mixture. That is the FRP glitch's absence,
    /// observable.
    pub fn committed<T: ScopeSchema>(&self, scope: Scope) -> Option<&T> {
        self.store.live.get(&scope)?.committed.downcast_ref::<T>()
    }
}

/// One producer's writing surface onto one scope, for one tick.
///
/// Every set is equality-checked and swallows a no-op (§5.3), and every set
/// is refused unless this producer claimed the field (§5.3's single
/// writer).
pub struct Writer<'a, T> {
    scope: Scope,
    reg: &'a Registration,
    state: &'a mut Live,
    claimed: &'a BTreeSet<usize>,
    _schema: PhantomData<fn() -> T>,
}

impl<T: ScopeSchema> Writer<'_, T> {
    /// Set one field, if it moved.
    ///
    /// `name` is the field as the schema declares it and `get` reaches the
    /// same field in the struct; the [`set!`](crate::set) macro takes both
    /// from one token, which is the way to write this that cannot drift. A
    /// plain function pointer rather than a closure, for the reason
    /// [`Guard`](crate::Guard) is one: a field accessor captures nothing,
    /// and saying so keeps the writing surface allocation-free.
    ///
    /// Returns whether the value changed. `Ok(false)` is the swallowed
    /// no-op — the compare that used to be hand-rolled sixteen times in one
    /// consumer.
    pub fn set<F: PartialEq>(
        &mut self,
        name: &str,
        get: fn(&mut T) -> &mut F,
        value: F,
    ) -> Result<bool, StoreError> {
        let idx = self.claim(name)?;
        let settled = {
            let Live {
                working, committed, ..
            } = &mut *self.state;
            let (Some(working), Some(committed)) =
                (working.downcast_mut::<T>(), committed.downcast_mut::<T>())
            else {
                return Err(StoreError::WrongType {
                    scope: self.scope,
                    want: std::any::type_name::<T>(),
                    got: self.reg.type_name,
                });
            };
            let slot = get(&mut *working);
            if *slot == value {
                return Ok(false);
            }
            *slot = value;
            // Dirty means "differs from what consumers can see", not "was
            // written": a producer that changes its mind and puts a field
            // back within one tick has changed nothing, and wakes nobody.
            get(&mut *working) == get(&mut *committed)
        };
        match settled {
            true => self.state.dirty.remove(&idx),
            false => self.state.dirty.insert(idx),
        };
        Ok(true)
    }

    /// Set a numeric field, floored to the grain its schema declares
    /// (§5.5).
    ///
    /// The store *applies* the declared grain rather than checking it. A
    /// check would be a float compared against a step, which has no honest
    /// answer, and would fail at whichever frame first computed one ulp too
    /// finely — a worse failure than the one it guards. Applying is also
    /// the truthful reading of the declaration: a grain says what the
    /// field's values *are*, and the store owns the field. Producers keep
    /// their §5.5 duty for facts they quantise before they get here (the
    /// sky clock is floored to the minute where the minute is computed);
    /// this is what stops a forgotten floor from waking every subscriber
    /// every frame.
    pub fn set_num<N: Grained>(
        &mut self,
        name: &str,
        get: fn(&mut T) -> &mut N,
        value: N,
    ) -> Result<bool, StoreError> {
        let idx = self.claim(name)?;
        let value = match &self.reg.fields[idx].grain {
            // Not a round trip through `f64` for an exact field: an `i64`
            // that came back through a float would lose the low bits of a
            // fact nobody asked to be quantised.
            Grain::Exact => value,
            grain => N::from_f64(grain.floor(value.to_f64())),
        };
        self.set(name, get, value)
    }

    /// This scope's working state — what this producer has written so far
    /// this tick, over what last tick committed.
    pub fn working(&self) -> &T {
        self.state
            .working
            .downcast_ref::<T>()
            .expect("a live scope only ever holds its own type")
    }

    /// The grain the schema declares for a field, for a producer that wants
    /// to quantise a fact where it computes it rather than where it sets it.
    pub fn grain(&self, name: &str) -> Result<&Grain, StoreError> {
        let idx = self.reg.field(self.scope, name)?;
        Ok(&self.reg.fields[idx].grain)
    }

    fn claim(&self, name: &str) -> Result<usize, StoreError> {
        let idx = self.reg.field(self.scope, name)?;
        if !self.claimed.contains(&idx) {
            return Err(StoreError::Unclaimed {
                scope: self.scope,
                field: name.to_string(),
            });
        }
        Ok(idx)
    }
}

/// A number a declared [`Grain`] can be applied to.
///
/// The numeric leaves of the schema vocabulary and nothing else: grain is
/// §5.5's answer to a raw per-frame float, so it is a question only numbers
/// can be asked. Blanket implementation is impossible and deliberate — a
/// type that is not a number has no grain, and `set` is the door it uses.
pub trait Grained: PartialEq + Copy {
    fn to_f64(self) -> f64;
    fn from_f64(value: f64) -> Self;
}

macro_rules! impl_grained {
    ($($t:ty),+ $(,)?) => {$(
        impl Grained for $t {
            fn to_f64(self) -> f64 {
                self as f64
            }
            fn from_f64(value: f64) -> Self {
                value as $t
            }
        }
    )+};
}

impl_grained!(f32, f64, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

// --- the notice ------------------------------------------------------------

/// One field that moved in one tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changed {
    pub scope: Scope,
    pub field: String,
}

/// What one tick's commit changed, and who is owed a rerender (§5.4).
///
/// One per tick, whatever happened inside it: ten sets are one notice, and
/// a subscriber that watches ten of the moved fields appears in it once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notice {
    woken: Vec<SubscriberId>,
    changed: Vec<Changed>,
}

impl Notice {
    /// Every subscriber owed a rerender, each at most once, in id order.
    pub fn woken(&self) -> &[SubscriberId] {
        &self.woken
    }

    /// Whether this subscriber is owed one.
    pub fn woke(&self, who: SubscriberId) -> bool {
        self.woken.binary_search(&who).is_ok()
    }

    /// Every field that moved, for a consumer that wants to know *what*
    /// rather than merely *that* — and for a diagnostic.
    pub fn changed(&self) -> &[Changed] {
        &self.changed
    }

    /// Whether the tick moved anything at all. An idle frame's notice is
    /// empty, which is the equality check paying for itself.
    pub fn is_empty(&self) -> bool {
        self.woken.is_empty() && self.changed.is_empty()
    }
}

#[cfg(test)]
mod tests;
