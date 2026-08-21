//! The router: the active path, the walk that derives it, lifecycle,
//! depth arbitration and occlusion.
//!
//! # The walk
//!
//! The path is **derived, never pushed** (axiom 3). Every frame the router
//! asks the game's root resolver which route a path starts at, then walks
//! down: each route on the path answers `resolve_child(cx)`, and the walk
//! stops at the first route that claims none.
//!
//! `resolve_child` returns an `Option`, so a node claims **at most one
//! child, structurally**. That is what makes the walk total and ordered
//! with no arbiter above it, and it is why `n` independent stance booleans
//! — `2ⁿ` states of which `n+1` are legal, so every entry point has to
//! re-assert the invariant — collapse into one `Option<RouteId>` that
//! cannot hold two values (`ROUTING.md` §13.3).
//!
//! # Lifecycle
//!
//! `enter` and `leave` mean *the id entered or left the path*, not "became
//! deepest" (axiom 10). A prompt pushed above the map editor must not
//! disturb the transaction the map editor is holding, so the editor's
//! `leave` does not run when its own exit prompt appears above it.
//!
//! # The store, and the guarded door
//!
//! The walk is where two of §5's sentences are enforced rather than
//! remembered. **Scope lifetime is node presence on the path** (§5): the
//! id that joins the path mounts its scope at the declared at-mount value,
//! and the id that leaves drops it, so state that outlived its screen is
//! not a bug to fix but a thing to write. And **a node guards its own
//! door** (§3.4): before the walk descends onto a child it asks that
//! child's guard, over committed store state, and a refusal stops the
//! descent — ask-then-mount, with the sentence kept for whoever surfaces
//! it. [`Router::ask`] is the same evaluation offered ahead of time, for
//! the panel row that grays itself.
//!
//! The router does not hold the store — the binding does
//! (`APPLICATION_BUILD.md` WP-4.1, §6.1: one binding, one store, one
//! instance per active root). It arrives as an argument to
//! [`resolve`](Router::resolve) and [`ask`](Router::ask) instead, which is
//! what keeps [`Store::mount`] and [`Store::unmount`] crate-private with
//! the walk as their only caller: a host owning the store still cannot
//! mount a scope with it, because the methods that do are not on its side
//! of the crate boundary.
//!
//! # The [`Node`] seam
//!
//! The router is generic over what it routes: `R` is any (possibly
//! unsized) type implementing [`Node`], which states everything the walk
//! needs and nothing that names a surface type. Two dispatches are *not*
//! here, deliberately: platform-event arbitration and drawing, whose
//! signatures carry surface types. Until P4 moves them into the driver,
//! they live in `ogham::route`'s wrapper, built from [`Router::get_mut`]
//! and [`Router::pop_at`].

use std::collections::BTreeMap;
use std::marker::PhantomData;

use crate::store::Store;
use crate::table::{RouteTable, TableError};
use crate::{Departure, Escape, Occlusion, Outbox, Refusal, RouteId};

/// The walk-facing surface of a routed node: everything the router core
/// needs from a route, and nothing that names a surface type.
///
/// A scaffolding seam rather than the final contract. Today the one
/// implementor is `ogham::route`'s bridge over `dyn Route`, whose
/// surface-coupled remainder (`event`'s widget payload, `draw`'s canvas,
/// `read_state`, `own_ui`) stays on the surface side; those methods
/// retire in P2/P4 (`APPLICATION_BUILD.md` WP-2.2, WP-4.1, WP-4.2), at
/// which point what is left of the route contract *is* this trait and
/// the bridge deletes.
pub trait Node<Cx, A> {
    /// Which of this node's children, if any, is active. See
    /// `resolve_child` on the surface-side trait for the contract; the
    /// walk's half of it is: at most one child, re-asked every frame.
    fn resolve_child(&self, cx: &Cx) -> Option<RouteId>;

    /// What this node hides beneath it. See [`Occlusion`].
    ///
    /// The fallback half of §6.2 (occlusion is node data): where the
    /// table declares the node's occlusion this is never called, and
    /// the method survives only for consumers whose tables do not
    /// declare yet. It retires with the P4 driver.
    fn occludes(&self) -> Occlusion;

    /// Ticked while on the path, deepest last.
    fn update(&mut self, cx: &Cx, out: &mut Outbox<A>, dt: f32);

