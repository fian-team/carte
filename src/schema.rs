//! §4.3's derived reflection: what a scope's schema struct says about
//! itself, in a form that can be read with **no value in hand**.
//!
//! A provider declares a scope's schema as an ordinary Rust type (§4.1).
//! Typed consumers read the struct and pay nothing; a document cannot see
//! a Rust type at all, so what its selection validates against is this —
//! a [`Kind`] tree derived from the struct, never hand-maintained. That is
//! the whole of §4.3: the struct *is* the schema, so the triple-list
//! (struct ↔ `.ogh` ↔ projection) and the guard test that policed it are
//! unrepresentable rather than merely guarded.
//!
//! # The vocabulary, and what it is not
//!
//! This is deliberately a sibling of the engine's `editable::Kind`, which
//! proved the derive move — recognisably the same shapes, the same
//! serde-faithful names — minus what a store scope cannot hold and plus
//! what §4 and §5.5 add. It is *not* that type: `editable` is the engine's
//! content-editing reflection in another repo, and this crate depends on
//! nothing (`APPLICATION.md` §2). The two describe different things and
//! must be free to move apart.
//!
//! Dropped from the editing vocabulary, each for a reason:
//!
//! - `Formula` and `Ref(table)` — an authoring tool's leaves. A store
//!   field is a fact about what should be presented (§5.2), not an
//!   expression to evaluate or an id to resolve against a content table
//!   the store has never heard of.
//! - `Text` — the editorial "show a text area" hint. Presentation, and
//!   §5.5 is exactly the line that keeps presentation out.
//! - `Optional` — optionality moves *up*, onto the field, because §4.1
//!   asks for it to be **stated** rather than implied by a wrapper. See
//!   [`Presence`].
//! - `Named` — the nominal back-edge. §4.7 makes structural comparison the
//!   rule and type names must not be load-bearing in it, so the back-edge
//!   here carries no name: [`Kind::Cycle`].
//!
//! Added, each from an axiom:
//!
//! - [`Presence`] — §4.1: "optionality is stated in the schema ('this may
//!   be absent while X'), not by convention", so an absent-able field
//!   carries the sentence saying when.
//! - [`Initial`] — §4.1: a field's at-mount value is declared, because a
//!   silent zero-default renders as an invisible chrome. What is *not*
//!   declared still lands in the reflection, marked as implied, so the
//!   drift is reportable (§4.1's second grade) instead of invisible.
//! - [`Grain`] — §5.5: a schema may declare a field's grain, because a raw
//!   per-frame float defeats the store's equality check and wakes every
//!   subscriber every frame.
//!
//! # Structural, never nominal
//!
//! [`Kind::compare`] is §4.7 written down: two records with the same field
//! names and kinds are the same shape, whatever their Rust types are
//! called, and field *order* is not part of the shape either. No variant
//! of [`Kind`] carries a type name, so there is nothing nominal available
//! to compare even by accident.
//!
//! # The printed form
//!
//! [`Kind`] prints, through [`Display`](std::fmt::Display), to a small
//! schema language — the form a refusal quotes when it says what shape it
//! wanted. It only prints. Nothing reads a reflection back from text,
//! because nothing writes one: a provider owns its schema as a Rust type
//! (§4.1) and the derive is the only thing that ever builds a `Kind`.

use std::collections::{BTreeMap, HashMap};
use std::fmt;

// --- the vocabulary --------------------------------------------------------

