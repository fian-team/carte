//! §4.4's write side: the intents a scope accepts, published exactly as
//! the fields it provides are.
//!
//! A provider declares a scope's schema as a Rust type and the store
//! publishes its [reflection](crate::schema); a provider declares the
//! scope's **intents** as a Rust enum and the store publishes its
//! [`Vocabulary`]. One declaration, two directions, and the document's
//! `events {}` block validates against the second at load exactly as its
//! selection validates against the first. That is the whole of §4.4, and
//! it is what makes untold_lore's `every_declared_raise_reaches_a_handler`
//! delete rather than merely pass.
//!
//! # The anti-corpus, and what is no longer expressible
//!
//! The shape being removed is `untold_lore`'s `chrome::intent_from_raise`
//! — one hand-written function turning `(name, Vec<RaiseArg>)` into a game
//! intent, ninety lines of positional decoding. Two of its lines are the
//! reason this module exists:
//!
//! ```ignore
//! "name_save" => UiIntent::NameSave(text(0)? == "focus"),
//! "character_action" => match text(0)? { "seat" => …, "delete" => …, … }
//! ```
//!
//! The first encodes a bool as a string because `RaiseArg` had no
//! `as_bool`; the second decides *which intent this is* by parsing an
//! argument. Neither survives the move, and not because a rule forbids
//! them:
//!
//! - **Nothing hand-writes a decoder.** [`Intents::accept`] is derived
//!   beside the vocabulary from one enum declaration
//!   (`#[derive(Intent)]`), so the vocabulary and the decoder cannot
//!   disagree about what a raise means.
//! - **An argument is unreachable until the intent is named.** A [`Raise`]
//!   offers its [`name`](Raise::name) and nothing else. The arguments are
//!   reachable only through [`Raise::parameters`], which takes the intent's
//!   name and its arity — so by the time any argument can be read, the
//!   question "which intent is this?" has already been answered, by name.
//!   `text(0)? == "focus"` has nowhere to stand.
//! - **An argument is read at its declared type.** [`Args::take`] is
//!   generic over [`Argument`] and is handed the parameter's Rust type by
//!   the derive; `bool` is one of them. There is no accessor to be missing,
//!   and therefore no workaround to reach for.
//! - **The cursor runs forwards.** [`Args::take`] has no index. Reading
//!   argument 0 twice — which every branching decoder does — is not a rule
//!   that is broken, it is a call that does not exist.
//!
//! The same move splits regency's colon-packed vocabulary
//! (`menu("stash:item:container")`, decoded by `id.split(':')` in
//! `regency-client`'s update): where one intent carried a packed string,
//! a scope publishes `stash(item, container)` and the packing has nowhere
//! left to happen.
//!
//! # Two grades, already
//!
//! [`Vocabulary::check`] is what a document's `events {}` is checked
//! against at load, and it answers in §4.1's two grades without being
//! asked twice: an intent a document raises and the scope does not accept
//! **refuses** (the modder's case — loud, immediate, named), while an
//! intent the scope publishes and no shipped document raises **reports**,
//! because in a modding world a provider legitimately publishes intents no
//! shipped document uses. [`Drift::refuses`] is the line between them.

use std::fmt;

use crate::schema::{Kind, Schema};
use crate::RaiseArg;

// --- what a scope accepts --------------------------------------------------

/// One intent a scope accepts: its name, and the parameters it carries.
///
/// The name is the one a document raises and the serde-faithful one the
/// derive wrote, for the same reason a field's is (§4.3): it is what
/// crosses the Rust/document boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct Accepted {
    pub name: String,
    pub parameters: Vec<Parameter>,
}

impl Accepted {
    pub fn new(name: impl Into<String>, parameters: Vec<Parameter>) -> Self {
        Self {
            name: name.into(),
            parameters,
        }
    }
}

/// One parameter of an [`Accepted`] intent: a name and a shape.
///
/// The shape is always a scalar, and that is a property of the seam rather
/// than a simplification: a [`RaiseArg`] carries a string, a number or a
/// bool, because an event handler must be `Send + Sync` and the surface
/// runtime's values are not. A fact travels the other way, through the
/// store; a raise carries what a click knew.
///
/// The name is not compared at load — a document's `events {}` declares
/// parameters positionally — but it is what a refusal says, and a refusal
/// that says `who` instead of `argument 0` is the difference between this
/// and the thing it replaces.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub kind: Kind,
}

impl Parameter {
    /// The parameter of type `T`, named. The derive emits one of these per
    /// field of an intent's variant, routing the kind through
    /// [`reflect_of`](crate::schema::reflect_of) exactly as a schema field
    /// does, so one shape vocabulary describes both directions.
    pub fn of<T: Schema>(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: crate::schema::reflect_of::<T>(),
        }
    }
}

