//! The write side's tests, against the shape it exists to remove.
//!
//! The fixtures are `untold_lore`'s `chrome::intent_from_raise` and
//! `regency-client`'s colon-packed `menu` id — the two anti-corpora
//! `APPLICATION_BUILD.md` WP-2.3 names — declared here as the vocabularies
//! they become. The [`Intents`] impls are written out by hand because this
//! crate depends on nothing and so cannot reach its own derive; they are
//! transcriptions of exactly what `#[derive(Intent)]` emits, and
//! `lorekeeper/editable-derive/tests/intent.rs` is where the derive itself
//! is held to them.

use super::*;
use crate::store::{Scope, Store, StoreError};
use crate::Outbox;

// --- the anti-corpus, declared ---------------------------------------------

/// untold_lore's menu raises. Six of these were one `character_action`
/// raise whose first *argument* said which of the six it was; two more were
/// a string standing in for a bool because `RaiseArg` had no `as_bool`.
/// Both packings are gone, and neither is a rule that was followed — see
/// the module doc for why neither can be written.
#[derive(Clone, Debug, PartialEq)]
enum Menu {
    RecordEntry,
    /// `name_save("focus")` — a bool the long way round.
    NameSave {
        focused: bool,
    },
    PickWorld {
        world: String,
    },
    WalkBackTo {
        step: usize,
    },
    SkyHour {
        hour: f32,
    },
    /// `character_action("seat")` and its five siblings, each now a name.
    SeatCharacter,
    DeleteCharacter,
    BeginLife,
}

impl Intents for Menu {
    fn vocabulary() -> Vocabulary {
        Vocabulary::new(vec![
            Accepted::new("record_entry", vec![]),
            Accepted::new("name_save", vec![Parameter::of::<bool>("focused")]),
            Accepted::new("pick_world", vec![Parameter::of::<String>("world")]),
            Accepted::new("walk_back_to", vec![Parameter::of::<usize>("step")]),
            Accepted::new("sky_hour", vec![Parameter::of::<f32>("hour")]),
            Accepted::new("seat_character", vec![]),
            Accepted::new("delete_character", vec![]),
            Accepted::new("begin_life", vec![]),
        ])
    }

    fn accept(raise: &Raise) -> Result<Self, Refused> {
        match raise.name() {
            "record_entry" => {
                raise.parameters("record_entry", 0)?;
                Ok(Menu::RecordEntry)
            }
            "name_save" => {
                let mut p = raise.parameters("name_save", 1)?;
                Ok(Menu::NameSave {
                    focused: p.take::<bool>("focused")?,
                })
            }
            "pick_world" => {
                let mut p = raise.parameters("pick_world", 1)?;
                Ok(Menu::PickWorld {
                    world: p.take::<String>("world")?,
                })
            }
            "walk_back_to" => {
                let mut p = raise.parameters("walk_back_to", 1)?;
                Ok(Menu::WalkBackTo {
                    step: p.take::<usize>("step")?,
                })
            }
            "sky_hour" => {
                let mut p = raise.parameters("sky_hour", 1)?;
                Ok(Menu::SkyHour {
                    hour: p.take::<f32>("hour")?,
                })
            }
            "seat_character" => {
                raise.parameters("seat_character", 0)?;
                Ok(Menu::SeatCharacter)
            }
            "delete_character" => {
                raise.parameters("delete_character", 0)?;
                Ok(Menu::DeleteCharacter)
            }
            "begin_life" => {
                raise.parameters("begin_life", 0)?;
                Ok(Menu::BeginLife)
            }
            other => Err(Refused::NoSuchIntent {
                intent: other.to_string(),
            }),
        }
    }
}

/// regency's manor rows, which speak through one `menu(id)` raise whose id
/// is colon-packed — `menu("stash:item:container")`, taken apart by
/// `id.split(':')` in the client's update. P6R splits it into these, and
/// the split is possible precisely because a scope publishes a vocabulary
/// rather than a name.
#[derive(Clone, Debug, PartialEq)]
enum Manor {
    Search { container: String },
    Take { item: String },
    Stash { item: String, container: String },
}

impl Intents for Manor {
    fn vocabulary() -> Vocabulary {
        Vocabulary::new(vec![
            Accepted::new("search", vec![Parameter::of::<String>("container")]),
            Accepted::new("take", vec![Parameter::of::<String>("item")]),
            Accepted::new(
                "stash",
                vec![
                    Parameter::of::<String>("item"),
                    Parameter::of::<String>("container"),
                ],
            ),
        ])
    }

