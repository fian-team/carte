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

use std::collections::{BTreeMap, BTreeSet};

use crate::RouteId;

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

    /// Check the table at startup: every edge names a registered route,
    /// no id is registered twice, the graph is acyclic, and something is
    /// at the root.
    ///
    /// Every one of these would otherwise surface as a wrong screen or a
    /// hang on the frame that first walked the bad edge, which is the
    /// class of failure this whole design exists to move earlier.
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
}