    /// Escape, offered to the deepest active node first and then up.
    fn escape(&mut self, cx: &Cx, out: &mut Outbox<A>) -> Escape;

    /// This node asked to leave in response to something that was not
    /// Escape. Read *and cleared* after every event dispatch.
    fn take_leave_request(&mut self) -> bool;

    /// A child of this node popped, and this node is what was claiming
    /// it. Stop.
    fn child_popped(&mut self, child: RouteId);

    /// The id entered the path. Not "became deepest" (axiom 10).
    fn enter(&mut self, cx: &mut Cx);

    /// The id left the path. Node-owned state dies here.
    fn leave(&mut self, cx: &mut Cx);

    /// How this node leaves the frame when the path moves to `to`.
    fn depart(&mut self, to: Option<RouteId>) -> Departure;
}

/// What the router did with an Escape, so the host can tell "nobody wanted
/// it" from "it was consumed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeOutcome {
    /// A route popped; the path is shorter.
    Popped(RouteId),
    /// A route raised a prompt instead of leaving. The path is unchanged
    /// this frame; the prompt appears on the next walk, because the
    /// prompt is a child route and the walk is what finds it.
    Prompted(RouteId),
    /// A route claimed the key and did nothing visible with it.
    Consumed(RouteId),
    /// Every route on the path declined. The host may do what it likes —
    /// this is the only case in which anything above the router hears
    /// about Escape at all.
    Unclaimed,
}

/// One router, for one window.
///
/// `Cx` is the game's services; `A` is its action type. Neither is erased,
/// because all three consumers have exactly one session and one action
/// enum, and erasing them would buy nothing but a vtable. `R` is what the
/// table's ids resolve to — see the module doc's [`Node`] seam.
pub struct Router<Cx, A, R: ?Sized> {
    table: RouteTable,
    routes: BTreeMap<RouteId, Box<R>>,
    root: Box<dyn Fn(&Cx) -> RouteId>,
    path: Vec<RouteId>,
    /// Reported once per offending (parent, child) pair rather than every
    /// frame: a bad `resolve_child` would otherwise print sixty times a
    /// second and bury everything else.
    reported: BTreeMap<RouteId, RouteId>,
    /// Set when the last `resolve` changed the path, so a host can drive
    /// a transition without diffing the path itself.
    last_departure: Option<(RouteId, Departure)>,
    /// The door the last walk asked at and was refused (§3.4). Re-derived
    /// every walk, never remembered — a refusal is a reading of the facts
    /// right now, and a stale one is exactly the shadow rooms table this
    /// replaces.
    refused: Option<(RouteId, Refusal)>,
    /// `A` appears only through [`Node`]'s methods, not in any field.
    _actions: PhantomData<fn(A)>,
}

impl<Cx, A, R: Node<Cx, A> + ?Sized> Router<Cx, A, R> {
    /// Build a router over a validated table.
    ///
    /// `root` is the game's lifecycle function: connected, seated,
    /// playing. About fifteen lines in the largest consumer, because
    /// everything below the first segment is `resolve_child`'s.
    pub fn new(
        table: RouteTable,
        routes: Vec<(RouteId, Box<R>)>,
        root: impl Fn(&Cx) -> RouteId + 'static,
    ) -> Result<Self, TableError> {
        table.validate()?;
        let mut map: BTreeMap<RouteId, Box<R>> = BTreeMap::new();
        for (id, route) in routes {
            if map.insert(id, route).is_some() {
                return Err(TableError::DuplicateId(id));
            }
        }
        // A registered id with no handler is the same class of mistake as
        // a registered id with no `screen` block, and is caught here for
        // the same reason: at startup, naming the id.
        for id in table.ids() {
            if !map.contains_key(id) {
                return Err(TableError::UnknownParent {
                    child: id,
                    parent: "<no handler registered>",
                });
            }
        }
        Ok(Self {
            table,
            routes: map,
            root: Box::new(root),
            path: Vec::new(),
            reported: BTreeMap::new(),
            last_departure: None,
            refused: None,
            _actions: PhantomData,
        })
    }

    pub fn table(&self) -> &RouteTable {
        &self.table
    }

    /// Ask whether a node's door would open, without going there (§3.4).
    ///
    /// The panel row that grays itself and the walk that declines to
    /// descend call the same function over the same facts, so the sentence
    /// a player reads is the sentence that stopped them. An unguarded door
    /// is open.
    pub fn ask(&self, id: RouteId, store: &Store) -> Result<(), Refusal> {
        match self.table.guard_of(id) {
            Some(guard) => guard(store),
            None => Ok(()),
        }
    }