/// The intents one scope accepts (§4.4) — the write half of its contract,
/// standing beside [`Store::reflection`](crate::Store::reflection)'s read
/// half.
///
/// Derived, never hand-written, for §4.3's reason unchanged: a vocabulary
/// somebody maintains beside the enum is a second list to drift.
#[derive(Clone, Debug, PartialEq)]
pub struct Vocabulary {
    intents: Vec<Accepted>,
}

impl Vocabulary {
    pub fn new(intents: Vec<Accepted>) -> Self {
        Self { intents }
    }

    /// Every intent, in the order the provider declared them.
    pub fn intents(&self) -> &[Accepted] {
        &self.intents
    }

    /// One intent by name.
    pub fn intent(&self, name: &str) -> Option<&Accepted> {
        self.intents.iter().find(|i| i.name == name)
    }

    /// The first name declared twice, if any. A startup question the store
    /// asks when a vocabulary is published: two `#[serde(rename)]`s can
    /// collide where two Rust variants cannot, and two intents under one
    /// name is one of them silently unreachable.
    pub fn duplicate(&self) -> Option<&str> {
        self.intents.iter().enumerate().find_map(|(i, intent)| {
            self.intents[..i]
                .iter()
                .any(|earlier| earlier.name == intent.name)
                .then_some(intent.name.as_str())
        })
    }

    /// Check what a document says it raises against what this scope
    /// accepts, in §4.1's two grades.
    ///
    /// Every disagreement is reported, not just the first: a load-time
    /// refusal that names one of four drifted raises makes a modder fix
    /// them one restart at a time.
    pub fn check(&self, declared: &[Declared]) -> Vec<Drift> {
        let mut drifts = Vec::new();
        for raised in declared {
            drifts.extend(self.check_one(raised));
        }
        for accepted in &self.intents {
            if !declared.iter().any(|d| d.name == accepted.name) {
                drifts.push(Drift::Unraised {
                    intent: accepted.name.clone(),
                });
            }
        }
        drifts
    }

    /// One raise, checked against this vocabulary — every grade except
    /// [`Drift::Unraised`].
    ///
    /// That one is left out because it is not a question about a raise at
    /// all: an intent is unraised only if *no* shipped document raises it,
    /// which is a question about the whole set (`APPLICATION.md` §4.1's
    /// second grade). [`check`](Vocabulary::check) is this in a loop plus
    /// that question answered for one document;
    /// [`Validation`](crate::Validation) is it answered across several,
    /// where a document may mount under more than one scope and only one of
    /// them accepts a given name.
    pub fn check_one(&self, raised: &Declared) -> Vec<Drift> {
        let Some(accepted) = self.intent(&raised.name) else {
            return vec![Drift::Unaccepted {
                intent: raised.name.clone(),
            }];
        };
        if accepted.parameters.len() != raised.parameters.len() {
            return vec![Drift::Arity {
                intent: raised.name.clone(),
                want: accepted.parameters.len(),
                got: raised.parameters.len(),
            }];
        }
        accepted
            .parameters
            .iter()
            .zip(&raised.parameters)
            .filter(|(want, got)| want.kind != **got)
            .map(|(want, got)| Drift::Parameter {
                intent: raised.name.clone(),
                parameter: want.name.clone(),
                want: want.kind.name(),
                got: got.name(),
            })
            .collect()
    }
}

/// One raise a document declares it makes: a name and its parameter shapes,
/// in order.
///
/// The surface framework builds these from a document's `events {}` block
/// and hands them over. Positional, because that is what the block declares
/// — `save(string, int)` names no parameters — so [`Vocabulary::check`]
/// compares shapes in order and keeps the provider's names for the
/// diagnostic. The conversion from the surface framework's own type
/// vocabulary lives on the surface side of the seam, where that vocabulary
/// does (§2).
#[derive(Clone, Debug, PartialEq)]
pub struct Declared {
    pub name: String,
    pub parameters: Vec<Kind>,
}

impl Declared {
    pub fn new(name: impl Into<String>, parameters: Vec<Kind>) -> Self {
        Self {
            name: name.into(),
            parameters,
        }
    }
}

/// A disagreement between what a scope accepts and what a document raises.
///
/// Two grades (§4.1), and [`refuses`](Drift::refuses) is the line: a
/// document naming an intent that does not exist is the modder's stale
/// expectation and must be loud, while a provider publishing an intent
/// nothing raises is a developer mid-build — or a modding surface working
/// exactly as intended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Drift {
    /// The document raises an intent this scope does not accept. Refuses.
    Unaccepted { intent: String },
    /// The document raises an accepted intent with the wrong number of
    /// arguments. Refuses.
    Arity {
        intent: String,
        want: usize,
        got: usize,
    },
    /// The document declares a parameter as a shape the intent does not
    /// take. Refuses, naming the parameter.
    Parameter {
        intent: String,
        parameter: String,
        want: &'static str,
        got: &'static str,
    },
    /// The scope accepts an intent no document raises. Reports.
    Unraised { intent: String },
}

