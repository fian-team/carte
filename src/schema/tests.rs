//! The reflection's own tests: the printed form round-trips, a mismatch
//! names the field, and comparison is structural. The derive's coverage of
//! a real projection lives with the derive (`lorekeeper/editable-derive`),
//! because a proc macro cannot be exercised from the crate it writes paths
//! into.

use super::*;

/// One reflection carrying every variant of [`Kind`] and every declaration
/// a field can make — the fixture the round-trip and the comparison tests
/// both work over.
fn everything() -> Kind {
    Kind::Record(vec![
        Field::new("heading", Kind::Str),
        Field::new("count", Kind::Int),
        Field::new("launch_fade", Kind::Float).starting_at(Lit::Float(1.0)),
        Field::new("can_save", Kind::Bool),
        Field::new("tier", Kind::Enum(vec!["calm".into(), "dire".into()])),
        Field::new("skills", Kind::List(Box::new(Kind::Str))),
        Field::new("by_key", Kind::Map(Box::new(Kind::Int))),
        Field::new("bar_hands", Kind::Tuple(vec![Kind::Str, Kind::Str])),
        Field::new(
            "item_card",
            Kind::Record(vec![
                Field::new("show", Kind::Bool),
                Field::new("name", Kind::Str),
            ]),
        ),
        Field::new("sea", Kind::Record(vec![Field::new("has_sea", Kind::Bool)]))
            .absent_when("the stance is down"),
        Field::new("sky_hour_now", Kind::Float).at_grain(1.0 / 1440.0),
        Field::new(
            "verdict",
            Kind::Union(vec![
                Variant::new("Passed", vec![Field::new("margin", Kind::Int)]),
                Variant::new("Failed", vec![]),
            ]),
        ),
        Field::new("branch", Kind::Cycle),
        // A serde rename can produce a name no Rust identifier could be.
        Field::new("kebab-case-name", Kind::Str).starting_at(Lit::Str("a \"quoted\" one".into())),
    ])
}

#[test]
fn a_reflection_round_trips_through_its_printed_form() {
    let reflection = everything();
    let printed = reflection.to_string();
    let read: Kind = printed.parse().expect("what we printed, we can read");
    assert_eq!(read, reflection, "printed:\n{printed}");
}

#[test]
fn an_implied_at_mount_value_is_recovered_rather_than_printed() {
    // The printed form carries only what an author would have written; the
    // kind's own zero comes back from the kind.
    let reflection = Kind::Record(vec![
        Field::new("name", Kind::Str),
        Field::new("tier", Kind::Enum(vec!["calm".into(), "dire".into()])),
        Field::new("sea", Kind::Bool).absent_when("the stance is down"),
    ]);
    let printed = reflection.to_string();
    assert!(!printed.contains('='), "nothing was declared:\n{printed}");
    let read: Kind = printed.parse().expect("it reads back");
    assert_eq!(read, reflection);

    let Kind::Record(fields) = &read else {
        panic!("a record")
    };
    assert_eq!(fields[0].initial, Initial::Implied(Lit::Str(String::new())));
    assert_eq!(
        fields[1].initial,
        Initial::Implied(Lit::Str("calm".into())),
        "a unit enum's zero is its first name"
    );
    assert_eq!(fields[2].initial, Initial::Implied(Lit::Absent));
}

#[test]
fn a_malformed_reflection_says_where_it_stopped() {
    let err = "{ name: nonsense }".parse::<Kind>().unwrap_err();
    assert_eq!(err.expected, "a kind");
    assert!(err.at > 0, "it got past the field name");
}

// --- structural comparison -------------------------------------------------

#[test]
fn a_mismatch_names_the_field() {
    let want = Kind::Record(vec![Field::new(
        "item_card",
        Kind::Record(vec![Field::new("name", Kind::Str)]),
    )]);
    let got = Kind::Record(vec![Field::new(
        "item_card",
        Kind::Record(vec![Field::new("name", Kind::Int)]),
    )]);
    let err = want.compare(&got).expect_err("the kinds differ");
    assert_eq!(err.field(), "item_card.name");
    assert!(err.to_string().contains("item_card.name"), "{err}");
}

#[test]
fn a_missing_field_is_named_and_a_spare_one_is_too() {
    let want = Kind::Record(vec![Field::new("clock", Kind::Str)]);
    let got = Kind::Record(vec![Field::new("tick", Kind::Str)]);
    let err = want
        .compare(&got)
        .expect_err("neither has the other's field");
    assert_eq!(err.field(), "clock");
    assert_eq!(err.difference(), &Difference::Missing);

    let err = got.compare(&want).expect_err("and the other way round");
    assert_eq!(err.field(), "tick");
}

#[test]
fn a_field_inside_a_list_is_named_through_it() {
    let row = |kind| Kind::List(Box::new(Kind::Record(vec![Field::new("fill", kind)])));
    let err = row(Kind::Float)
        .compare(&row(Kind::Str))
        .expect_err("the row's field differs");
    assert_eq!(err.field(), "[].fill");
}

#[test]
fn field_order_is_not_part_of_the_shape() {
    let a = Kind::Record(vec![
        Field::new("who", Kind::Str),
        Field::new("when", Kind::Str),
    ]);
    let b = Kind::Record(vec![
        Field::new("when", Kind::Str),
        Field::new("who", Kind::Str),
    ]);
    assert_eq!(a.compare(&b), Ok(()));
}