    /// The door the last walk was refused at, if it was refused at one.
    pub fn refused(&self) -> Option<(RouteId, &Refusal)> {
        self.refused.as_ref().map(|(id, why)| (*id, why))
    }

    /// The active path, outermost first.
    pub fn path(&self) -> &[RouteId] {
        &self.path
    }

    /// The deepest active route's id, if any.
    pub fn deepest(&self) -> Option<RouteId> {
        self.path.last().copied()
    }

    pub fn is_active(&self, id: RouteId) -> bool {
        self.path.contains(&id)
    }

    pub fn get(&self, id: RouteId) -> Option<&R> {
        self.routes.get(id).map(|b| b.as_ref())
    }

    pub fn get_mut(&mut self, id: RouteId) -> Option<&mut R> {
        self.routes.get_mut(id).map(|b| b.as_mut())
    }

    /// Recompute the path from session state, running `leave` for every id
    /// that dropped off it and `enter` for every id that joined.
    ///
    /// Call once per frame, before `update`. Returns `true` if the path
    /// changed.
    pub fn resolve(&mut self, cx: &mut Cx, store: &mut Store) -> bool {
        let next = self.walk(cx, store);
        if next == self.path {
            self.last_departure = None;
            return false;
        }

        // `leave` runs deepest-first, so a child releases before the
        // parent it was standing on. `enter` runs outermost-first for the
        // same reason in reverse.
        let departing: Vec<RouteId> = self
            .path
            .iter()
            .filter(|id| !next.contains(id))
            .copied()
            .collect();
        let arriving: Vec<RouteId> = next
            .iter()
            .filter(|id| !self.path.contains(id))
            .copied()
            .collect();

        let going_to = next.last().copied();
        if let Some(outgoing) = departing.last().copied() {
            if let Some(route) = self.routes.get_mut(outgoing) {
                let departure = route.depart(going_to);
                self.last_departure = Some((outgoing, departure));
            }
        } else {
            self.last_departure = None;
        }

        for id in departing.iter().rev().copied() {
            if let Some(route) = self.routes.get_mut(id) {
                route.leave(cx);
            }
        }
        // After `leave`, because a node releasing its hold on the world may
        // want to read the facts it was standing on one last time; before
        // the path moves, because from here on the scope is gone.
        for id in departing {
            store.unmount(id);
        }
        self.path = next;
        // Before `enter`, so a node that reacts to arriving finds its scope
        // already standing at the at-mount value its schema declared.
        for id in &arriving {
            store.mount(id);
        }
        for id in arriving {
            if let Some(route) = self.routes.get_mut(id) {
                route.enter(cx);
            }
        }
        true
    }

    /// The departure the last path change asked for, if it asked for one.
    pub fn last_departure(&self) -> Option<(RouteId, Departure)> {
        self.last_departure
    }

    /// Derive the path without touching lifecycle. Split out so `resolve`
    /// can diff against the current path before running anything.
    fn walk(&mut self, cx: &Cx, store: &Store) -> Vec<RouteId> {
        self.refused = None;
        let mut path = Vec::new();
        let mut id = (self.root)(cx);
        loop {
            if !self.table.contains(id) {
                self.report_once(id, "<root>");
                break;
            }
            if path.contains(&id) {
                // A `resolve_child` pointing back up the path. The table
                // is acyclic, so this is a handler bug rather than a
                // table one; stopping here keeps the frame renderable.
                self.report_once(id, "<already on the path>");
                break;
            }
            path.push(id);
            let Some(route) = self.routes.get(id) else {
                break;
            };
            let Some(child) = route.resolve_child(cx) else {
                break;
            };
            if !self.table.is_child_of(child, id) {
                self.report_once(child, id);
                break;
            }
            // Ask-then-mount (§3.4). The guard rules on the *child*, over
            // committed facts, and a refusal simply ends the walk here —
            // the parent keeps the frame, and the sentence is kept for
            // whoever surfaces it. A root is not asked: the lifecycle put
            // it there and only the lifecycle can take it away, which is
            // the same argument [`pop_at`](Self::pop_at) makes.
            if let Some(guard) = self.table.guard_of(child) {
                if let Err(refusal) = guard(store) {
                    self.refused = Some((child, refusal));
                    break;
                }
            }
            id = child;
        }
        path
    }