/// A field's shape — the recursive reflection a schema type reports from
/// [`Schema::reflect`].
///
/// Every variant is anonymous: nothing here records what a Rust type is
/// called, which is what makes [`compare`](Kind::compare) structural by
/// construction rather than by discipline (§4.7).
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Str,
    Int,
    Float,
    Bool,
    /// One of a fixed set of names — a payload-free enum. The names are
    /// the serde-faithful ones, because they are what a document reads and
    /// what a producer writes.
    Enum(Vec<String>),
    /// A record with named fields. A scope's schema is one of these.
    Record(Vec<Field>),
    /// A homogeneous list. One field, per §4.2: a row change invalidates
    /// the list's listeners, and that coarseness is named rather than
    /// patched around.
    List(Box<Kind>),
    /// A string-keyed map of a single value shape. Keyed like a list is
    /// indexed; the key type is not part of the shape because a document
    /// only ever reads a key as text.
    Map(Box<Kind>),
    /// A fixed-arity heterogeneous sequence — a Rust tuple or `[T; N]`.
    /// Addressed by position; the arity is part of the shape.
    Tuple(Vec<Kind>),
    /// A tagged union: one of several named variants, each carrying a
    /// record of fields.
    Union(Vec<Variant>),
    /// A back-edge to a record already being expanded — how a recursive
    /// schema type stays a finite reflection.
    ///
    /// It carries **no name**, and that is the §4.7 point: two recursive
    /// shapes that agree everywhere else agree here too, which is the
    /// coinductively right answer and leaves nothing nominal to compare. A
    /// selection cannot descend through a back-edge (see
    /// [`field_at`](Kind::field_at)) — a recursive *fact* is a §5.2 drift
    /// indicator, so the reflection describes one without inviting one.
    Cycle,
}

/// A named field of a [`Kind::Record`] or a union [`Variant`], with the
/// three things §4 and §5.5 ask a schema to declare about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The serde-faithful name — what a document's selection names.
    pub name: String,
    pub kind: Kind,
    /// Whether the field is always there, and when it is not (§4.1).
    pub presence: Presence,
    /// The value the field holds at mount (§4.1).
    pub initial: Initial,
    /// The granularity a producer quantizes to before setting (§5.5).
    pub grain: Grain,
}

impl Field {
    /// A required field of `kind`, taking the kind's implied at-mount value
    /// and no declared grain — the shape most fields have. The declarations
    /// are added with the builders below, which is how the derive emits
    /// them.
    pub fn new(name: impl Into<String>, kind: Kind) -> Self {
        let initial = Initial::Implied(Lit::implied_by(&kind));
        Self {
            name: name.into(),
            kind,
            presence: Presence::Always,
            initial,
            grain: Grain::Exact,
        }
    }

    /// State that the field may be absent, and when — §4.1's sentence,
    /// written once here and read everywhere it surfaces.
    pub fn absent_when(mut self, sentence: impl Into<String>) -> Self {
        self.presence = Presence::Sometimes {
            when: sentence.into(),
        };
        self.initial = Initial::Implied(Lit::Absent);
        self
    }

    /// State the field's at-mount value (§4.1's `launch_fade` lesson).
    pub fn starting_at(mut self, value: Lit) -> Self {
        self.initial = Initial::Declared(value);
        self
    }

    /// State the field's grain: a producer floors to `step` before setting
    /// (§5.5).
    pub fn at_grain(mut self, step: f64) -> Self {
        self.grain = Grain::Step(step);
        self
    }
}

/// One variant of a [`Kind::Union`]: a name and the record it carries. A
/// payload-free variant has no fields; a tuple variant names its positional
/// fields `"0"`, `"1"`, … — the `editable` convention, kept because a
/// document reads these names.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub name: String,
    pub fields: Vec<Field>,
}

impl Variant {
    pub fn new(name: impl Into<String>, fields: Vec<Field>) -> Self {
        Self {
            name: name.into(),
            fields,
        }
    }
}

/// Whether a field is always there — and, when it is not, the authored
/// sentence saying when (§4.1).
///
/// Optionality is a property of the *field* rather than a wrapper around
/// its kind, which is the difference between stating it and implying it. A
/// bare `Option<T>` says only "sometimes nothing"; the consumer left
/// holding it invents its own explanation, which is the same failure
/// [`Refusal`](crate::Refusal) exists to prevent one rung down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Presence {
    /// Present from mount to teardown.
    Always,
    /// May be absent; `when` is the one authored sentence saying when —
    /// "while no world is loaded", "until the roster has answered".
    Sometimes { when: String },
}

impl Presence {
    /// Whether the field may be absent. The predicate a consumer branches
    /// on; the sentence is what it *shows*.
    pub fn may_be_absent(&self) -> bool {
        matches!(self, Presence::Sometimes { .. })
    }
}

