//! The two grades, held apart.
//!
//! The fixtures are celia's, from `APPLICATION_BUILD.md` A.1 and B.3,
//! because celia is the consumer whose live drift §4.1 names: a root
//! `status` the host computes every frame and nothing reads, and an arena
//! `status` selected from a scope that never had one. Both appear below
//! under their own names.
//!
//! The [`Schema`] and [`Intents`] impls are written out by hand for the
//! reason the store's and the intent module's are: this crate depends on
//! nothing and so cannot reach its own derive.

use super::*;
use crate::intent::{Accepted, Intents, Parameter, Raise, Refused, Vocabulary};
use crate::schema::{Difference, Field, Kind, Lit, Schema};
use crate::store::Store;

// --- the fixtures ----------------------------------------------------------

/// The engine's front-of-house scope, as P5 will publish it: a heading, and
/// the connect status celia's document declares and never reads.
#[derive(Clone, Debug, Default, PartialEq)]
struct Front {
    heading: String,
    status: String,
}

impl Schema for Front {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("heading", Kind::Str),
            Field::new("status", Kind::Str).absent_when("no connection is being made"),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Front")
    }
}

/// The lobby view's scope, flat rather than nested (B.3's record-grain
/// lesson).
#[derive(Clone, Debug, Default, PartialEq)]
struct Lobby {
    pane: String,
    ready: bool,
    seats: i64,
}

impl Schema for Lobby {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("pane", Kind::Str),
            Field::new("ready", Kind::Bool),
            Field::new("seats", Kind::Int),
        ])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Lobby")
    }
}

/// The lobby's intents. `pick` and `confirm` are raised by the shipped
/// document; `withdraw` is the one a provider publishes that no shipped
/// document uses — §4.1's licensed modding surface.
#[derive(Clone, Debug, PartialEq)]
enum Muster {
    Pick { character: String },
    Confirm,
    Withdraw,
}

impl Intents for Muster {
    fn vocabulary() -> Vocabulary {
        Vocabulary::new(vec![
            Accepted::new("pick", vec![Parameter::of::<String>("character")]),
            Accepted::new("confirm", vec![]),
            Accepted::new("withdraw", vec![]),
        ])
    }

    fn accept(raise: &Raise) -> Result<Self, Refused> {
        match raise.name() {
            "pick" => {
                let mut p = raise.parameters("pick", 1)?;
                Ok(Muster::Pick {
                    character: p.take::<String>("character")?,
                })
            }
            "confirm" => {
                raise.parameters("confirm", 0)?;
                Ok(Muster::Confirm)
            }
            "withdraw" => {
                raise.parameters("withdraw", 0)?;
                Ok(Muster::Withdraw)
            }
            other => Err(Refused::NoSuchIntent {
                intent: other.to_string(),
            }),
        }
    }
}

/// The arena's scope, B.3's "empty or deletes" — a countdown and nothing
/// else. It is the scope celia's `screen "arena" { state { status: string } }`
/// selects against once the block inverts, and it has never had a `status`.
#[derive(Clone, Debug, Default, PartialEq)]
struct Arena {
    countdown: i64,
}

impl Schema for Arena {
    fn reflect() -> Kind {
        Kind::Record(vec![Field::new("countdown", Kind::Int)])
    }
    fn at_mount(_: Option<&Lit>) -> Self {
        Self::default()
    }
    fn type_name() -> Option<&'static str> {
        Some("Arena")
    }
}

const FRONT: Scope = Scope::Process;
const LOBBY: Scope = Scope::Node("lobby");

/// The store as a consumer's registration function leaves it: two scopes
/// provided, one of them accepting intents. No path, no mount, no frame —
/// which is what makes the whole check answerable under `cargo test`.
fn store() -> Store {
    let mut store = Store::new();
    store.provides::<Front>(FRONT).expect("the process scope");
    store.provides::<Lobby>(LOBBY).expect("the lobby's scope");
    store.accepts::<Muster>(LOBBY).expect("the lobby's intents");
    store
}

/// What celia's `lobby.ogh` selects once its `host_state {}` has inverted:
/// the heading from the front-of-house rung, its own three fields from the
/// lobby's.
fn lobby_selection() -> Vec<Field> {
    vec![
        Field::new("heading", Kind::Str),
        Field::new("pane", Kind::Str),
        Field::new("ready", Kind::Bool),
        Field::new("seats", Kind::Int),
    ]
}