    fn accept(raise: &Raise) -> Result<Self, Refused> {
        match raise.name() {
            "search" => {
                let mut p = raise.parameters("search", 1)?;
                Ok(Manor::Search {
                    container: p.take::<String>("container")?,
                })
            }
            "take" => {
                let mut p = raise.parameters("take", 1)?;
                Ok(Manor::Take {
                    item: p.take::<String>("item")?,
                })
            }
            "stash" => {
                let mut p = raise.parameters("stash", 2)?;
                Ok(Manor::Stash {
                    item: p.take::<String>("item")?,
                    container: p.take::<String>("container")?,
                })
            }
            other => Err(Refused::NoSuchIntent {
                intent: other.to_string(),
            }),
        }
    }
}

/// The host's action type: wider than any one scope's intents, which is the
/// ordinary case and the reason [`Store::raise`] asks for `A: From<I>`.
#[derive(Clone, Debug, PartialEq)]
enum Act {
    Menu(Menu),
    Manor(Manor),
    /// Something no document raised — a service's own request.
    Quit,
}

impl From<Menu> for Act {
    fn from(intent: Menu) -> Self {
        Act::Menu(intent)
    }
}

impl From<Manor> for Act {
    fn from(intent: Manor) -> Self {
        Act::Manor(intent)
    }
}

const MENU: Scope = Scope::Node("menu");
const MANOR: Scope = Scope::Node("manor");

fn published() -> Store {
    let mut store = Store::new();
    store.accepts::<Menu>(MENU).unwrap();
    store.accepts::<Manor>(MANOR).unwrap();
    store
}

fn raise(name: &str, args: Vec<RaiseArg>) -> Raise {
    Raise::new(name, args)
}

// --- what a scope publishes ------------------------------------------------

#[test]
fn a_scope_publishes_the_intents_it_accepts_beside_the_fields_it_provides() {
    let store = published();
    let vocabulary = store.intents(MENU).expect("published");
    assert_eq!(vocabulary.intents().len(), 8);
    assert_eq!(
        vocabulary.intent("name_save").unwrap().parameters[0].kind,
        Kind::Bool,
        "the bool that used to travel as \"focus\"/\"blur\""
    );
    assert!(store.intents(Scope::Node("arena")).is_none());
}

#[test]
fn a_vocabulary_prints_what_a_refusal_would_quote() {
    let printed = Manor::vocabulary().to_string();
    assert!(
        printed.contains("stash(item: str, container: str)"),
        "{printed}"
    );
    assert!(printed.starts_with("accepts {"), "{printed}");
}

// --- a validated raise lands as an outbox action ---------------------------

#[test]
fn a_validated_raise_lands_on_the_outbox_as_a_typed_action() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    // A service's own request, on the same queue: one outbox, one order,
    // and a raised intent is an action like any other once it is typed.
    out.push(Act::Quit);
    store
        .raise::<Menu, Act>(
            MENU,
            &raise("pick_world", vec![RaiseArg::Str("aisling".into())]),
            &mut out,
        )
        .unwrap();
    assert_eq!(
        out.peek(),
        [
            Act::Quit,
            Act::Menu(Menu::PickWorld {
                world: "aisling".to_string()
            })
        ],
        "typed from the name to the action, with no string left in it"
    );
}

/// The `menu("stash:item:container")` split, end to end: two parameters
/// where a packed id used to be, and nothing anywhere takes a string apart.
#[test]
fn a_packed_id_becomes_two_parameters() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    store
        .raise::<Manor, Act>(
            MANOR,
            &raise(
                "stash",
                vec![
                    RaiseArg::Str("brooch".into()),
                    RaiseArg::Str("writing_desk".into()),
                ],
            ),
            &mut out,
        )
        .unwrap();
    assert_eq!(
        out.peek(),
        [Act::Manor(Manor::Stash {
            item: "brooch".to_string(),
            container: "writing_desk".to_string(),
        })]
    );
}

