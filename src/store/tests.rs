//! The store's own tests. The scope shapes are celia's, from
//! `APPLICATION_BUILD.md` A.1 and B.3, because that is the consumer whose
//! same-tick echo §5.4 names as the regression test.
//!
//! The two acceptance criteria that are the *walk*'s — a scope dying with
//! its node, and a guarded door — live with the walk, in
//! [`crate::router`]'s tests.

use super::*;
use crate::schema::{Field, Kind};
use crate::{set, set_num};

// --- the fixtures ----------------------------------------------------------

/// The structural session scope: the facts that must outlive both the
/// lobby and the arena (§3.2).
#[derive(Clone, Debug, Default, PartialEq)]
struct Session {
    focused: String,
    seats: i64,
    stage: String,
}

impl Schema for Session {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("focused", Kind::Str),
            Field::new("seats", Kind::Int),
            Field::new("stage", Kind::Str).starting_at(crate::Lit::Str("lobby".into())),
        ])
    }
    fn type_name() -> Option<&'static str> {
        Some("Session")
    }
}

/// The lobby view's scope, flat rather than nested — B.3's record-grain
/// lesson: one nested `view` field would forfeit per-field invalidation.
#[derive(Clone, Debug, Default, PartialEq)]
struct Lobby {
    pane: String,
    ready: bool,
}

impl Schema for Lobby {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("pane", Kind::Str),
            Field::new("ready", Kind::Bool),
        ])
    }
    fn type_name() -> Option<&'static str> {
        Some("Lobby")
    }
}

/// The app-global rung: the fields untold_lore's B.1 puts there, including
/// the one whose implied zero was the `launch_fade` lesson.
#[derive(Clone, Debug, PartialEq)]
struct App {
    launch_fade: f64,
    heading: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            launch_fade: 1.0,
            heading: String::new(),
        }
    }
}

impl Schema for App {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("launch_fade", Kind::Float).starting_at(crate::Lit::Float(1.0)),
            Field::new("heading", Kind::Str),
        ])
    }
    fn type_name() -> Option<&'static str> {
        Some("App")
    }
}

/// The sky's hour, whose grain is a minute of a day (§5.5's own example).
#[derive(Clone, Debug, Default, PartialEq)]
struct Sky {
    hour: f64,
}

const MINUTE: f64 = 1.0 / 1440.0;

impl Schema for Sky {
    fn reflect() -> Kind {
        Kind::Record(vec![Field::new("hour", Kind::Float).at_grain(MINUTE)])
    }
    fn type_name() -> Option<&'static str> {
        Some("Sky")
    }
}

/// Ten fields, for the ten-sets-one-notification sentence.
#[derive(Clone, Debug, Default, PartialEq)]
struct Bars {
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    e: i64,
    f: i64,
    g: i64,
    h: i64,
    i: i64,
    j: i64,
}

const BAR_NAMES: [&str; 10] = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];

impl Schema for Bars {
    fn reflect() -> Kind {
        Kind::Record(
            BAR_NAMES
                .iter()
                .map(|name| Field::new(*name, Kind::Int))
                .collect(),
        )
    }
    fn type_name() -> Option<&'static str> {
        Some("Bars")
    }
}

const SESSION: Scope = Scope::Node("session");
const LOBBY: Scope = Scope::Node("lobby");

/// A store standing where the walk would have left it: the session and the
/// lobby both on the path.
fn seated() -> Store {
    let mut store = Store::new();
    store.provides(SESSION, Session::default()).unwrap();
    store.provides(LOBBY, Lobby::default()).unwrap();
    store.mount("session");
    store.mount("lobby");
    store
}

/// An empty outbox, for a tick with no intent behind it.
fn quiet() -> Outbox<()> {
    Outbox::new()
}

// --- the five acceptance criteria (the store's three) ----------------------

/// **The FRP glitch.** A consumer looking during the barrier — after one
/// field is set and before the other is — sees the *whole* of last tick and
/// never a mixture. Producers read working state; consumers read committed
/// state; the two never meet mid-tick.
#[test]
fn no_consumer_sees_one_field_new_and_another_old() {
    let mut store = seated();
    let session = store
        .producer::<Session>(SESSION, &["focused", "seats"])
        .unwrap();
    let mut seen = Vec::new();

    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&session).unwrap();
        set!(w, focused, "tohri".to_string()).unwrap();
        // Between the two sets. This is the one place a glitch could be
        // observed, and what is observable here is last tick, entire.
        let mid = b.committed::<Session>(SESSION).unwrap();
        seen.push((mid.focused.clone(), mid.seats));
        let mut w = b.writer(&session).unwrap();
        set!(w, seats, 4).unwrap();
    });

    let after = store.read::<Session>(SESSION).unwrap();
    seen.push((after.focused.clone(), after.seats));
    assert_eq!(
        seen,
        vec![(String::new(), 0), ("tohri".to_string(), 4)],
        "the only observable states are all-old and all-new"
    );
}