/// A field's at-mount value, and whether the schema said so (§4.1).
///
/// Both arms carry a value, because the store must be able to mount a scope
/// with no producer having run yet. The arms differ in what they license a
/// consumer to *report*: an implied zero is exactly untold_lore's
/// `launch_fade` — a chrome that draws perfectly and shows nothing — so the
/// reflection keeps the fact that nobody chose it. Under §4.1's two grades
/// that is a report, not a refusal: a zero is usually right, and a build
/// that refused every undeclared field would be unusable.
#[derive(Clone, Debug, PartialEq)]
pub enum Initial {
    /// Nobody chose it; this is the kind's own zero.
    Implied(Lit),
    /// The schema chose it.
    Declared(Lit),
}

impl Initial {
    /// The value itself, however it was arrived at.
    pub fn value(&self) -> &Lit {
        match self {
            Initial::Implied(v) | Initial::Declared(v) => v,
        }
    }

    /// Whether the schema chose this value rather than falling into the
    /// kind's zero. What a coverage report asks.
    pub fn is_declared(&self) -> bool {
        matches!(self, Initial::Declared(_))
    }
}

/// A literal at-mount value. The scalar vocabulary and nothing more: a
/// default that needs computing is a producer (§5.7), not a schema.
#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// The field mounts absent. Only a [`Presence::Sometimes`] field can.
    Absent,
    /// No literal of its own: an empty list or map, a tuple or record whose
    /// parts each take their own at-mount value.
    Composed,
}

impl Lit {
    /// The zero a kind implies when the schema declares nothing. A unit
    /// enum's zero is its **first** name — the `editable` convention, and
    /// the only honest answer for a type whose values are all names.
    pub fn implied_by(kind: &Kind) -> Lit {
        match kind {
            Kind::Str => Lit::Str(String::new()),
            Kind::Int => Lit::Int(0),
            Kind::Float => Lit::Float(0.0),
            Kind::Bool => Lit::Bool(false),
            Kind::Enum(names) => match names.first() {
                Some(first) => Lit::Str(first.clone()),
                None => Lit::Str(String::new()),
            },
            Kind::Record(_)
            | Kind::List(_)
            | Kind::Map(_)
            | Kind::Tuple(_)
            | Kind::Union(_)
            | Kind::Cycle => Lit::Composed,
        }
    }
}

/// The granularity a producer quantizes a numeric fact to before setting it
/// (§5.5).
///
/// Declared here, enforced by the producer: the sky clock floors to the
/// minute and a countdown to the second because a raw per-frame float
/// defeats the store's equality check and wakes every subscriber every
/// frame. The store (WP-2.2) is where a declared grain can be *checked* on
/// a set; the schema is where it is *said*, so that the sentence lives with
/// the field rather than in whichever producer happened to be written first.
#[derive(Clone, Debug, PartialEq)]
pub enum Grain {
    /// Taken as the producer sets it. Every non-numeric field, and every
    /// numeric one whose every value matters.
    Exact,
    /// Floored to this step.
    Step(f64),
}

// --- the trait -------------------------------------------------------------

/// A type that describes itself as a scope schema.
///
/// Derived, never hand-written (§4.3) — the derive lives in the
/// `#[derive(Editable)]` lineage, `lorekeeper/editable-derive`, because
/// that is where the serde-faithful naming discipline already is. A proc
/// macro emits *paths*, so that crate names this one in the code it writes
/// and not in its `Cargo.toml`; this crate's empty `[dependencies]` is
/// untouched in both directions.
pub trait Schema {
    /// This type's reflection. Prefer [`reflect_of`] at call sites — it
    /// breaks cycles for a recursive type, where this expands one extra
    /// level before the guard engages.
    fn reflect() -> Kind;

    /// A stable name for nominal types, used by [`reflect_of`] to break
    /// recursion — and used *nowhere else*, least of all in
    /// [`Kind::compare`]. Anonymous shapes (scalars, `Vec`, tuples) keep
    /// the default `None`; the derive overrides it.
    ///
    /// There is deliberately no `implied_initial` beside it: a field's
    /// at-mount value follows from its *kind* ([`Lit::implied_by`]), so a
    /// type has no say in it. That is §4.1 held to — a type whose blank
    /// value is not its zero (untold_lore's `Arrival(1.0)`) has to
    /// **declare** it, which is the whole lesson.
    fn type_name() -> Option<&'static str>
    where
        Self: Sized,
    {
        None
    }
}