/// The `as_bool` workaround's successor: there isn't one. A parameter
/// declared `bool` is read as a bool, and a raise that carries the old
/// string is refused naming the parameter rather than quietly reading
/// `false`.
#[test]
fn a_bool_parameter_arrives_as_a_bool_and_a_string_is_refused() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    store
        .raise::<Menu, Act>(
            MENU,
            &raise("name_save", vec![RaiseArg::Bool(true)]),
            &mut out,
        )
        .unwrap();
    assert_eq!(out.peek(), [Act::Menu(Menu::NameSave { focused: true })]);

    let err = store
        .raise::<Menu, Act>(
            MENU,
            &raise("name_save", vec![RaiseArg::Str("focus".into())]),
            &mut out,
        )
        .expect_err("the old encoding is not silently true");
    assert_eq!(
        err.refusal(),
        &Refused::Parameter {
            intent: "name_save",
            parameter: "focused",
            want: "bool",
            got: "str",
        }
    );
    assert!(err.to_string().contains("focused"), "{err}");
}

/// The `character_action("seat")` split. Six intents, six names; the
/// argument that used to choose between them has nowhere to be, and a raise
/// still carrying it is refused on arity — *before* an argument is read,
/// which is the ordering that makes argument-dispatch unreachable.
#[test]
fn an_intent_is_decided_by_its_name_and_an_argument_cannot_decide_one() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    for (name, want) in [
        ("seat_character", Menu::SeatCharacter),
        ("delete_character", Menu::DeleteCharacter),
        ("begin_life", Menu::BeginLife),
    ] {
        let mut one: Outbox<Act> = Outbox::new();
        store
            .raise::<Menu, Act>(MENU, &raise(name, vec![]), &mut one)
            .unwrap();
        assert_eq!(one.peek(), [Act::Menu(want)]);
    }

    let err = store
        .raise::<Menu, Act>(
            MENU,
            &raise("seat_character", vec![RaiseArg::Str("seat".into())]),
            &mut out,
        )
        .expect_err("the old packing carried an argument these do not take");
    assert_eq!(
        err.refusal(),
        &Refused::Arity {
            intent: "seat_character",
            want: 0,
            got: 1,
        }
    );
}

#[test]
fn a_raise_no_scope_accepts_is_refused_by_name() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    let err = store
        .raise::<Menu, Act>(MENU, &raise("character_action", vec![]), &mut out)
        .expect_err("the packed name went away with the packing");
    assert_eq!(
        err.refusal(),
        &Refused::NoSuchIntent {
            intent: "character_action".to_string(),
        }
    );
    assert!(err.to_string().contains("menu"), "{err}");
    assert!(out.is_empty(), "a refused raise reaches nobody");
}

#[test]
fn a_scope_that_accepts_nothing_refuses_every_raise() {
    let store = Store::new();
    let mut out: Outbox<Act> = Outbox::new();
    let err = store
        .raise::<Menu, Act>(MENU, &raise("record_entry", vec![]), &mut out)
        .expect_err("nothing published");
    assert_eq!(err.refusal(), &Refused::NothingPublished);
    assert_eq!(err.scope(), MENU);
}

#[test]
fn asking_the_wrong_intent_type_of_a_scope_names_both() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    let err = store
        .raise::<Manor, Act>(
            MENU,
            &raise("search", vec![RaiseArg::Str("x".into())]),
            &mut out,
        )
        .expect_err("the menu does not accept the manor's intents");
    assert!(
        matches!(err.refusal(), Refused::WrongType { .. }),
        "{err:?}"
    );
}

// --- what an argument may be -----------------------------------------------

/// An integer widens into a float because a document writes `1` where it
/// means `1.0`; a float does **not** narrow into an integer, because
/// `2.5` into a step count has a right answer nobody can name.
#[test]
fn an_integer_widens_into_a_float_and_a_float_does_not_narrow() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    store
        .raise::<Menu, Act>(MENU, &raise("sky_hour", vec![RaiseArg::Int(7)]), &mut out)
        .unwrap();
    assert_eq!(out.peek(), [Act::Menu(Menu::SkyHour { hour: 7.0 })]);

    let err = store
        .raise::<Menu, Act>(
            MENU,
            &raise("walk_back_to", vec![RaiseArg::Float(2.5)]),
            &mut out,
        )
        .expect_err("a step is not two and a half");
    assert_eq!(
        err.refusal(),
        &Refused::Parameter {
            intent: "walk_back_to",
            parameter: "step",
            want: "int",
            got: "float",
        }
    );
}

#[test]
fn an_opaque_argument_names_itself_in_the_refusal() {
    let store = published();
    let mut out: Outbox<Act> = Outbox::new();
    let err = store
        .raise::<Menu, Act>(MENU, &raise("pick_world", vec![RaiseArg::Opaque]), &mut out)
        .expect_err("a closure is nothing a host can act on");
    assert!(err.to_string().contains("a closure or a widget"), "{err}");
}