impl Drift {
    /// Whether this drift refuses the document or merely reports it
    /// (§4.1's two grades).
    pub fn refuses(&self) -> bool {
        !matches!(self, Drift::Unraised { .. })
    }

    /// The intent this drift is about. Always named — the property §4.1
    /// asks of a selection refusal, held on the write side too.
    pub fn intent(&self) -> &str {
        match self {
            Drift::Unaccepted { intent }
            | Drift::Arity { intent, .. }
            | Drift::Parameter { intent, .. }
            | Drift::Unraised { intent } => intent,
        }
    }
}

impl fmt::Display for Drift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Drift::Unaccepted { intent } => {
                write!(f, "`{intent}` is raised here, but no scope accepts it")
            }
            Drift::Arity { intent, want, got } => write!(
                f,
                "`{intent}` takes {want} argument(s), and this document raises it with {got}"
            ),
            Drift::Parameter {
                intent,
                parameter,
                want,
                got,
            } => write!(
                f,
                "`{intent}`'s parameter `{parameter}` is {want}, and this document raises it as \
                 {got}"
            ),
            Drift::Unraised { intent } => write!(
                f,
                "`{intent}` is accepted, but no shipped document raises it"
            ),
        }
    }
}

// --- the raise, and the typed thing it becomes -----------------------------

/// One named raise, as it arrives from a document.
///
/// It offers its [`name`](Raise::name) and nothing else, which is the
/// module doc's second bullet written as an API: an argument is unreachable
/// until an intent has been named, so no decoder can dispatch on one.
/// [`parameters`](Raise::parameters) is the derive's door and takes the
/// name it has already matched.
#[derive(Clone, Debug, PartialEq)]
pub struct Raise {
    name: String,
    args: Vec<RaiseArg>,
}

impl Raise {
    /// Build a raise from what a document event carried. The surface
    /// framework's call, at the one point a `Value` becomes data (§2).
    pub fn new(name: impl Into<String>, args: Vec<RaiseArg>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// What was raised. The only question askable before an intent is
    /// named, and therefore the only thing a decoder can branch on.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Open this raise's arguments, for the intent named and the arity it
    /// declares.
    ///
    /// The derive's door — a host never calls it, because a host never
    /// writes a decoder. The arity is checked here rather than at the end,
    /// so a wrong-length raise is refused before a single argument is read
    /// and the refusal is about the raise rather than about a parameter
    /// that happened to be missing.
    pub fn parameters(&self, intent: &'static str, arity: usize) -> Result<Args<'_>, Refused> {
        if self.args.len() != arity {
            return Err(Refused::Arity {
                intent,
                want: arity,
                got: self.args.len(),
            });
        }
        Ok(Args {
            intent,
            args: &self.args,
            next: 0,
        })
    }
}

/// A raise's arguments, read forwards, at their declared types.
///
/// No index and no length: the derive reads each parameter once, in the
/// order the variant declares them, and the arity was settled by
/// [`Raise::parameters`]. Both absences are load-bearing — see the module
/// doc.
pub struct Args<'a> {
    intent: &'static str,
    args: &'a [RaiseArg],
    next: usize,
}

impl Args<'_> {
    /// Read the next argument as `T`, the type the parameter declares.
    ///
    /// `parameter` is the name, for the refusal; it never selects anything,
    /// because the cursor's position does.
    pub fn take<T: Argument>(&mut self, parameter: &'static str) -> Result<T, Refused> {
        let arg = self.args.get(self.next).ok_or(Refused::Arity {
            intent: self.intent,
            want: self.next + 1,
            got: self.args.len(),
        })?;
        self.next += 1;
        T::from_arg(arg).ok_or(Refused::Parameter {
            intent: self.intent,
            parameter,
            want: T::SHAPE,
            got: arg.shape(),
        })
    }
}

/// A type an intent's parameter may be declared as: the scalars a
/// [`RaiseArg`] can carry.
///
/// Deliberately not blanket-implemented and deliberately small. A raise
/// carries what a click knew — an id, a count, a flag — and a parameter
/// asking for anything richer is a fact, which travels the other way
/// (§5.2). A type outside this list fails to compile at the derive, which
/// is where the declaration is.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be an intent's parameter",
    note = "a raise carries what a click knew — a string, a number or a bool; a richer shape \
            is a fact, and a fact travels the other way, through the store"
)]
pub trait Argument: Sized {
    /// This type's shape, as a refusal names it. The same word
    /// [`Kind::name`] uses, so one vocabulary describes both directions.
    const SHAPE: &'static str;

    /// Read one argument, or refuse. Never a coercion that loses
    /// something: see the impls below for the one widening that is allowed
    /// and the one that is not.
    fn from_arg(arg: &RaiseArg) -> Option<Self>;
}