    fn report_once(&mut self, child: RouteId, parent: RouteId) {
        if self.reported.insert(child, parent).is_none() {
            eprintln!(
                "route: `{parent}` resolved to `{child}`, which is not a registered child of it; \
                 rendering as if it resolved to nothing"
            );
        }
    }

    /// The ids that actually render this frame, outermost first, after
    /// occlusion.
    ///
    /// Everything from the deepest [`Occlusion::Surface`] route down is
    /// dropped; everything above the deepest [`Occlusion::View`] route
    /// keeps its `draw` but loses its screen. See
    /// [`visible_views`](Self::visible_views) for the second half.
    pub fn drawing(&self) -> Vec<RouteId> {
        let cut = self.deepest_at_least(Occlusion::Surface);
        match cut {
            Some(i) => self.path[i..].to_vec(),
            None => self.path.clone(),
        }
    }

    /// The ids whose *screens* render this frame, outermost first.
    ///
    /// A route declaring [`Occlusion::None`] leaves its ancestors' screens
    /// up — which is what the exit prompts want and cannot express today,
    /// and is the seventh symptom bug in `ROUTING.md` §2.4.
    pub fn visible_views(&self) -> Vec<RouteId> {
        let cut = self.deepest_at_least(Occlusion::View);
        match cut {
            Some(i) => self.path[i..].to_vec(),
            None => self.path.clone(),
        }
    }

    /// Index of the deepest route on the path whose occlusion is at least
    /// `level`. Everything before it is hidden at that level.
    fn deepest_at_least(&self, level: Occlusion) -> Option<usize> {
        self.path
            .iter()
            .enumerate()
            .rev()
            .find(|(_, id)| self.occlusion_of(id) >= level)
            .map(|(i, _)| i)
    }

    /// One node's occlusion: the table's declaration where there is one
    /// (§6.2 — occlusion is node data, so the arithmetic reads the
    /// table first), else the node's `occludes()` method, which is the
    /// seam un-migrated consumers still answer through. An id with no
    /// handler is unreachable past [`Router::new`]'s check and answers
    /// [`Occlusion::None`], which never cuts.
    fn occlusion_of(&self, id: RouteId) -> Occlusion {
        self.table.declared_occlusion(id).unwrap_or_else(|| {
            self.routes
                .get(id)
                .map(|r| r.occludes())
                .unwrap_or(Occlusion::None)
        })
    }

    /// Tick every active route, outermost first.
    pub fn update(&mut self, cx: &Cx, out: &mut Outbox<A>, dt: f32) {
        for id in self.path.clone() {
            if let Some(route) = self.routes.get_mut(id) {
                route.update(cx, out, dt);
            }
        }
    }

    /// Tell whoever is claiming the route at `depth` to stop.
    ///
    /// The path is not edited here — the next `resolve` shortens it,
    /// which is what keeps "derived, never pushed" true for the two
    /// gestures whose whole purpose is to change the path.
    ///
    /// Public for one caller: the surface-side event dispatch, which asks
    /// [`take_leave_request`](Node::take_leave_request) after every
    /// delivery and reports the answer here. When P4 moves that dispatch
    /// into the driver this stays the seam it uses.
    pub fn pop_at(&mut self, depth: usize) {
        if depth == 0 {
            // A root route asking to leave has nobody to ask. The
            // lifecycle is what put it there, and only the lifecycle can
            // take it away.
            return;
        }
        let (child, parent) = (self.path[depth], self.path[depth - 1]);
        if let Some(parent) = self.routes.get_mut(parent) {
            parent.child_popped(child);
        }
    }