std::thread_local! {
    /// Names of schema types currently being expanded on this thread, so
    /// [`reflect_of`] can emit a [`Kind::Cycle`] back-edge instead of
    /// recursing forever. The names never leave this stack.
    static EXPANDING: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The cycle-safe entry point for reflecting a type. For a nominal type
/// already being expanded on this stack it yields [`Kind::Cycle`];
/// otherwise it expands [`Schema::reflect`] with that name guarded. The
/// derive routes every field through this.
pub fn reflect_of<T: Schema>() -> Kind {
    match T::type_name() {
        Some(name) => {
            if EXPANDING.with(|s| s.borrow().contains(&name)) {
                return Kind::Cycle;
            }
            EXPANDING.with(|s| s.borrow_mut().push(name));
            let k = T::reflect();
            EXPANDING.with(|s| {
                s.borrow_mut().pop();
            });
            k
        }
        None => T::reflect(),
    }
}

// --- leaves ----------------------------------------------------------------

impl Schema for String {
    fn reflect() -> Kind {
        Kind::Str
    }
}

impl Schema for bool {
    fn reflect() -> Kind {
        Kind::Bool
    }
}

/// `impl Schema` for the numeric scalars. Ogham's value model has one
/// integer type and one float type, so the Rust widths collapse to two
/// kinds exactly as they do in `editable`.
macro_rules! impl_numeric_leaf {
    ($($t:ty => $kind:expr);+ $(;)?) => {$(
        impl Schema for $t {
            fn reflect() -> Kind { $kind }
        }
    )+};
}

impl_numeric_leaf! {
    i64   => Kind::Int;
    i32   => Kind::Int;
    i16   => Kind::Int;
    i8    => Kind::Int;
    isize => Kind::Int;
    u64   => Kind::Int;
    u32   => Kind::Int;
    u16   => Kind::Int;
    u8    => Kind::Int;
    usize => Kind::Int;
    f64   => Kind::Float;
    f32   => Kind::Float;
}

impl<T: Schema> Schema for Vec<T> {
    fn reflect() -> Kind {
        Kind::List(Box::new(reflect_of::<T>()))
    }
}

impl<T: Schema, const N: usize> Schema for [T; N] {
    fn reflect() -> Kind {
        Kind::Tuple((0..N).map(|_| reflect_of::<T>()).collect())
    }
}

/// `impl Schema` for the fixed heterogeneous tuples — untold_lore's
/// `bar_hands: (String, String)` is the corpus case.
macro_rules! impl_tuple {
    ($($name:ident),+) => {
        impl<$($name: Schema),+> Schema for ($($name,)+) {
            fn reflect() -> Kind {
                Kind::Tuple(::std::vec![$(reflect_of::<$name>()),+])
            }
        }
    };
}

impl_tuple!(A, B);
impl_tuple!(A, B, C);
impl_tuple!(A, B, C, D);

/// `impl Schema` for the keyed maps. Only the value shape is part of the
/// reflection: a document reads a key as text whatever the Rust key type
/// is, so a key type in the shape would be a distinction no consumer can
/// observe.
macro_rules! impl_map {
    ($($map:ident),+ $(,)?) => {$(
        impl<K, V: Schema> Schema for $map<K, V> {
            fn reflect() -> Kind {
                Kind::Map(Box::new(reflect_of::<V>()))
            }
        }
    )+};
}

impl_map!(BTreeMap, HashMap);

// --- structural comparison -------------------------------------------------

/// Where two reflections stop agreeing, and how.
///
/// It always **names the field** (§4.1's "a load-time refusal that names
/// the field"): [`field`](Mismatch::field) is the dotted path from the root
/// of the reflection to the offending place.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    path: String,
    at: Difference,
}

impl Mismatch {
    /// The dotted path to the offending field — `"item_card.name"`,
    /// `"wardrobe[].tints[].swatches"`. Empty only when the two reflections
    /// disagree at the root.
    pub fn field(&self) -> &str {
        &self.path
    }