fn raised() -> Vec<Declared> {
    vec![
        Declared::new("pick", vec![Kind::Str]),
        Declared::new("confirm", vec![]),
    ]
}

// --- the refusing grade ----------------------------------------------------

/// The modder's case: a selection naming a field that does not exist. Loud,
/// immediate, and it names the field.
#[test]
fn a_selection_naming_a_field_nothing_provides_refuses() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects(
        "lobby.ogh",
        &[LOBBY, FRONT],
        &[
            Field::new("pane", Kind::Str),
            Field::new("stance", Kind::Str),
        ],
    );
    let found = check.finish();

    assert!(found.refuses());
    let refusal = found
        .refusals()
        .find(|f| matches!(f, Finding::Unprovided { field, .. } if field == "stance"))
        .expect("the missing field is named");
    let printed = refusal.to_string();
    assert!(printed.contains("stance"), "{printed}");
    assert!(printed.contains("lobby.ogh"), "{printed}");
}

/// A field both sides have, at two different shapes. Structural, so what
/// the refusal names is the dotted path down to where they stopped
/// agreeing — never a type name (§4.7).
#[test]
fn a_selection_at_another_shape_refuses_and_names_the_path() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects("lobby.ogh", &[LOBBY], &[Field::new("seats", Kind::Str)]);
    let found = check.finish();

    let Some(Finding::Shape { at, .. }) = found.refusals().next() else {
        panic!("a shape refusal: {found}");
    };
    assert_eq!(at.field(), "seats");
    assert_eq!(
        at.difference(),
        &Difference::Kind {
            want: "str",
            got: "int"
        }
    );
}

/// celia's arena `status`: a screen's `state {}` block selecting a field
/// the scope it mounts under never had. Today the read is silently empty
/// and the screen simply shows nothing; here it refuses at load, naming
/// the field and the scopes that were asked.
#[test]
fn the_arena_status_celia_never_provided_refuses() {
    let mut store = store();
    store
        .provides::<Arena>(Scope::Node("arena"))
        .expect("the arena's scope");
    let mut check = Validation::new(&store);
    check.selects(
        "arena.ogh",
        &[Scope::Node("arena")],
        &[Field::new("status", Kind::Str)],
    );
    let found = check.finish();

    assert!(found.refuses(), "{found}");
    let refusal = found
        .refusals()
        .find(|f| matches!(f, Finding::Unprovided { field, .. } if field == "status"))
        .expect("the field is named");
    let printed = refusal.to_string();
    assert!(printed.contains("arena.ogh"), "{printed}");
    assert!(printed.contains("`arena`'s scope"), "{printed}");
}

/// A mount naming a scope nothing publishes. The mapping itself is wrong,
/// so it refuses rather than letting every selection against it be refused
/// for the wrong reason.
#[test]
fn a_mount_naming_a_scope_nothing_publishes_refuses() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects("lobby.ogh", &[Scope::Node("lobbby")], &[]);
    let found = check.finish();

    assert!(found.refuses());
    assert!(found.to_string().contains("lobbby"), "{found}");
}

/// The write side of the modder's case: a raise no scope accepts. celia's
/// Back button was exactly this and nothing anywhere said so.
#[test]
fn a_raise_nothing_accepts_refuses_and_names_the_intent() {
    let store = store();
    let mut check = Validation::new(&store);
    check.raises("lobby.ogh", &[LOBBY], &[Declared::new("back", vec![])]);
    let found = check.finish();

    assert!(found.refuses());
    assert!(
        found
            .refusals()
            .any(|f| matches!(f, Finding::Unaccepted { intent, .. } if intent == "back")),
        "{found}"
    );
}

/// An accepted intent raised with the wrong argument shape. The grade
/// travels from [`Drift::refuses`] unchanged rather than being re-decided
/// here.
#[test]
fn a_raise_at_the_wrong_shape_refuses_through_the_drift_it_carries() {
    let store = store();
    let mut check = Validation::new(&store);
    check.raises(
        "lobby.ogh",
        &[LOBBY],
        &[Declared::new("pick", vec![Kind::Int])],
    );
    let found = check.finish();

    let Some(Finding::Raise { at, .. }) = found.refusals().next() else {
        panic!("a parameter refusal: {found}");
    };
    assert_eq!(at.intent(), "pick");
    assert!(at.refuses());
    assert!(at.to_string().contains("character"), "{at}");
}

// --- the reporting grade ---------------------------------------------------