    /// Offer Escape to the deepest active route, then up the path.
    ///
    /// No layer above intercepts, which is the whole of axiom 9. A `Pop`
    /// does not edit the path directly: it asks the route to stop
    /// claiming itself, and the next `resolve` is what shortens the path.
    /// That keeps "the path is derived, never pushed" true even for the
    /// one gesture whose whole purpose is to change it.
    pub fn escape(&mut self, cx: &Cx, out: &mut Outbox<A>) -> EscapeOutcome {
        let path = self.path.clone();
        for (depth, id) in path.iter().copied().enumerate().rev() {
            let Some(route) = self.routes.get_mut(id) else {
                continue;
            };
            match route.escape(cx, out) {
                Escape::Pop => {
                    // Tell whoever was claiming this id to stop. Without
                    // this the next walk re-derives the same path and the
                    // pop does nothing — the child cannot shorten a path
                    // it does not derive.
                    self.pop_at(depth);
                    return EscapeOutcome::Popped(id);
                }
                Escape::Prompt => return EscapeOutcome::Prompted(id),
                Escape::Ignore => return EscapeOutcome::Consumed(id),
                Escape::Fall => continue,
            }
        }
        EscapeOutcome::Unclaimed
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::schema::{Field, Kind, Schema};
    use crate::set;
    use crate::store::{Producer, Scope};

    /// A node that claims a fixed child and reports a fixed occlusion,
    /// so a test can make the table and the method disagree on purpose.
    struct Fixed {
        child: Option<RouteId>,
        occludes: Occlusion,
    }

    impl Node<(), ()> for Fixed {
        fn resolve_child(&self, _cx: &()) -> Option<RouteId> {
            self.child
        }
        fn occludes(&self) -> Occlusion {
            self.occludes
        }
        fn update(&mut self, _cx: &(), _out: &mut Outbox<()>, _dt: f32) {}
        fn escape(&mut self, _cx: &(), _out: &mut Outbox<()>) -> Escape {
            Escape::Fall
        }
        fn take_leave_request(&mut self) -> bool {
            false
        }
        fn child_popped(&mut self, _child: RouteId) {}
        fn enter(&mut self, _cx: &mut ()) {}
        fn leave(&mut self, _cx: &mut ()) {}
        fn depart(&mut self, _to: Option<RouteId>) -> Departure {
            Departure::Cut
        }
    }

    /// A router and the store a binding hands it, paired.
    ///
    /// The store left the router in WP-4.1 (§6.1: the binding owns the one
    /// store), so the walk's own tests stand the pair up the way the
    /// binding does rather than reaching for a field that is no longer
    /// there. Every method here is one line of the binding's frame.
    struct Bound {
        router: Router<(), (), Fixed>,
        store: Store,
    }

    impl Bound {
        fn new(table: RouteTable, routes: Vec<(RouteId, Box<Fixed>)>, root: RouteId) -> Self {
            let store = Store::over(&table);
            Self {
                router: Router::new(table, routes, move |_| root)
                    .expect("this table is well formed"),
                store,
            }
        }

        fn resolve(&mut self) -> bool {
            self.router.resolve(&mut (), &mut self.store)
        }

        fn path(&self) -> &[RouteId] {
            self.router.path()
        }

        fn ask(&self, id: RouteId) -> Result<(), Refusal> {
            self.router.ask(id, &self.store)
        }

        fn refused(&self) -> Option<(RouteId, &Refusal)> {
            self.router.refused()
        }

        fn visible_views(&self) -> Vec<RouteId> {
            self.router.visible_views()
        }

        fn claims(&mut self, id: RouteId, child: Option<RouteId>) {
            self.router.get_mut(id).unwrap().child = child;
        }
    }

    fn prompt_router(declare: bool) -> Bound {
        let mut t = RouteTable::new();
        t.at_root("workspace").under("prompt", "workspace");
        if declare {
            t.occludes("prompt", Occlusion::None);
        }
        Bound::new(
            t,
            vec![
                (
                    "workspace",
                    Box::new(Fixed {
                        child: Some("prompt"),
                        occludes: Occlusion::View,
                    }),
                ),
                (
                    "prompt",
                    Box::new(Fixed {
                        child: None,
                        occludes: Occlusion::View,
                    }),
                ),
            ],
            "workspace",
        )
    }

    /// The exit-prompt shape §6.2 exists for: the prompt's method still
    /// answers the old default, the table declares `Occlusion::None`,
    /// and the declaration wins — the workspace's screen stays up
    /// beneath the prompt, with no route object consulted about it.
    #[test]
    fn declared_occlusion_outranks_the_method() {
        let mut r = prompt_router(true);
        r.resolve();
        assert_eq!(r.visible_views(), vec!["workspace", "prompt"]);
    }

    /// No declaration, and the method still rules: an un-migrated
    /// consumer changes nothing and loses nothing.
    #[test]
    fn an_undeclared_node_still_answers_through_its_method() {
        let mut r = prompt_router(false);
        r.resolve();
        assert_eq!(r.visible_views(), vec!["prompt"]);
    }

    // --- the store on the walk (WP-2.2) ------------------------------------

    /// celia's session scope: the facts that outlive both instance roots.
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Session {
        seats: i64,
    }

    impl Schema for Session {
        fn reflect() -> Kind {
            Kind::Record(vec![Field::new("seats", Kind::Int)])
        }
        fn at_mount(_: Option<&crate::Lit>) -> Self {
            Self {
                seats: i64::at_mount(None),
            }
        }
        fn type_name() -> Option<&'static str> {
            Some("Session")
        }
    }

