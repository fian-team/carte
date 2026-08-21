//! The route table: a DAG of ids, static, built once at startup.
//!
//! A route is registered under each parent it may appear beneath. Settings
//! reachable from the title *and* from pause is **two edges, one handler,
//! one screen block** — not two routes and not a remembered origin (axiom
//! 2). The walk records how you arrived, which is exactly the information
//! `Shell::sub_origin` hand-tracked with a one-slot enum.
//!
//! Registered is not active (axiom 10). This is a static description of
//! what *may* appear beneath what; which of them is on the path right now
//! is the [`Router`](crate::Router)'s, derived every frame.
//!
//! Since WP-1.2 a node is more than an id and its edges: it declares a
//! [`Tier`] (`APPLICATION.md` §3.2), an instance root its [`Area`] and
//! its document, any node its [`Occlusion`] and its [`Guard`] (§3.4) —
//! all of it static table data, answerable with no route object, which
//! is what lets the binding mount and the host pick music before any
//! handler exists. One naming trap, called out because the words nearly
//! collide: [`at_root`](RouteTable::at_root) is *placement* (a path may
//! start here), [`Tier::InstanceRoot`] is *meaning* (a document mounts
//! here). They are orthogonal — untold_lore's four instance roots are
//! graph roots, celia's lobby and arena are instance roots beneath a
//! structural `session`.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Area, Guard, Occlusion, RouteId};

/// A node's tier (`APPLICATION.md` §3.2): what standing on it *means*.
///
/// The default is [`View`](Tier::View), undeclared, because it is the
/// common case and the un-migrated one: every node of today's consumers
/// selects a screen within the enclosing instance's document. The other
/// two tiers are declarations — [`mounts`](RouteTable::mounts) and
/// [`structural`](RouteTable::structural).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tier {
    /// Mounts its own document; crossing into one is teardown/mount with
    /// a marked passage. The document is table data rather than a
    /// route's answer because the binding mounts what the table names
    /// (§6.1) — no host hand-mounts a second document.
    InstanceRoot {
        /// The document the binding mounts for this instance.
        document: &'static str,
    },
    /// Selects a screen/outlet within the enclosing instance. With
    /// several parents (§3.2, the table is a DAG), the screen resolves
    /// against the *enclosing* instance's document — see
    /// [`enclosing_instance`](RouteTable::enclosing_instance).
    #[default]
    View,
    /// Mounts nothing, selects nothing; exists to own a scope whose
    /// lifetime spans its children (celia's `session` holds the focused
    /// character precisely because that fact must outlive both the
    /// lobby and the arena). That it *cannot* name a document is the
    /// variant's shape, not a validation rule.
    Structural,
}

/// What is wrong with a table, found at startup rather than at the frame
/// that would have tripped over it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableError {
    /// An edge names a parent nobody registered. Almost always a typo, so
    /// the message carries both ends.
    UnknownParent { child: RouteId, parent: RouteId },
    /// The same id was registered twice.
    DuplicateId(RouteId),
    /// The graph has a cycle, so some path would never terminate. Carries
    /// one id on the cycle — enough to find it, and cheaper than
    /// reconstructing the whole loop.
    Cycle(RouteId),
    /// Nothing is registered at the root, so no path can start.
    NoRoot,
    /// A tier, area, occlusion or guard was declared for an id nobody
    /// registered. Attributes do not register (the same argument as
    /// [`an edge does not declare its parent`](RouteTable::under)): if
    /// they did, a typo'd id would quietly become a real node, and the
    /// first path near it would be the diagnostic.
    AttributeOnUnknown {
        id: RouteId,
        /// Which declaration it was: `"tier"`, `"area"`, `"occlusion"`
        /// or `"guard"`.
        attribute: &'static str,
    },
    /// The same node declared the same attribute more than once,
    /// contradictorily — an instance root also declared structural, an
    /// area declared twice with different grounds, a second guard. One
    /// node, one answer; two call sites disagreeing is exactly the
    /// startup-loud failure, not a last-wins.
    Redeclared {
        id: RouteId,
        attribute: &'static str,
    },
    /// An area was declared on a view or a structural node. An area is
    /// the ground an *instance* stands on (`APPLICATION.md` §3.2); a
    /// view stands wherever its enclosing instance does.
    AreaOnNonInstanceRoot(RouteId),
    /// Areas are declared but no default area is. The catch-all must be
    /// chosen on purpose — the property inherited from the `side_of`
    /// this attribute replaces: a root that forgets to declare its area
    /// lands somewhere *safe* (untold_lore chose the menu, where a
    /// wrong answer is a wrong picture rather than a wrong sound over a
    /// live world), never somewhere lucky.
    NoDefaultArea,
}