/// **Ten sets, one notification.** One tick, ten fields moved, one
/// subscriber watching all ten: it is woken once, and the notice still says
/// which ten moved for whoever wants to know.
#[test]
fn ten_sets_in_one_tick_wake_a_subscriber_once() {
    let mut store = Store::new();
    store.provides(LOBBY, Bars::default()).unwrap();
    store.mount("lobby");
    let bars = store.producer::<Bars>(LOBBY, &BAR_NAMES).unwrap();
    let who = store.subscriber();
    for name in BAR_NAMES {
        store.subscribe(who, LOBBY, name).unwrap();
    }

    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&bars).unwrap();
        set!(w, a, 1).unwrap();
        set!(w, b, 2).unwrap();
        set!(w, c, 3).unwrap();
        set!(w, d, 4).unwrap();
        set!(w, e, 5).unwrap();
        set!(w, f, 6).unwrap();
        set!(w, g, 7).unwrap();
        set!(w, h, 8).unwrap();
        set!(w, i, 9).unwrap();
        set!(w, j, 10).unwrap();
    });

    assert_eq!(notice.woken(), &[who], "ten sets are one rerender, not ten");
    assert_eq!(notice.changed().len(), 10);
    assert_eq!(store.read::<Bars>(LOBBY).unwrap().j, 10);
    assert_eq!(
        store.version(LOBBY),
        store.version(LOBBY),
        "one commit, one version"
    );
}

/// **The same-tick echo** (§5.4, celia's tile focus). A `pick` raised this
/// tick reaches the outbox; the tick drains it, the host's producer sets
/// the session's focus, a derived producer (§5.7) reads that *working*
/// value and projects the lobby's pane from it — and all of it commits in
/// this tick. The frame that saw the click shows the focus.
///
/// This is what deletes the local patch in celia's `pick` handler, which
/// writes `state.view.pane.id` by hand beside the action precisely because
/// the action would otherwise land a frame late.
#[test]
fn a_pick_this_tick_focuses_this_tick() {
    #[derive(Debug)]
    enum Act {
        Focus(String),
    }

    let mut store = seated();
    let session = store.producer::<Session>(SESSION, &["focused"]).unwrap();
    let lobby = store.producer::<Lobby>(LOBBY, &["pane"]).unwrap();
    let watching_the_pane = store.subscriber();
    store.subscribe(watching_the_pane, LOBBY, "pane").unwrap();

    // The click happened: the route raised, and the raise is on the outbox
    // before the tick begins.
    let mut outbox: Outbox<Act> = Outbox::new();
    outbox.push(Act::Focus("tohri".to_string()));

    let notice = store.tick(&mut outbox, |actions, b| {
        // The host's services apply what the intents asked for…
        let mut w = b.writer(&session).unwrap();
        for action in actions {
            let Act::Focus(id) = action;
            set!(w, focused, id.clone()).unwrap();
        }
        // …and the derivation reads working state, in the same barrier.
        let focused = b.working::<Session>(SESSION).unwrap().focused.clone();
        let mut w = b.writer(&lobby).unwrap();
        set!(w, pane, focused).unwrap();
    });

    assert_eq!(
        store.read::<Lobby>(LOBBY).unwrap().pane,
        "tohri",
        "the echo of a click costs zero frames"
    );
    assert!(notice.woke(watching_the_pane));
    assert!(outbox.is_empty(), "the tick drained it");
}