/// celia's dead root `status`: provided, computed every frame, and read by
/// nothing. It reports — a provider is allowed to publish more than the
/// shipped documents use — and it names the scope and the field.
#[test]
fn a_field_provided_and_read_by_nothing_reports() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects("lobby.ogh", &[LOBBY, FRONT], &lobby_selection());
    let found = check.finish();

    assert!(!found.refuses(), "an unread field must not refuse: {found}");
    let report = found
        .reports()
        .find(|f| matches!(f, Finding::Unread { field, .. } if field == "status"))
        .expect("the unread field is named");
    assert!(report.to_string().contains("status"), "{report}");
}

/// A field one document selects and another does not is read. The unread
/// direction is a question about the shipped set, which is why
/// [`Validation`] accumulates instead of answering per document.
#[test]
fn a_field_only_one_shipped_document_selects_is_read() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects("menu.ogh", &[FRONT], &[Field::new("heading", Kind::Str)]);
    check.selects("connect.ogh", &[FRONT], &[Field::new("status", Kind::Str)]);
    let found = check.finish();

    assert!(
        !found
            .all()
            .iter()
            .any(|f| matches!(f, Finding::Unread { .. })),
        "between them the two documents read everything: {found}"
    );
}

/// An intent a provider publishes that no shipped document raises. §4.1
/// says this in as many words: in a modding world that is the surface
/// working as intended, so it reports.
#[test]
fn an_intent_no_shipped_document_raises_reports() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects("lobby.ogh", &[LOBBY, FRONT], &lobby_selection());
    check.raises("lobby.ogh", &[LOBBY], &raised());
    let found = check.finish();

    assert!(!found.refuses(), "{found}");
    assert!(
        found
            .reports()
            .any(|f| matches!(f, Finding::Unraised { intent, .. } if intent == "withdraw")),
        "{found}"
    );
}

/// Table-coverage drift, both ways: a registered id nothing draws, and a
/// screen no node reaches. Reports — a game that would not boot over a
/// screen nobody has routed yet is a game whose check gets deleted rather
/// than fixed.
#[test]
fn a_screen_and_a_route_id_that_disagree_report_without_refusing() {
    let store = store();
    let mut check = Validation::new(&store);
    check.draws("menu.ogh", &["title", "credits"], &["title", "settings"]);
    let found = check.finish();

    assert!(!found.refuses(), "{found}");
    let printed = found.to_string();
    assert!(printed.contains("credits"), "{printed}");
    assert!(printed.contains("settings"), "{printed}");
}

/// Two scopes in one mount providing one name. Both are legitimate —
/// untold_lore's app-global `heading` and the pause scope's are the live
/// pair — so the nearer one wins and the ambiguity reports, against the day
/// §4.6's binding syntax lets the document say which it meant.
#[test]
fn a_field_two_scopes_provide_reports_which_one_won() {
    let mut store = store();
    store
        .provides::<Front>(Scope::Node("pause"))
        .expect("the pause scope");
    let mut check = Validation::new(&store);
    check.selects(
        "lobby.ogh",
        &[Scope::Node("pause"), FRONT],
        &[Field::new("heading", Kind::Str)],
    );
    let found = check.finish();

    assert!(!found.refuses(), "{found}");
    let Some(Finding::Shadowed { scope, by, .. }) = found
        .reports()
        .find(|f| matches!(f, Finding::Shadowed { .. }))
    else {
        panic!("the shadowing is reported: {found}");
    };
    assert_eq!(*scope, Scope::Node("pause"), "the nearer scope wins");
    assert_eq!(*by, FRONT);
}

// --- the two grades together -----------------------------------------------

/// A document and a store that agree about everything, coverage included.
/// The check that can pass is the one worth running.
#[test]
fn a_document_that_agrees_with_the_store_finds_nothing() {
    let mut store = Store::new();
    store.provides::<Lobby>(LOBBY).expect("the lobby's scope");
    store.accepts::<Muster>(LOBBY).expect("the lobby's intents");
    let mut check = Validation::new(&store);
    check.selects(
        "lobby.ogh",
        &[LOBBY],
        &[
            Field::new("pane", Kind::Str),
            Field::new("ready", Kind::Bool),
            Field::new("seats", Kind::Int),
        ],
    );
    check.raises(
        "lobby.ogh",
        &[LOBBY],
        &[
            Declared::new("pick", vec![Kind::Str]),
            Declared::new("confirm", vec![]),
            Declared::new("withdraw", vec![]),
        ],
    );
    check.draws("lobby.ogh", &["lobby"], &["lobby"]);
    let found = check.finish();

    assert!(found.is_empty(), "{found}");
    assert_eq!(found.to_string(), "the documents and the store agree");
}