    /// How they differ, for a consumer that wants to act rather than print.
    pub fn difference(&self) -> &Difference {
        &self.at
    }

    fn new(path: impl Into<String>, at: Difference) -> Self {
        Self {
            path: path.into(),
            at,
        }
    }
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "the reflection itself: {}", self.at)
        } else {
            write!(f, "`{}`: {}", self.path, self.at)
        }
    }
}

/// The ways two shapes can disagree.
#[derive(Clone, Debug, PartialEq)]
pub enum Difference {
    /// The shape being validated has this field; the one validated against
    /// does not. The modder's case, and the one §4.1 wants named.
    Missing,
    /// The shape validated against has this field and the other does not —
    /// provided, and read by nothing (§4.1's unread direction).
    Unexpected,
    /// Both have the field; the kinds are different shapes.
    Kind {
        want: &'static str,
        got: &'static str,
    },
    /// Two enums or two unions offer different names.
    Members { want: Vec<String>, got: Vec<String> },
    /// Two tuples are different lengths. The arity is part of the type, so
    /// this is a shape difference and not a value one.
    Arity { want: usize, got: usize },
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Difference::Missing => f.write_str("no such field"),
            Difference::Unexpected => f.write_str("provided, but named by nothing"),
            Difference::Kind { want, got } => write!(f, "expected {want}, found {got}"),
            Difference::Members { want, got } => {
                write!(
                    f,
                    "expected one of [{}], found [{}]",
                    want.join(", "),
                    got.join(", ")
                )
            }
            Difference::Arity { want, got } => write!(f, "expected {want} parts, found {got}"),
        }
    }
}

impl Kind {
    /// This shape's one-word name, for a diagnostic.
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Str => "str",
            Kind::Int => "int",
            Kind::Float => "float",
            Kind::Bool => "bool",
            Kind::Enum(_) => "an enum",
            Kind::Record(_) => "a record",
            Kind::List(_) => "a list",
            Kind::Map(_) => "a map",
            Kind::Tuple(_) => "a tuple",
            Kind::Union(_) => "a union",
            Kind::Cycle => "a back-edge",
        }
    }

    /// Compare two reflections **structurally** (§4.7): field names and
    /// kinds, never type names, and never field order. This is what lets
    /// one fragment validate against every scope that provides its shape —
    /// untold_lore's sea panel under the world root and under the editor —
    /// and it is the reason nothing in [`Kind`] carries a type name.
    ///
    /// What is *not* compared: [`Presence`], [`Initial`] and [`Grain`].
    /// Those are declarations each provider makes about its own field, and
    /// they legitimately differ between two providers of the same shape —
    /// the sea panel's block is always there in the editor and absent while
    /// the stance is down in the world, which is the very pair §4.7 exists
    /// to admit. A consumer reads the presence of the scope it is mounted
    /// under; the shape is what has to agree.
    ///
    /// `self` is the shape being validated (the consumer's), `other` the
    /// shape validated against (the provider's), which is what decides
    /// whether a stray field reads as [`Difference::Missing`] or
    /// [`Difference::Unexpected`].
    pub fn compare(&self, other: &Kind) -> Result<(), Mismatch> {
        self.compare_at("", other)
    }

    fn compare_at(&self, path: &str, other: &Kind) -> Result<(), Mismatch> {
        match (self, other) {
            (Kind::Str, Kind::Str)
            | (Kind::Int, Kind::Int)
            | (Kind::Float, Kind::Float)
            | (Kind::Bool, Kind::Bool)
            | (Kind::Cycle, Kind::Cycle) => Ok(()),
            (Kind::Enum(want), Kind::Enum(got)) => same_names(path, want, got),
            (Kind::Record(want), Kind::Record(got)) => compare_fields(path, want, got),
            (Kind::List(want), Kind::List(got)) => want.compare_at(&join_part(path, "[]"), got),
            (Kind::Map(want), Kind::Map(got)) => want.compare_at(&join_part(path, "{}"), got),
            (Kind::Tuple(want), Kind::Tuple(got)) => {
                if want.len() != got.len() {
                    return Err(Mismatch::new(
                        path,
                        Difference::Arity {
                            want: want.len(),
                            got: got.len(),
                        },
                    ));
                }
                for (i, (w, g)) in want.iter().zip(got).enumerate() {
                    w.compare_at(&join_part(path, &format!(".{i}")), g)?;
                }
                Ok(())
            }
            (Kind::Union(want), Kind::Union(got)) => {
                let want_names: Vec<String> = want.iter().map(|v| v.name.clone()).collect();
                let got_names: Vec<String> = got.iter().map(|v| v.name.clone()).collect();
                same_names(path, &want_names, &got_names)?;
                for w in want {
                    let g = got
                        .iter()
                        .find(|g| g.name == w.name)
                        .expect("the name sets already agree");
                    compare_fields(
                        &join_part(path, &format!("${}", w.name)),
                        &w.fields,
                        &g.fields,
                    )?;
                }
                Ok(())
            }
            (want, got) => Err(Mismatch::new(
                path,
                Difference::Kind {
                    want: want.name(),
                    got: got.name(),
                },
            )),
        }
    }

    /// Resolve a dotted selection path against this reflection, naming the
    /// field that fails (§4.1).
    ///
    /// It descends through records only. A path that tries to reach *into*
    /// a list, a map or a union stops there and says so: a collection is
    /// one field in v1 (§4.2), a union's fields belong to whichever variant
    /// is live, and a back-edge is where a recursive fact would start
    /// growing into the god object §5.2 fences off. Every one of those is a
    /// refusal naming the field rather than a silent empty read.
    pub fn field_at(&self, path: &str) -> Result<&Field, Mismatch> {
        let mut here = self;
        let mut walked = String::new();
        let mut found: Option<&Field> = None;
        for segment in path.split('.') {
            let fields = match here {
                Kind::Record(fields) => fields,
                other => {
                    return Err(Mismatch::new(
                        walked,
                        Difference::Kind {
                            want: "a record",
                            got: other.name(),
                        },
                    ))
                }
            };
            let next = join_field(&walked, segment);
            match fields.iter().find(|f| f.name == segment) {
                Some(field) => {
                    here = &field.kind;
                    found = Some(field);
                    walked = next;
                }
                None => return Err(Mismatch::new(next, Difference::Missing)),
            }
        }
        found.ok_or_else(|| Mismatch::new(String::new(), Difference::Missing))
    }
}