/// **A raise this tick commits this tick.** The same sentence as the test
/// above, entered through the door §4.4 opens: the click arrives as a named
/// raise, the scope's published vocabulary decides what it means, and the
/// typed intent lands on the outbox — which is the *only* place it can land,
/// so §5.4's pinned order carries it whether the host remembered the order
/// or not.
#[test]
fn a_raise_landing_this_tick_commits_this_tick() {
    /// The lobby's write side, transcribed as `#[derive(Intent)]` emits it.
    #[derive(Debug, PartialEq)]
    enum Pick {
        Focus { who: String },
    }

    impl crate::Intents for Pick {
        fn vocabulary() -> crate::Vocabulary {
            crate::Vocabulary::new(vec![crate::Accepted::new(
                "focus",
                vec![crate::Parameter::of::<String>("who")],
            )])
        }
        fn accept(raise: &crate::Raise) -> Result<Self, crate::Refused> {
            match raise.name() {
                "focus" => {
                    let mut p = raise.parameters("focus", 1)?;
                    Ok(Pick::Focus {
                        who: p.take::<String>("who")?,
                    })
                }
                other => Err(crate::Refused::NoSuchIntent {
                    intent: other.to_string(),
                }),
            }
        }
    }

    let mut store = seated();
    store.accepts::<Pick>(LOBBY).unwrap();
    let session = store.producer::<Session>(SESSION, &["focused"]).unwrap();

    // The document raised; nothing has ticked yet.
    let mut outbox: Outbox<Pick> = Outbox::new();
    store
        .raise::<Pick, Pick>(
            LOBBY,
            &crate::Raise::new("focus", vec![crate::RaiseArg::Str("tohri".into())]),
            &mut outbox,
        )
        .unwrap();

    store.tick(&mut outbox, |actions, b| {
        let mut w = b.writer(&session).unwrap();
        for Pick::Focus { who } in actions {
            set!(w, focused, who.clone()).unwrap();
        }
    });

    assert_eq!(
        store.read::<Session>(SESSION).unwrap().focused,
        "tohri",
        "the raise was drained ahead of the producers in its own tick"
    );
    assert!(outbox.is_empty(), "the tick drained it");
}

/// **An out-of-scope set is an error.** The arena's producer exists, its
/// node does not stand on the path, and the writing surface refuses to open
/// — once, naming the scope, rather than silently accepting facts about a
/// screen nobody is looking at.
#[test]
fn a_set_into_a_scope_that_is_not_on_the_path_is_an_error() {
    let mut store = Store::new();
    store.provides(LOBBY, Lobby::default()).unwrap();
    let lobby = store.producer::<Lobby>(LOBBY, &["pane"]).unwrap();

    let mut refused = None;
    store.tick(&mut quiet(), |_, b| {
        refused = b.writer(&lobby).err();
    });
    assert_eq!(refused, Some(StoreError::NotMounted(LOBBY)));

    store.mount("lobby");
    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&lobby).expect("the node is on the path now");
        set!(w, pane, "tohri".to_string()).unwrap();
    });
    assert_eq!(store.read::<Lobby>(LOBBY).unwrap().pane, "tohri");
}

// --- single writer (§5.3) --------------------------------------------------

/// Two producers, one field: refused where it can be seen, at **startup**.
/// At runtime each of them looks right on the frames it wins, which is why
/// this cannot be a runtime check.
#[test]
fn a_second_producer_claiming_one_field_fails_at_startup() {
    let mut store = seated();
    store.producer::<Session>(SESSION, &["focused"]).unwrap();
    assert_eq!(
        store
            .producer::<Session>(SESSION, &["seats", "focused"])
            .err(),
        Some(StoreError::AlreadyClaimed {
            scope: SESSION,
            field: "focused".to_string(),
        })
    );
    // The refused claim takes nothing with it: `seats` is still free.
    store.producer::<Session>(SESSION, &["seats"]).unwrap();
}

#[test]
fn a_producer_may_not_set_a_field_it_did_not_claim() {
    let mut store = seated();
    let session = store.producer::<Session>(SESSION, &["focused"]).unwrap();
    let mut refused = None;
    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&session).unwrap();
        refused = set!(w, seats, 4).err();
    });
    assert_eq!(
        refused,
        Some(StoreError::Unclaimed {
            scope: SESSION,
            field: "seats".to_string(),
        })
    );
    assert_eq!(store.read::<Session>(SESSION).unwrap().seats, 0);
}

#[test]
fn a_claim_on_a_field_the_schema_does_not_declare_fails_at_startup() {
    let mut store = seated();
    assert_eq!(
        store.producer::<Session>(SESSION, &["focussed"]).err(),
        Some(StoreError::NoSuchField {
            scope: SESSION,
            field: "focussed".to_string(),
        })
    );
}

// --- the equality check (§5.3) ---------------------------------------------

/// The compare that was hand-rolled sixteen times in one consumer, done
/// once: setting what is already there moves nothing and wakes nobody.
#[test]
fn an_equal_set_is_swallowed() {
    let mut store = seated();
    let lobby = store.producer::<Lobby>(LOBBY, &["pane"]).unwrap();
    let who = store.subscriber();
    store.subscribe(who, LOBBY, "pane").unwrap();

    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&lobby).unwrap();
        assert!(set!(w, pane, "tohri".to_string()).unwrap());
    });
    let after_first = store.version(LOBBY);

    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&lobby).unwrap();
        assert!(
            !set!(w, pane, "tohri".to_string()).unwrap(),
            "the same value is not a change"
        );
    });
    assert!(notice.is_empty(), "an idle frame notifies nobody");
    assert_eq!(store.version(LOBBY), after_first, "and moves no version");
}