/// The two grades travel together: one run refuses the stale selection
/// *and* reports the drift, because a caller that only ever saw the
/// refusals would never fix the second.
#[test]
fn one_run_refuses_and_reports_at_once() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects(
        "lobby.ogh",
        &[LOBBY, FRONT],
        &[
            Field::new("heading", Kind::Str),
            Field::new("stance", Kind::Str),
        ],
    );
    let found = check.finish();

    assert!(found.refuses());
    assert!(found.reports().count() > 0, "{found}");
    let printed = found.to_string();
    assert!(printed.contains("refuses:"), "{printed}");
    assert!(printed.contains("reports:"), "{printed}");
}

// --- a selection that names fields and nothing else (§4.6) -----------------

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

/// The same four fields celia's `lobby.ogh` reads, selected by name.
/// Nothing about their shapes is stated, so nothing about their shapes can
/// disagree — and the check is otherwise the one the declared form gets.
#[test]
fn a_selection_that_names_only_fields_is_checked_the_same_way() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects_named(
        "lobby.ogh",
        &[LOBBY, FRONT],
        &names(&["heading", "pane", "ready", "seats"]),
    );
    let found = check.finish();

    assert!(!found.refuses(), "{found}");
    assert!(
        !found
            .reports()
            .any(|f| matches!(f, Finding::Unread { field, .. } if field == "pane")),
        "a selected field is read: {found}"
    );
}

/// The refusal a selection is *for*: a name nothing on the mount provides.
#[test]
fn a_named_selection_naming_a_field_nothing_provides_refuses() {
    let store = store();
    let mut check = Validation::new(&store);
    check.selects_named("arena.ogh", &[LOBBY, FRONT], &names(&["pane", "status"]));
    let found = check.finish();

    // `status` is the front rung's, so it resolves; `stance` is nobody's.
    let mut check = Validation::new(&store);
    check.selects_named("arena.ogh", &[LOBBY], &names(&["pane", "stance"]));
    let refused = check.finish();
    assert!(!found.refuses(), "{found}");
    assert!(refused.refuses(), "{refused}");
    assert!(
        refused
            .refusals()
            .any(|f| matches!(f, Finding::Unprovided { field, .. } if field == "stance")),
        "{refused}"
    );
}

/// A selection cannot disagree about a shape, because it never states one.
///
/// This is the property that deletes regency's 254-line record block: the
/// document stops carrying a second copy of the provider's shapes, so the
/// two copies can no longer drift apart.
#[test]
fn a_named_selection_has_no_shape_to_disagree_about() {
    let store = store();
    let mut check = Validation::new(&store);
    // `seats` is an Int; the declared form would have to say so, and would
    // refuse if it said `string`.
    check.selects("declared.ogh", &[LOBBY], &[Field::new("seats", Kind::Str)]);
    assert!(check.finish().refuses());

    let mut check = Validation::new(&store);
    check.selects_named("selected.ogh", &[LOBBY], &names(&["seats"]));
    assert!(!check.finish().refuses());
}

/// Two scopes providing one name still reports, whichever form asked —
/// §4.6's binding is what a *document* uses to settle it, by naming the
/// scope it meant.
#[test]
fn a_named_selection_reports_a_field_two_scopes_provide() {
    let mut store = store();
    store
        .provides::<Front>(Scope::Node("pause"))
        .expect("the pause scope");
    let mut check = Validation::new(&store);
    check.selects_named(
        "pause.ogh",
        &[Scope::Node("pause"), FRONT],
        &names(&["heading"]),
    );
    let found = check.finish();

    assert!(!found.refuses(), "{found}");
    assert!(
        found
            .reports()
            .any(|f| matches!(f, Finding::Shadowed { field, .. } if field == "heading")),
        "{found}"
    );

    // And naming the scope settles it: one scope, one provider, no report.
    let mut check = Validation::new(&store);
    check.selects_named("pause.ogh", &[Scope::Node("pause")], &names(&["heading"]));
    assert!(
        !check
            .finish()
            .all()
            .iter()
            .any(|f| matches!(f, Finding::Shadowed { .. })),
        "naming the scope is how the document says which one it meant"
    );
}