fn join_part(path: &str, part: &str) -> String {
    let mut joined = String::with_capacity(path.len() + part.len());
    joined.push_str(path);
    joined.push_str(part);
    joined
}

fn join_field(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_string()
    } else {
        format!("{path}.{name}")
    }
}

fn same_names(path: &str, want: &[String], got: &[String]) -> Result<(), Mismatch> {
    let mut a: Vec<&String> = want.iter().collect();
    let mut b: Vec<&String> = got.iter().collect();
    a.sort();
    b.sort();
    if a == b {
        Ok(())
    } else {
        Err(Mismatch::new(
            path,
            Difference::Members {
                want: want.to_vec(),
                got: got.to_vec(),
            },
        ))
    }
}

/// Records compare by name, not by position: two structs that declare the
/// same fields in a different order are the same shape (§4.7).
fn compare_fields(path: &str, want: &[Field], got: &[Field]) -> Result<(), Mismatch> {
    for w in want {
        match got.iter().find(|g| g.name == w.name) {
            Some(g) => w.kind.compare_at(&join_field(path, &w.name), &g.kind)?,
            None => {
                return Err(Mismatch::new(
                    join_field(path, &w.name),
                    Difference::Missing,
                ))
            }
        }
    }
    for g in got {
        if !want.iter().any(|w| w.name == g.name) {
            return Err(Mismatch::new(
                join_field(path, &g.name),
                Difference::Unexpected,
            ));
        }
    }
    Ok(())
}

// --- the printed form ------------------------------------------------------

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_kind(f, self, 0)
    }
}