/// Dirtiness is derived from values, never remembered: a producer that
/// changes its mind within one tick has changed nothing.
#[test]
fn a_field_put_back_inside_one_tick_wakes_nobody() {
    let mut store = seated();
    let lobby = store.producer::<Lobby>(LOBBY, &["pane"]).unwrap();
    let who = store.subscriber();
    store.subscribe(who, LOBBY, "pane").unwrap();

    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&lobby).unwrap();
        set!(w, pane, "tohri".to_string()).unwrap();
        set!(w, pane, String::new()).unwrap();
    });
    assert!(notice.is_empty());
}

// --- grain (§5.5) ----------------------------------------------------------

/// The store applies the grain the schema declares, so the raw per-frame
/// float that would defeat the equality check and wake every subscriber
/// every frame lands on the minute instead — and the second frame's finer
/// value is swallowed.
#[test]
fn a_declared_grain_is_applied_on_the_way_in() {
    let mut store = Store::new();
    store.provides(LOBBY, Sky::default()).unwrap();
    store.mount("lobby");
    let sky = store.producer::<Sky>(LOBBY, &["hour"]).unwrap();

    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&sky).unwrap();
        set_num!(w, hour, 0.5 + MINUTE * 0.4).unwrap();
    });
    let landed = store.read::<Sky>(LOBBY).unwrap().hour;
    assert!(
        (landed - 0.5).abs() < MINUTE * 1e-6,
        "floored to the minute, not kept at {landed}"
    );

    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&sky).unwrap();
        assert!(
            !set_num!(w, hour, 0.5 + MINUTE * 0.9).unwrap(),
            "a finer float inside the same minute is the same fact"
        );
    });
    assert!(notice.is_empty());

    // The next minute is a different fact, and does wake.
    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&sky).unwrap();
        assert!(set_num!(w, hour, 0.5 + MINUTE * 1.1).unwrap());
    });
    assert_eq!(notice.changed().len(), 1);
}

/// An undeclared grain is exact: `set_num` on a plain field is `set`.
#[test]
fn a_field_with_no_declared_grain_is_taken_as_the_producer_set_it() {
    let mut store = seated();
    let session = store.producer::<Session>(SESSION, &["seats"]).unwrap();
    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&session).unwrap();
        set_num!(w, seats, 3).unwrap();
    });
    assert_eq!(store.read::<Session>(SESSION).unwrap().seats, 3);
}

// --- the two verbs (§5.1) --------------------------------------------------

/// The read verb: a typed consumer downcasts once and then reads fields.
/// The version beside it is the whole of "has this moved?" — one integer,
/// monotonic, never repeated across a scope that left and came back.
#[test]
fn a_version_moves_only_when_a_field_does() {
    let mut store = seated();
    let lobby = store.producer::<Lobby>(LOBBY, &["ready"]).unwrap();
    let mounted = store.version(LOBBY);
    assert!(mounted > 0);

    store.tick(&mut quiet(), |_, _| {});
    assert_eq!(store.version(LOBBY), mounted, "an idle tick moves nothing");

    store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&lobby).unwrap();
        set!(w, ready, true).unwrap();
    });
    let after = store.version(LOBBY);
    assert!(after > mounted);

    store.unmount("lobby");
    assert_eq!(store.version(LOBBY), 0, "an absent scope has no version");
    store.mount("lobby");
    assert!(
        store.version(LOBBY) > after,
        "a scope that came back cannot be mistaken for the one that left"
    );
}

/// The push verb is per-field: a subscriber to one field is not woken by
/// its neighbour. This is §4.2's finest-invalidation claim, at the store.
#[test]
fn a_subscriber_hears_its_own_field_and_not_its_neighbour() {
    let mut store = seated();
    let session = store
        .producer::<Session>(SESSION, &["focused", "seats"])
        .unwrap();
    let watching_focus = store.subscriber();
    let watching_seats = store.subscriber();
    store.subscribe(watching_focus, SESSION, "focused").unwrap();
    store.subscribe(watching_seats, SESSION, "seats").unwrap();

    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&session).unwrap();
        set!(w, focused, "saph".to_string()).unwrap();
    });
    assert!(notice.woke(watching_focus));
    assert!(!notice.woke(watching_seats));
}