impl std::fmt::Display for TableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TableError::UnknownParent { child, parent } => write!(
                f,
                "route `{child}` is registered under `{parent}`, which is not a registered route"
            ),
            TableError::DuplicateId(id) => {
                write!(f, "route `{id}` is registered more than once")
            }
            TableError::Cycle(id) => write!(
                f,
                "the route table has a cycle through `{id}`; a path through it would not terminate"
            ),
            TableError::NoRoot => write!(
                f,
                "no route is registered at the root, so no path can start"
            ),
            TableError::AttributeOnUnknown { id, attribute } => write!(
                f,
                "`{id}` has a declared {attribute} but is not a registered route"
            ),
            TableError::Redeclared { id, attribute } => write!(
                f,
                "route `{id}` declares its {attribute} more than once; one node, one answer"
            ),
            TableError::AreaOnNonInstanceRoot(id) => write!(
                f,
                "route `{id}` declares an area but is not an instance root; an area is the \
                 ground an instance stands on, and a view stands wherever its enclosing \
                 instance does"
            ),
            TableError::NoDefaultArea => write!(
                f,
                "areas are declared but no default area is; the catch-all must be chosen on \
                 purpose, so a root that forgets its area lands somewhere safe rather than \
                 somewhere lucky"
            ),
        }
    }
}

impl std::error::Error for TableError {}

/// The static shape of a game's surfaces.
#[derive(Clone, Debug, Default)]
pub struct RouteTable {
    /// Registration order, which is also the order diagnostics list ids
    /// in. `BTreeMap` for the edges so error messages are deterministic.
    ids: Vec<RouteId>,
    parents: BTreeMap<RouteId, BTreeSet<RouteId>>,
    roots: BTreeSet<RouteId>,
    /// Declared tiers; absence is [`Tier::View`], the common case.
    tiers: BTreeMap<RouteId, Tier>,
    /// Declared grounds. Instance roots only, held by validation.
    areas: BTreeMap<RouteId, Area>,
    /// The catch-all ground (see [`TableError::NoDefaultArea`]).
    default_area: Option<Area>,
    /// Declared occlusions. Absence means the walk still asks the
    /// node's `occludes()` method — the un-migrated consumers' seam.
    occlusions: BTreeMap<RouteId, Occlusion>,
    /// One guard per node (§3.4). The one-list property lives in this
    /// being the only map, next to the node it rules for.
    guards: BTreeMap<RouteId, Guard>,
    /// Contradictory declarations, remembered until [`validate`]
    /// (RouteTable::validate) — the declaration methods chain and
    /// cannot return errors, so the error waits where every other
    /// table error already does.
    redeclared: BTreeSet<(RouteId, &'static str)>,
}

impl RouteTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `id` at the root — a route a path may *start* at.
    ///
    /// Several routes are roots (a title, a world, a library); which one a
    /// path starts at each frame is the game's root resolver, not the
    /// table's.
    pub fn at_root(&mut self, id: RouteId) -> &mut Self {
        self.declare(id);
        self.roots.insert(id);
        self
    }

    /// Register `id` beneath `parent`. Call once per parent: this is the
    /// edge, and a route reachable from three places has three of them.
    pub fn under(&mut self, id: RouteId, parent: RouteId) -> &mut Self {
        self.declare(id);
        self.parents.entry(id).or_default().insert(parent);
        self
    }