fn write_kind(f: &mut fmt::Formatter<'_>, kind: &Kind, depth: usize) -> fmt::Result {
    match kind {
        Kind::Str | Kind::Int | Kind::Float | Kind::Bool | Kind::Cycle => {
            f.write_str(kind_word(kind))
        }
        Kind::Enum(names) => {
            f.write_str("one of [")?;
            for (i, name) in names.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_quoted(f, name)?;
            }
            f.write_str("]")
        }
        Kind::Record(fields) => write_block(f, fields, depth),
        Kind::List(inner) => {
            f.write_str("[")?;
            write_kind(f, inner, depth)?;
            f.write_str("]")
        }
        Kind::Map(inner) => {
            f.write_str("<")?;
            write_kind(f, inner, depth)?;
            f.write_str(">")
        }
        Kind::Tuple(parts) => {
            f.write_str("(")?;
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write_kind(f, part, depth)?;
            }
            f.write_str(")")
        }
        Kind::Union(variants) => {
            f.write_str("union {")?;
            for v in variants {
                f.write_str("\n")?;
                indent(f, depth + 1)?;
                write_name(f, &v.name)?;
                f.write_str(" ")?;
                write_block(f, &v.fields, depth + 1)?;
            }
            f.write_str("\n")?;
            indent(f, depth)?;
            f.write_str("}")
        }
    }
}

fn write_block(f: &mut fmt::Formatter<'_>, fields: &[Field], depth: usize) -> fmt::Result {
    if fields.is_empty() {
        return f.write_str("{}");
    }
    f.write_str("{")?;
    for field in fields {
        f.write_str("\n")?;
        indent(f, depth + 1)?;
        write_name(f, &field.name)?;
        if let Presence::Sometimes { when } = &field.presence {
            f.write_str("? ")?;
            write_quoted(f, when)?;
        }
        f.write_str(": ")?;
        write_kind(f, &field.kind, depth + 1)?;
        // Only a *declared* at-mount value is printed: an implied one is
        // recoverable from the kind and the presence, so the printed form
        // stays the schema an author would have written.
        if let Initial::Declared(value) = &field.initial {
            f.write_str(" = ")?;
            write_lit(f, value)?;
        }
        if let Grain::Step(step) = field.grain {
            write!(f, " @ {step:?}")?;
        }
        f.write_str(",")?;
    }
    f.write_str("\n")?;
    indent(f, depth)?;
    f.write_str("}")
}

fn indent(f: &mut fmt::Formatter<'_>, depth: usize) -> fmt::Result {
    for _ in 0..depth {
        f.write_str("    ")?;
    }
    Ok(())
}

fn kind_word(kind: &Kind) -> &'static str {
    match kind {
        Kind::Str => "str",
        Kind::Int => "int",
        Kind::Float => "float",
        Kind::Bool => "bool",
        Kind::Cycle => "cycle",
        _ => unreachable!("only the wordless leaves have a word"),
    }
}

/// A name prints bare when it reads as an identifier and quoted otherwise —
/// serde renames produce `kebab-case` names, and a schema that could not
/// print one would be a schema that could not describe a real type.
fn write_name(f: &mut fmt::Formatter<'_>, name: &str) -> fmt::Result {
    if is_bare_name(name) {
        f.write_str(name)
    } else {
        write_quoted(f, name)
    }
}

fn is_bare_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn write_quoted(f: &mut fmt::Formatter<'_>, text: &str) -> fmt::Result {
    f.write_str("\"")?;
    for c in text.chars() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' => f.write_str("\\\\")?,
            '\n' => f.write_str("\\n")?,
            other => write!(f, "{other}")?,
        }
    }
    f.write_str("\"")
}

fn write_lit(f: &mut fmt::Formatter<'_>, lit: &Lit) -> fmt::Result {
    match lit {
        Lit::Str(s) => write_quoted(f, s),
        // `{:?}` on a float is the shortest form that prints exactly, and
        // it always carries a `.` or an `e`, so a float never reads as an
        // int in the printed form.
        Lit::Float(v) => write!(f, "{v:?}"),
        Lit::Int(v) => write!(f, "{v}"),
        Lit::Bool(v) => write!(f, "{v}"),
        Lit::Absent => f.write_str("absent"),
        Lit::Composed => f.write_str("composed"),
    }
}

#[cfg(test)]
mod tests;