/// §5.6: a frozen consumer is the ghost. Unsubscribing is the whole of
/// freezing — the retained tree keeps its last-delivered values because
/// nothing wakes it again.
#[test]
fn an_unsubscribed_consumer_is_told_nothing_more() {
    let mut store = seated();
    let lobby = store.producer::<Lobby>(LOBBY, &["pane"]).unwrap();
    let ghost = store.subscriber();
    store.subscribe(ghost, LOBBY, "pane").unwrap();
    store.unsubscribe_all(ghost);

    let notice = store.tick(&mut quiet(), |_, b| {
        let mut w = b.writer(&lobby).unwrap();
        set!(w, pane, "tohri".to_string()).unwrap();
    });
    assert!(!notice.woke(ghost));
    assert_eq!(notice.changed().len(), 1, "the fact still moved");
}

/// A subscription is validated against the registration, not the mount: a
/// consumer subscribes when it is built, and the path moves afterwards.
#[test]
fn a_scope_can_be_subscribed_to_before_its_node_arrives() {
    let mut store = Store::new();
    store.provides(LOBBY, Lobby::default()).unwrap();
    let who = store.subscriber();
    store.subscribe(who, LOBBY, "pane").unwrap();
    assert_eq!(
        store.subscribe(who, LOBBY, "pain").err(),
        Some(StoreError::NoSuchField {
            scope: LOBBY,
            field: "pain".to_string(),
        })
    );

    // Arriving is news: what the field reads as has changed.
    store.mount("lobby");
    let notice = store.tick(&mut quiet(), |_, _| {});
    assert!(notice.woke(who));
}

// --- the three rungs -------------------------------------------------------

/// The process rung mounts when it is provided and stands through every
/// path — including the empty one, which is the first frame of the process.
#[test]
fn the_process_scope_stands_before_any_path_does() {
    let mut store = Store::new();
    store.provides(Scope::Process, App::default()).unwrap();
    assert!(store.is_mounted(Scope::Process));
    assert_eq!(
        store.read::<App>(Scope::Process).unwrap().launch_fade,
        1.0,
        "the at-mount value is the provider's, and it is not a zero"
    );
}

#[test]
fn a_scope_mounts_at_the_value_its_provider_declared() {
    let store = seated();
    assert_eq!(
        store.read::<Session>(SESSION).unwrap(),
        &Session::default(),
        "before any producer has run"
    );
}

// --- registration errors ---------------------------------------------------

#[test]
fn a_second_provider_for_one_scope_fails_at_startup() {
    let mut store = Store::new();
    store.provides(LOBBY, Lobby::default()).unwrap();
    assert_eq!(
        store.provides(LOBBY, Lobby::default()).err(),
        Some(StoreError::AlreadyProvided(LOBBY))
    );
}

#[test]
fn a_scope_schema_that_is_not_a_record_fails_at_startup() {
    #[derive(Clone)]
    struct Rows;
    impl Schema for Rows {
        fn reflect() -> Kind {
            Kind::List(Box::new(Kind::Str))
        }
    }
    let mut store = Store::new();
    assert_eq!(
        store.provides(LOBBY, Rows).err(),
        Some(StoreError::NotARecord {
            scope: LOBBY,
            kind: "a list",
        })
    );
}

#[test]
fn asking_the_wrong_type_of_a_scope_names_both() {
    let mut store = seated();
    assert!(matches!(
        store.producer::<Lobby>(SESSION, &["pane"]).err(),
        Some(StoreError::WrongType { scope, .. }) if scope == SESSION
    ));
}

/// Every error says which scope, and the field errors say which field —
/// the §4.1 property, one rung down.
#[test]
fn every_refusal_names_the_scope_it_is_about() {
    let printed = StoreError::NoSuchField {
        scope: SESSION,
        field: "focussed".to_string(),
    }
    .to_string();
    assert!(printed.contains("session"), "{printed}");
    assert!(printed.contains("focussed"), "{printed}");
    assert!(StoreError::NotMounted(Scope::Process)
        .to_string()
        .contains("the process scope"));
}

// --- the reflection seam ---------------------------------------------------

/// What WP-2.4 validates a selection against, reachable from the scope
/// alone: the store carries the provider's schema, not a copy of it.
#[test]
fn a_scope_carries_the_reflection_a_selection_validates_against() {
    let store = seated();
    let kind = store.reflection(SESSION).expect("provided");
    assert_eq!(kind, &crate::reflect_of::<Session>());
    assert_eq!(
        kind.field_at("stage").unwrap().initial,
        crate::Initial::Declared(crate::Lit::Str("lobby".into())),
        "a declared at-mount value survives into the store's copy"
    );
    assert!(store.reflection(Scope::Node("arena")).is_none());
}