    fn declare(&mut self, id: RouteId) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }

    /// Declare `id` an instance root that mounts `document`.
    ///
    /// Tier, not placement: an instance root may sit at the graph's root
    /// (untold_lore's four) or beneath a structural node (celia's lobby
    /// and arena under `session`), so this composes with
    /// [`at_root`](Self::at_root)/[`under`](Self::under) rather than
    /// replacing them. Declaring the same id structural too is a startup
    /// error, not a last-wins.
    pub fn mounts(&mut self, id: RouteId, document: &'static str) -> &mut Self {
        self.set_tier(id, Tier::InstanceRoot { document });
        self
    }

    /// Declare `id` structural: it mounts nothing and selects nothing,
    /// existing to own a scope whose lifetime spans its children (§3.2).
    pub fn structural(&mut self, id: RouteId) -> &mut Self {
        self.set_tier(id, Tier::Structural);
        self
    }

    fn set_tier(&mut self, id: RouteId, tier: Tier) {
        match self.tiers.get(&id) {
            Some(prev) if *prev != tier => {
                self.redeclared.insert((id, "tier"));
            }
            _ => {
                self.tiers.insert(id, tier);
            }
        }
    }

    /// Declare the area `id` stands on. Instance roots only — an area
    /// is an instance-boundary fact (§3.2), and validation holds that
    /// line so `side_of`'s successor cannot drift back into per-view
    /// special cases.
    pub fn in_area(&mut self, id: RouteId, area: Area) -> &mut Self {
        match self.areas.get(&id) {
            Some(prev) if *prev != area => {
                self.redeclared.insert((id, "area"));
            }
            _ => {
                self.areas.insert(id, area);
            }
        }
        self
    }

    /// The area an instance root stands on when it declares none.
    ///
    /// The catch-all is deliberate, inherited from the `side_of` this
    /// attribute replaces: a new root that forgets to declare itself
    /// must land somewhere *chosen* — untold_lore chose the menu, where
    /// a wrong answer is a wrong picture rather than a wrong sound over
    /// a live world. Declaring any area without declaring this is a
    /// startup error ([`TableError::NoDefaultArea`]), because a
    /// catch-all nobody picked is not a catch-all, it is luck. One per
    /// table; a later declaration replaces the earlier.
    pub fn default_area(&mut self, area: Area) -> &mut Self {
        self.default_area = Some(area);
        self
    }

    /// Declare what `id` hides beneath it, as node data.
    ///
    /// The successor to the `occludes()` method (§6.2: occlusion is
    /// mount/node data): the walk's arithmetic reads this where
    /// declared and asks the node only where it is not, so an
    /// un-migrated consumer keeps its behavior without a table edit.
    pub fn occludes(&mut self, id: RouteId, occlusion: Occlusion) -> &mut Self {
        match self.occlusions.get(&id) {
            Some(prev) if *prev != occlusion => {
                self.redeclared.insert((id, "occlusion"));
            }
            _ => {
                self.occlusions.insert(id, occlusion);
            }
        }
        self
    }

    /// Register `id`'s guard (§3.4): the precondition ruled on when
    /// entry is *asked for*, never after the fact.
    ///
    /// One function per node — a second registration is a startup error
    /// even with the same function, because "written once" is the
    /// property, and two call sites both believing they own a door is
    /// the drift. Evaluation activates with the store (WP-2.2); the
    /// shape lands now so a game's shadow rooms table has somewhere to
    /// move.
    pub fn guard(&mut self, id: RouteId, guard: Guard) -> &mut Self {
        if self.guards.insert(id, guard).is_some() {
            self.redeclared.insert((id, "guard"));
        }
        self
    }

    /// Every registered id, in registration order.
    pub fn ids(&self) -> &[RouteId] {
        &self.ids
    }

    /// True iff `child` is registered beneath `parent`.
    pub fn is_child_of(&self, child: RouteId, parent: RouteId) -> bool {
        self.parents
            .get(child)
            .is_some_and(|ps| ps.contains(&parent))
    }

    pub fn is_root(&self, id: RouteId) -> bool {
        self.roots.contains(&id)
    }

    pub fn contains(&self, id: RouteId) -> bool {
        self.ids.contains(&id)
    }

    /// The ids registered beneath `parent`, sorted. For a workspace rail
    /// or a menu that derives itself from the table rather than repeating
    /// it — which is what stops a rail offering a destination nobody
    /// registered.
    pub fn children_of(&self, parent: RouteId) -> Vec<RouteId> {
        self.parents
            .iter()
            .filter(|(_, ps)| ps.contains(&parent))
            .map(|(child, _)| *child)
            .collect()
    }

    /// The parents `id` is registered beneath, sorted. More than one is
    /// the DAG being a DAG (§3.2): settings under the title and under
    /// pause is one node with two edges, and the screen it selects
    /// resolves against whichever instance encloses it *on the path* —
    /// [`enclosing_instance`](Self::enclosing_instance) is that
    /// question's table half; the resolution mechanism is the
    /// binding's (P3/P4).
    pub fn parents_of(&self, id: RouteId) -> Vec<RouteId> {
        self.parents
            .get(&id)
            .map(|ps| ps.iter().copied().collect())
            .unwrap_or_default()
    }

    /// `id`'s tier. Undeclared is [`Tier::View`], the common case.
    pub fn tier_of(&self, id: RouteId) -> Tier {
        self.tiers.get(&id).copied().unwrap_or_default()
    }

    /// The document `id` mounts, iff it is an instance root. What the
    /// binding mounts (§6.1) — table data, so it is answerable before
    /// any route object exists, which is when mounting happens.
    pub fn document_of(&self, id: RouteId) -> Option<&'static str> {
        match self.tier_of(id) {
            Tier::InstanceRoot { document } => Some(document),
            _ => None,
        }
    }

    /// The area `id` stands on: its declared area, else the catch-all.
    /// `None` for views and structural nodes — they stand wherever
    /// their enclosing instance does, and *which* instance encloses
    /// them is a path question ([`area_of_path`](Self::area_of_path)),
    /// not a node one.
    pub fn area_of(&self, id: RouteId) -> Option<Area> {
        match self.tier_of(id) {
            Tier::InstanceRoot { .. } => self.areas.get(&id).copied().or(self.default_area),
            _ => None,
        }
    }

    /// The deepest instance root on `path` — the instance every view on
    /// it belongs to. This is the table half of §3.2's multi-parent
    /// rule (a shared view's screen resolves against the *enclosing*
    /// instance's document); the resolution mechanism itself lands with
    /// the binding (P3/P4). `None` when nothing on the path mounts —
    /// the un-migrated table, or a structural prefix.
    pub fn enclosing_instance(&self, path: &[RouteId]) -> Option<RouteId> {
        path.iter()
            .rev()
            .copied()
            .find(|id| matches!(self.tier_of(id), Tier::InstanceRoot { .. }))
    }

    /// The area `path` stands on: the enclosing instance's, else the
    /// catch-all. `side_of`'s contract, kept whole: total whenever a
    /// default is declared — including the empty path, which is the
    /// first frame of the process.
    pub fn area_of_path(&self, path: &[RouteId]) -> Option<Area> {
        self.enclosing_instance(path)
            .and_then(|id| self.area_of(id))
            .or(self.default_area)
    }

    /// What `id` hides beneath it: its declared occlusion, or the
    /// [`Occlusion::View`] every consumer already defaults to. Node
    /// data (§6.2), answerable with no route object.
    pub fn occlusion_of(&self, id: RouteId) -> Occlusion {
        self.occlusions.get(&id).copied().unwrap_or_default()
    }

    /// `id`'s occlusion iff declared. The walk's fallback seam: where
    /// this is `None` the router still asks the node's `occludes()`
    /// method, so an un-migrated consumer keeps its behavior without a
    /// table edit; the method retires with the P4 driver.
    pub fn declared_occlusion(&self, id: RouteId) -> Option<Occlusion> {
        self.occlusions.get(&id).copied()
    }

    /// `id`'s guard, if one is registered. An unguarded door is open.
    /// `Copy`, because a guard is a plain function pointer — the
    /// framework takes it at ask-time without borrowing the table.
    pub fn guard_of(&self, id: RouteId) -> Option<Guard> {
        self.guards.get(&id).copied()
    }

    /// Check the table at startup: every edge names a registered route,
    /// no id is registered twice, no attribute names an unregistered id
    /// or contradicts itself, areas sit on instance roots and have a
    /// chosen catch-all, the graph is acyclic, and something is at the
    /// root.
    ///
    /// Every one of these would otherwise surface as a wrong screen, a
    /// wrong sound, or a hang on the frame that first walked the bad
    /// edge, which is the class of failure this whole design exists to
    /// move earlier.
    pub fn validate(&self) -> Result<(), TableError> {
        let mut seen = BTreeSet::new();
        for id in &self.ids {
            if !seen.insert(*id) {
                return Err(TableError::DuplicateId(id));
            }
        }
        for (child, parents) in &self.parents {
            for parent in parents {
                if !self.ids.contains(parent) {
                    return Err(TableError::UnknownParent { child, parent });
                }
            }
        }
        if let Some((id, attribute)) = self.redeclared.iter().next() {
            return Err(TableError::Redeclared { id, attribute });
        }
        let attributed = self
            .tiers
            .keys()
            .map(|id| (*id, "tier"))
            .chain(self.areas.keys().map(|id| (*id, "area")))
            .chain(self.occlusions.keys().map(|id| (*id, "occlusion")))
            .chain(self.guards.keys().map(|id| (*id, "guard")));
        for (id, attribute) in attributed {
            if !self.ids.contains(&id) {
                return Err(TableError::AttributeOnUnknown { id, attribute });
            }
        }
        for id in self.areas.keys() {
            if !matches!(self.tier_of(id), Tier::InstanceRoot { .. }) {
                return Err(TableError::AreaOnNonInstanceRoot(id));
            }
        }
        if !self.areas.is_empty() && self.default_area.is_none() {
            return Err(TableError::NoDefaultArea);
        }
        if self.roots.is_empty() {
            return Err(TableError::NoRoot);
        }
        self.check_acyclic()
    }

    /// Depth-first three-colour cycle check over the parent edges.
    fn check_acyclic(&self) -> Result<(), TableError> {
        #[derive(Clone, Copy, PartialEq)]
        enum Mark {
            Open,
            Done,
        }
        let mut marks: BTreeMap<RouteId, Mark> = BTreeMap::new();

        fn visit(
            id: RouteId,
            table: &RouteTable,
            marks: &mut BTreeMap<RouteId, Mark>,
        ) -> Result<(), TableError> {
            match marks.get(id) {
                Some(Mark::Done) => return Ok(()),
                Some(Mark::Open) => return Err(TableError::Cycle(id)),
                None => {}
            }
            marks.insert(id, Mark::Open);
            for child in table.children_of(id) {
                visit(child, table, marks)?;
            }
            marks.insert(id, Mark::Done);
            Ok(())
        }

        for id in &self.ids {
            visit(id, self, &mut marks)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guard::{Refusal, Store};

    /// The table from `ROUTING.md` §4: four edges, three real paths, one
    /// handler for settings.
    fn settings_table() -> RouteTable {
        let mut t = RouteTable::new();
        t.at_root("title")
            .at_root("world")
            .at_root("library")
            .under("settings", "title")
            .under("settings", "pause")
            .under("pause", "world")
            .under("pause", "library");
        t
    }

    #[test]
    fn one_handler_registered_under_two_parents_is_one_route() {
        let t = settings_table();
        assert_eq!(t.ids().iter().filter(|id| **id == "settings").count(), 1);
        assert!(t.is_child_of("settings", "title"));
        assert!(t.is_child_of("settings", "pause"));
        assert!(!t.is_child_of("settings", "world"));
    }

    #[test]
    fn a_valid_table_validates() {
        settings_table()
            .validate()
            .expect("this table is well formed");
    }

    #[test]
    fn an_edge_to_an_unregistered_parent_fails_at_startup() {
        let mut t = RouteTable::new();
        t.at_root("title").under("settings", "puase");
        assert_eq!(
            t.validate(),
            Err(TableError::UnknownParent {
                child: "settings",
                parent: "puase"
            })
        );
    }

    #[test]
    fn an_empty_table_fails_at_startup_rather_than_rendering_nothing() {
        assert_eq!(RouteTable::new().validate(), Err(TableError::NoRoot));
    }

    #[test]
    fn an_edge_does_not_declare_its_parent() {
        // Deliberate: if `under` registered the parent too, a typo'd
        // parent would quietly become a real route with no handler, and
        // the first path through it would be the diagnostic. Requiring
        // the parent to exist already is what turns that into
        // `UnknownParent` at startup.
        let mut t = RouteTable::new();
        t.under("settings", "title");
        assert_eq!(
            t.validate(),
            Err(TableError::UnknownParent {
                child: "settings",
                parent: "title"
            })
        );
    }

    #[test]
    fn a_cycle_fails_at_startup_rather_than_hanging_a_frame() {
        let mut t = RouteTable::new();
        t.at_root("a").under("b", "a").under("a", "b");
        assert!(matches!(t.validate(), Err(TableError::Cycle(_))));
    }

    #[test]
    fn children_are_derived_so_a_rail_cannot_offer_an_unregistered_place() {
        let t = settings_table();
        assert_eq!(t.children_of("world"), vec!["pause"]);
        assert_eq!(t.children_of("pause"), vec!["settings"]);
        assert!(t.children_of("settings").is_empty());
    }

    /// The celia shape from `APPLICATION_BUILD.md` A.1: a structural
    /// session over two instance roots, views beneath them.
    fn celia_table() -> RouteTable {
        let mut t = RouteTable::new();
        t.at_root("session")
            .under("lobby", "session")
            .under("arena", "session")
            .under("pause", "lobby")
            .under("pause", "arena");
        t.structural("session")
            .mounts("lobby", "data/ui/lobby.ogh")
            .mounts("arena", "data/ui/arena.ogh");
        t
    }

    /// The untold_lore shape from A.3, replacing `crossing::side_of`:
    /// four instance roots, three areas, roster and world sharing one —
    /// and the title declaring nothing, on purpose, to stand on the
    /// catch-all.
    fn untold_table() -> RouteTable {
        let mut t = RouteTable::new();
        t.at_root("title")
            .at_root("roster")
            .at_root("world")
            .at_root("library");
        t.mounts("title", "data/ui/title.ogh")
            .mounts("roster", "data/ui/roster.ogh")
            .mounts("world", "data/ui/world.ogh")
            .mounts("library", "data/ui/library.ogh");
        t.default_area("menu")
            .in_area("roster", "world")
            .in_area("world", "world")
            .in_area("library", "library");
        t
    }

    #[test]
    fn a_tier_is_table_data_answered_with_no_route_object() {
        let t = celia_table();
        t.validate().expect("this table is well formed");
        assert_eq!(t.tier_of("session"), Tier::Structural);
        assert_eq!(
            t.tier_of("lobby"),
            Tier::InstanceRoot {
                document: "data/ui/lobby.ogh"
            }
        );
        assert_eq!(
            t.tier_of("pause"),
            Tier::View,
            "undeclared is the common case"
        );
        assert_eq!(t.document_of("arena"), Some("data/ui/arena.ogh"));
        assert_eq!(t.document_of("session"), None, "structural mounts nothing");
        assert_eq!(t.document_of("pause"), None);
    }

    #[test]
    fn four_roots_three_areas_and_the_forgetful_one_lands_on_the_default() {
        let t = untold_table();
        t.validate().expect("this table is well formed");
        assert_eq!(t.area_of("roster"), Some("world"));
        assert_eq!(t.area_of("world"), Some("world"));
        assert_eq!(t.area_of("library"), Some("library"));
        // The `side_of` property, kept: a root that declares nothing
        // lands on the chosen catch-all, with nobody remembering it.
        assert_eq!(t.area_of("title"), Some("menu"));
    }

    #[test]
    fn a_path_stands_on_its_enclosing_instances_area_and_the_empty_path_on_the_default() {
        let t = untold_table();
        assert_eq!(t.area_of_path(&["world"]), Some("world"));
        assert_eq!(t.area_of_path(&["title"]), Some("menu"));
        assert_eq!(
            t.area_of_path(&[]),
            Some("menu"),
            "the first frame of the process is the front"
        );
    }

    #[test]
    fn a_view_belongs_to_the_deepest_instance_root_above_it() {
        let t = celia_table();
        assert_eq!(
            t.enclosing_instance(&["session", "lobby", "pause"]),
            Some("lobby")
        );
        assert_eq!(
            t.enclosing_instance(&["session", "arena", "pause"]),
            Some("arena")
        );
        assert_eq!(
            t.enclosing_instance(&["session"]),
            None,
            "a structural prefix mounts nothing"
        );
    }

    #[test]
    fn areas_without_a_chosen_catch_all_fail_at_startup() {
        let mut t = RouteTable::new();
        t.at_root("world").mounts("world", "data/ui/world.ogh");
        t.in_area("world", "world");
        assert_eq!(t.validate(), Err(TableError::NoDefaultArea));
    }

    #[test]
    fn an_area_on_a_view_fails_at_startup() {
        let mut t = celia_table();
        t.default_area("menu").in_area("pause", "menu");
        assert_eq!(
            t.validate(),
            Err(TableError::AreaOnNonInstanceRoot("pause"))
        );
    }

    #[test]
    fn occlusion_is_declared_once_and_read_from_the_table() {
        let mut t = RouteTable::new();
        t.at_root("workspace").under("prompt", "workspace");
        t.occludes("prompt", Occlusion::None);
        t.validate().expect("this table is well formed");
        assert_eq!(t.occlusion_of("prompt"), Occlusion::None);
        assert_eq!(t.declared_occlusion("prompt"), Some(Occlusion::None));
        // Undeclared: the effective answer is the default every consumer
        // already uses; the *declared* answer is `None`, which is the
        // seam the router's method fallback keys on.
        assert_eq!(t.occlusion_of("workspace"), Occlusion::View);
        assert_eq!(t.declared_occlusion("workspace"), None);
    }

    #[test]
    fn a_multi_parent_node_answers_every_parent_it_stands_under() {
        let t = settings_table();
        assert_eq!(t.parents_of("settings"), vec!["pause", "title"]);
        assert!(
            t.parents_of("title").is_empty(),
            "a root stands under nobody"
        );
    }

    fn needs_two_players(_store: &Store) -> Result<(), Refusal> {
        Err(Refusal::new("Needs two more players."))
    }

    #[test]
    fn a_guard_registers_with_its_node_and_refuses_in_one_sentence() {
        let mut t = celia_table();
        t.guard("arena", needs_two_players);
        t.validate().expect("this table is well formed");
        assert!(t.guard_of("lobby").is_none(), "an unguarded door is open");
        let guard = t.guard_of("arena").expect("registered with its node");
        let refusal = guard(&Store::new()).unwrap_err();
        assert_eq!(refusal.sentence(), "Needs two more players.");
        assert_eq!(
            refusal.to_string(),
            refusal.sentence(),
            "every surface shows exactly the authored sentence"
        );
    }

    #[test]
    fn a_second_guard_on_one_door_fails_at_startup() {
        let mut t = celia_table();
        t.guard("arena", needs_two_players)
            .guard("arena", needs_two_players);
        assert_eq!(
            t.validate(),
            Err(TableError::Redeclared {
                id: "arena",
                attribute: "guard"
            })
        );
    }

    #[test]
    fn an_attribute_on_an_unregistered_id_fails_at_startup() {
        // The typo protection `under` already has, applied to tiers: a
        // misspelt id must not quietly become a node.
        let mut t = RouteTable::new();
        t.at_root("title").mounts("titel", "data/ui/title.ogh");
        assert_eq!(
            t.validate(),
            Err(TableError::AttributeOnUnknown {
                id: "titel",
                attribute: "tier"
            })
        );
    }

    #[test]
    fn a_node_cannot_be_an_instance_root_and_structural_at_once() {
        let mut t = RouteTable::new();
        t.at_root("session")
            .structural("session")
            .mounts("session", "data/ui/session.ogh");
        assert_eq!(
            t.validate(),
            Err(TableError::Redeclared {
                id: "session",
                attribute: "tier"
            })
        );
    }
}