// --- the load-time check, in two grades (§4.1) -----------------------------

/// What a shipped document declares it raises, in the vocabulary the
/// surface framework hands over.
fn declares(raises: &[(&str, &[Kind])]) -> Vec<Declared> {
    raises
        .iter()
        .map(|(name, params)| Declared::new(*name, params.to_vec()))
        .collect()
}

#[test]
fn a_document_that_raises_everything_it_may_drifts_not_at_all() {
    let drifts = Manor::vocabulary().check(&declares(&[
        ("search", &[Kind::Str]),
        ("take", &[Kind::Str]),
        ("stash", &[Kind::Str, Kind::Str]),
    ]));
    assert!(drifts.is_empty(), "{drifts:?}");
}

/// §4.1's two grades, both from one call: the modder's stale expectation
/// refuses and names the intent; the intent nothing raises reports, because
/// a provider legitimately publishes more than a shipped document uses.
#[test]
fn an_unaccepted_raise_refuses_and_an_unraised_intent_reports() {
    let drifts = Manor::vocabulary().check(&declares(&[
        ("search", &[Kind::Str]),
        ("stash", &[Kind::Str, Kind::Str]),
        ("pilfer", &[Kind::Str]),
    ]));
    let refusals: Vec<&Drift> = drifts.iter().filter(|d| d.refuses()).collect();
    assert_eq!(
        refusals,
        [&Drift::Unaccepted {
            intent: "pilfer".to_string()
        }]
    );
    let reports: Vec<&Drift> = drifts.iter().filter(|d| !d.refuses()).collect();
    assert_eq!(
        reports,
        [&Drift::Unraised {
            intent: "take".to_string()
        }],
        "a published intent no shipped document raises is a report, not a refusal"
    );
    assert!(refusals[0].to_string().contains("pilfer"));
}

#[test]
fn a_raise_declared_with_the_wrong_arity_or_shape_names_which() {
    let drifts = Vocabulary::check(
        &Manor::vocabulary(),
        &declares(&[
            ("search", &[Kind::Str, Kind::Str]),
            ("take", &[Kind::Int]),
            ("stash", &[Kind::Str, Kind::Str]),
        ]),
    );
    assert_eq!(
        drifts,
        [
            Drift::Arity {
                intent: "search".to_string(),
                want: 1,
                got: 2,
            },
            Drift::Parameter {
                intent: "take".to_string(),
                parameter: "item".to_string(),
                want: "str",
                got: "int",
            },
        ],
        "every disagreement, not merely the first"
    );
    assert!(drifts[1].to_string().contains("item"), "{}", drifts[1]);
    assert!(drifts.iter().all(|d| d.refuses()));
}

// --- startup checks --------------------------------------------------------

#[test]
fn a_second_vocabulary_for_one_scope_fails_at_startup() {
    let mut store = published();
    assert_eq!(
        store.accepts::<Menu>(MENU).err(),
        Some(StoreError::AlreadyAccepting(MENU))
    );
}

#[test]
fn two_intents_under_one_name_fail_at_startup() {
    /// What a pair of colliding `#[serde(rename)]`s produces. One of the
    /// two could never be raised, and nothing at runtime would say which.
    struct Collided;
    impl Intents for Collided {
        fn vocabulary() -> Vocabulary {
            Vocabulary::new(vec![
                Accepted::new("save", vec![]),
                Accepted::new("save", vec![Parameter::of::<String>("slot")]),
            ])
        }
        fn accept(_: &Raise) -> Result<Self, Refused> {
            Ok(Collided)
        }
    }
    let mut store = Store::new();
    assert_eq!(
        store.accepts::<Collided>(MENU).err(),
        Some(StoreError::DuplicateIntent {
            scope: MENU,
            intent: "save".to_string(),
        })
    );
}

#[test]
fn a_vocabulary_on_an_id_the_table_does_not_register_fails_at_startup() {
    let mut table = crate::RouteTable::new();
    table.at_root("menu");
    let mut store = Store::new();
    store.knows_nodes(table.ids());
    assert_eq!(
        store.accepts::<Menu>(Scope::Node("mneu")).err(),
        Some(StoreError::UnknownNode("mneu")),
        "the typo protection a scope's fields get, on the intents too"
    );
    assert!(store.accepts::<Menu>(MENU).is_ok());
}