#[test]
fn declarations_are_the_providers_own_and_are_not_compared() {
    // §4.7's motivating pair: the sea panel's block is always there in the
    // editor and absent while the stance is down in the world. One fragment
    // has to validate against both, so presence, at-mount value and grain
    // are not part of the shape.
    let world = Kind::Record(vec![Field::new("sea_level", Kind::Float)
        .absent_when("the stance is down")
        .at_grain(0.5)]);
    let editor = Kind::Record(vec![
        Field::new("sea_level", Kind::Float).starting_at(Lit::Float(40.0))
    ]);
    assert_eq!(world.compare(&editor), Ok(()));
}

#[test]
fn two_enums_that_offer_different_names_are_different_shapes() {
    let a = Kind::Record(vec![Field::new(
        "tone",
        Kind::Enum(vec!["good".into(), "bad".into()]),
    )]);
    let b = Kind::Record(vec![Field::new(
        "tone",
        Kind::Enum(vec!["good".into(), "grim".into()]),
    )]);
    let err = a.compare(&b).expect_err("the member sets differ");
    assert_eq!(err.field(), "tone");
    assert!(matches!(err.difference(), Difference::Members { .. }));
}

#[test]
fn a_tuples_arity_is_part_of_its_shape() {
    let a = Kind::Tuple(vec![Kind::Str, Kind::Str]);
    let b = Kind::Tuple(vec![Kind::Str, Kind::Str, Kind::Str]);
    assert_eq!(
        a.compare(&b).unwrap_err().difference(),
        &Difference::Arity { want: 2, got: 3 }
    );
}

#[test]
fn a_union_names_the_variant_its_field_went_missing_from() {
    let variants = |kind| {
        Kind::Union(vec![
            Variant::new("Passed", vec![Field::new("margin", kind)]),
            Variant::new("Failed", vec![]),
        ])
    };
    let err = variants(Kind::Int)
        .compare(&variants(Kind::Str))
        .expect_err("the payload differs");
    assert_eq!(err.field(), "$Passed.margin");
}

#[test]
fn every_back_edge_is_the_same_back_edge() {
    // Nothing nominal survives into the reflection, so two recursive shapes
    // that agree everywhere else agree here too (§4.7).
    assert_eq!(Kind::Cycle.compare(&Kind::Cycle), Ok(()));
}

// --- selection paths -------------------------------------------------------

#[test]
fn a_selection_resolves_through_nested_records() {
    let reflection = everything();
    let field = reflection.field_at("item_card.name").expect("it is there");
    assert_eq!(field.name, "name");
    assert_eq!(field.kind, Kind::Str);
}

#[test]
fn a_selection_naming_nothing_is_refused_by_name() {
    let reflection = everything();
    let err = reflection
        .field_at("item_card.nmae")
        .expect_err("a typo is a refusal");
    assert_eq!(err.field(), "item_card.nmae");
    assert_eq!(err.difference(), &Difference::Missing);
}

#[test]
fn a_selection_cannot_reach_into_a_collection() {
    // §4.2: a collection is one field in v1, and the refusal says so by
    // naming where the path stopped rather than reading back nothing.
    let reflection = everything();
    let err = reflection
        .field_at("skills.label")
        .expect_err("a list has no fields");
    assert_eq!(err.field(), "skills");
    assert_eq!(
        err.difference(),
        &Difference::Kind {
            want: "a record",
            got: "a list"
        }
    );
}

// --- the trait's leaves ----------------------------------------------------

#[test]
fn the_leaves_reflect_without_a_value_in_hand() {
    assert_eq!(reflect_of::<String>(), Kind::Str);
    assert_eq!(reflect_of::<bool>(), Kind::Bool);
    assert_eq!(reflect_of::<u8>(), Kind::Int);
    assert_eq!(reflect_of::<f32>(), Kind::Float);
    assert_eq!(reflect_of::<Vec<String>>(), Kind::List(Box::new(Kind::Str)));
    assert_eq!(
        reflect_of::<(String, String)>(),
        Kind::Tuple(vec![Kind::Str, Kind::Str])
    );
    assert_eq!(
        reflect_of::<[f32; 3]>(),
        Kind::Tuple(vec![Kind::Float, Kind::Float, Kind::Float])
    );
    assert_eq!(
        reflect_of::<HashMap<String, i32>>(),
        Kind::Map(Box::new(Kind::Int))
    );
}

/// A hand-written recursive schema — what the derive emits for a type that
/// reaches itself.
struct Branch;

impl Schema for Branch {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("label", Kind::Str),
            Field::new("children", reflect_of::<Vec<Branch>>()),
        ])
    }
    fn type_name() -> Option<&'static str> {
        Some("Branch")
    }
}

#[test]
fn a_recursive_schema_reflects_as_a_finite_back_edge() {
    let reflection = reflect_of::<Branch>();
    let Kind::Record(fields) = &reflection else {
        panic!("a record")
    };
    assert_eq!(fields[1].kind, Kind::List(Box::new(Kind::Cycle)));
    // And the guard unwinds: a second reflection is not poisoned by the
    // first.
    assert_eq!(reflect_of::<Branch>(), reflection);
}