impl Argument for String {
    const SHAPE: &'static str = "str";
    fn from_arg(arg: &RaiseArg) -> Option<Self> {
        match arg {
            RaiseArg::Str(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl Argument for bool {
    const SHAPE: &'static str = "bool";
    fn from_arg(arg: &RaiseArg) -> Option<Self> {
        match arg {
            RaiseArg::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// `impl Argument` for the integer widths.
///
/// An integer argument reads as an integer and a float does **not**: a
/// document raising `2.5` into a count is a mistake with a right answer
/// nobody can name, and truncating it silently is how a grid picks the
/// wrong cell for a week. The refusal says which parameter.
macro_rules! impl_integer_argument {
    ($($t:ty),+ $(,)?) => {$(
        impl Argument for $t {
            const SHAPE: &'static str = "int";
            fn from_arg(arg: &RaiseArg) -> Option<Self> {
                match arg {
                    RaiseArg::Int(i) => <$t>::try_from(*i).ok(),
                    _ => None,
                }
            }
        }
    )+};
}

impl_integer_argument!(i64, i32, i16, i8, isize, u64, u32, u16, u8, usize);

/// `impl Argument` for the float widths.
///
/// An integer *does* widen into a float, because a document writes `1`
/// where it means `1.0` and there is nothing to lose on the way in — the
/// asymmetry with the integers above is the point, not an oversight.
macro_rules! impl_float_argument {
    ($($t:ty),+ $(,)?) => {$(
        impl Argument for $t {
            const SHAPE: &'static str = "float";
            fn from_arg(arg: &RaiseArg) -> Option<Self> {
                match arg {
                    RaiseArg::Float(f) => Some(*f as $t),
                    RaiseArg::Int(i) => Some(*i as $t),
                    _ => None,
                }
            }
        }
    )+};
}

impl_float_argument!(f32, f64);

/// The intents one scope accepts, as a Rust type (§4.4).
///
/// Derived by `#[derive(Intent)]` on an enum, which writes the
/// [`vocabulary`](Intents::vocabulary) a document validates against *and*
/// the [`accept`](Intents::accept) that turns a raise into a value of the
/// type — from one declaration, so the two cannot disagree about what a
/// raise means. Nothing implements this by hand; the module doc says what
/// hand-writing it was.
pub trait Intents: Sized + 'static {
    /// What this scope accepts, for the document's load-time check.
    fn vocabulary() -> Vocabulary;

    /// Turn one raise into an intent, or refuse.
    ///
    /// The derived body is a `match` on [`Raise::name`] and nothing else:
    /// every arm names one intent, opens its arguments at the arity it
    /// declares, and reads each at its declared type.
    fn accept(raise: &Raise) -> Result<Self, Refused>;
}

/// Why a raise did not become an intent.
///
/// Every arm names the intent, or — when the name is what failed — names
/// what was raised, which is §4.1's "a refusal that names the field" on the
/// write side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// Nothing publishes a vocabulary for this scope, so it accepts
    /// nothing at all.
    NothingPublished,
    /// The intent type asked for is not the one this scope published.
    WrongType {
        want: &'static str,
        got: &'static str,
    },
    /// The scope publishes no intent by this name.
    NoSuchIntent { intent: String },
    /// The raise carried the wrong number of arguments.
    Arity {
        intent: &'static str,
        want: usize,
        got: usize,
    },
    /// An argument arrived as a shape its parameter's type cannot hold.
    Parameter {
        intent: &'static str,
        parameter: &'static str,
        want: &'static str,
        got: &'static str,
    },
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refused::NothingPublished => f.write_str("accepts no intents"),
            Refused::WrongType { want, got } => {
                write!(f, "accepts `{got}`, not `{want}`")
            }
            Refused::NoSuchIntent { intent } => write!(f, "accepts no intent named `{intent}`"),
            Refused::Arity { intent, want, got } => write!(
                f,
                "accepts `{intent}` with {want} argument(s), and this raise carried {got}"
            ),
            Refused::Parameter {
                intent,
                parameter,
                want,
                got,
            } => write!(
                f,
                "accepts `{intent}`'s `{parameter}` as {want}, and this raise carried {got}"
            ),
        }
    }
}

// --- the printed form ------------------------------------------------------

impl fmt::Display for Vocabulary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("accepts {")?;
        for intent in &self.intents {
            write!(f, "\n    {intent},")?;
        }
        if self.intents.is_empty() {
            return f.write_str("}");
        }
        f.write_str("\n}")
    }
}

impl fmt::Display for Accepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        for (i, parameter) in self.parameters.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}: {}", parameter.name, parameter.kind.name())?;
        }
        f.write_str(")")
    }
}

#[cfg(test)]
mod tests;