    std::thread_local! {
        /// How many [`Watched`] values are alive on this thread.
        ///
        /// A thread local rather than a handle carried in the value,
        /// because [`Store::provides`](crate::Store::provides) takes no
        /// value: a scope's first value is assembled by its own schema
        /// (§4.1), so a witness has no ride in on one. Thread-local rather
        /// than static so the count belongs to the one test that reads it.
        static ALIVE: AtomicUsize = const { AtomicUsize::new(0) };
    }

    fn alive() -> usize {
        ALIVE.with(|n| n.load(Ordering::SeqCst))
    }

    /// A view scope that says when it is dropped, so "the state died with
    /// its node" is witnessed rather than inferred from an absent read.
    #[derive(Debug)]
    struct Watched {
        pane: String,
    }

    impl Watched {
        fn born(pane: String) -> Self {
            ALIVE.with(|n| n.fetch_add(1, Ordering::SeqCst));
            Self { pane }
        }
    }

    impl Clone for Watched {
        fn clone(&self) -> Self {
            Self::born(self.pane.clone())
        }
    }

    impl Drop for Watched {
        fn drop(&mut self) {
            ALIVE.with(|n| n.fetch_sub(1, Ordering::SeqCst));
        }
    }

    impl Schema for Watched {
        fn reflect() -> Kind {
            Kind::Record(vec![Field::new("pane", Kind::Str)])
        }
        fn at_mount(_: Option<&crate::Lit>) -> Self {
            Self::born(String::at_mount(None))
        }
        fn type_name() -> Option<&'static str> {
            Some("Watched")
        }
    }

    /// The celia shape: a structural session over a lobby that claims the
    /// arena. `claims` is what the lobby's `resolve_child` answers.
    fn celia_router(claims_arena: bool) -> Bound {
        let mut t = RouteTable::new();
        t.at_root("session")
            .under("lobby", "session")
            .under("arena", "lobby");
        t.structural("session")
            .mounts("lobby", "data/ui/lobby.ogh")
            .mounts("arena", "data/ui/arena.ogh");
        t.guard("arena", needs_two_players);
        let node = |child: Option<RouteId>| {
            Box::new(Fixed {
                child,
                occludes: Occlusion::View,
            })
        };
        Bound::new(
            t,
            vec![
                ("session", node(Some("lobby"))),
                ("lobby", node(claims_arena.then_some("arena"))),
                ("arena", node(None)),
            ],
            "session",
        )
    }

    /// §3.4, as a game writes one: an ordinary Rust function over the
    /// store's facts, with the sentence authored once.
    fn needs_two_players(store: &Store) -> Result<(), Refusal> {
        let seated = store
            .read::<Session>(Scope::Node("session"))
            .map(|s| s.seats)
            .unwrap_or(0);
        match seated >= 2 {
            true => Ok(()),
            false => Err(Refusal::new("Needs two more players.")),
        }
    }

    /// **A scope dies with its node.** The lobby's scope stands while the
    /// lobby is on the path and is gone the frame it leaves — read as
    /// absent, dropped for real (the witness counts), and starting again
    /// from the declared at-mount value when the node comes back. This is
    /// what makes half of regency's `teardown` unwritable.
    #[test]
    fn a_scope_dies_with_its_node_and_comes_back_at_mount() {
        let mut r = celia_router(true);
        r.store
            .provides::<Watched>(Scope::Node("arena"))
            .expect("`arena` is a registered node");
        assert_eq!(alive(), 1, "the at-mount template the schema built");
        let pane = r
            .store
            .producer::<Watched>(Scope::Node("arena"), &["pane"])
            .unwrap();
        // The arena's door is open once the seats are filled.
        let seats = session_of(&mut r);
        set_seats(&mut r, &seats, 2);

        r.resolve();
        assert_eq!(r.path(), ["session", "lobby", "arena"]);
        assert_eq!(alive(), 3, "the template and two copies");
        r.store.tick(&mut Outbox::<()>::new(), |_, b| {
            let mut w = b.writer(&pane).unwrap();
            set!(w, pane, "tohri".to_string()).unwrap();
        });
        assert_eq!(
            r.store.read::<Watched>(Scope::Node("arena")).unwrap().pane,
            "tohri"
        );

        // The lobby stops claiming the arena: the node leaves the path.
        r.claims("lobby", None);
        r.resolve();
        assert_eq!(r.path(), ["session", "lobby"]);
        assert!(r.store.read::<Watched>(Scope::Node("arena")).is_none());
        assert_eq!(alive(), 1, "only the at-mount template is left");

        // And back. What it holds is the at-mount value, because there was
        // nowhere for the old one to have been kept.
        r.claims("lobby", Some("arena"));
        r.resolve();
        assert_eq!(
            r.store.read::<Watched>(Scope::Node("arena")).unwrap().pane,
            "",
            "a scope that came back is a new scope"
        );
    }

    /// **A node guards its own door** (§3.4). The lobby claims the arena
    /// every frame; the guard rules on the facts, and while it refuses the
    /// walk simply stops at the lobby — with the authored sentence kept for
    /// whoever surfaces it.
    #[test]
    fn a_guarded_door_refuses_the_walk_and_keeps_its_sentence() {
        let mut r = celia_router(true);
        let seats = session_of(&mut r);

        assert_eq!(r.path(), ["session", "lobby"], "the door stayed shut");
        let (id, why) = r.refused().expect("the walk was refused at one");
        assert_eq!(id, "arena");
        assert_eq!(why.sentence(), "Needs two more players.");

        set_seats(&mut r, &seats, 2);
        r.resolve();
        assert_eq!(r.path(), ["session", "lobby", "arena"]);
        assert!(r.refused().is_none(), "a refusal is never remembered");
    }

    /// The panel row that grays and the walk that declines read the same
    /// sentence, because they call the same function (§3.4's "written once,
    /// read everywhere").
    #[test]
    fn ask_answers_what_the_walk_would_have_been_refused_with() {
        let mut r = celia_router(false);
        let seats = session_of(&mut r);

        // Nobody is walking towards the arena, and the row still knows.
        assert_eq!(
            r.ask("arena").unwrap_err().sentence(),
            "Needs two more players."
        );
        assert!(r.ask("lobby").is_ok(), "an unguarded door is open");

        set_seats(&mut r, &seats, 2);
        assert!(r.ask("arena").is_ok());
    }

    /// A guard is a reading of the facts *right now*: it re-runs every
    /// walk, so a door that opened stays open only while the reason does.
    #[test]
    fn a_door_that_opened_shuts_again_when_its_reason_goes() {
        let mut r = celia_router(true);
        let seats = session_of(&mut r);
        set_seats(&mut r, &seats, 2);
        r.resolve();
        assert_eq!(r.path().len(), 3);

        set_seats(&mut r, &seats, 1);
        r.resolve();
        assert_eq!(r.path(), ["session", "lobby"]);
    }

    /// Provide the session scope, walk once so it mounts, and hand back
    /// the one producer allowed to write its seats.
    fn session_of(r: &mut Bound) -> Producer<Session> {
        let scope = Scope::Node("session");
        r.store.provides::<Session>(scope).unwrap();
        r.resolve();
        r.store
            .producer::<Session>(scope, &["seats"])
            .expect("one producer, once")
    }

    /// Fill the seats through that producer, so the guard's facts are the
    /// store's and not a test's fixture.
    fn set_seats(r: &mut Bound, seats: &Producer<Session>, count: i64) {
        r.store.tick(&mut Outbox::<()>::new(), |_, b| {
            let mut w = b.writer(seats).unwrap();
            set!(w, seats, count).unwrap();
        });
    }

    /// A scope keyed on an id the table does not register fails at startup,
    /// rather than never mounting and letting an empty screen be the
    /// diagnostic.
    #[test]
    fn a_scope_on_a_misspelt_node_fails_at_startup() {
        let mut r = celia_router(false);
        assert_eq!(
            r.store.provides::<Session>(Scope::Node("lobbi")).err(),
            Some(crate::StoreError::UnknownNode("lobbi"))
        );
    }
}
