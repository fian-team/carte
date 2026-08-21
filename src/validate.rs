//! §4.1's two grades, and the one check that separates them.
//!
//! A provider owns the schema and a consumer owns a selection. This module
//! is what happens when the two are put side by side: every field a
//! document says it reads is looked up in the scopes that document mounts
//! under, every intent it says it raises is looked up in their
//! vocabularies, and every disagreement is graded.
//!
//! # The grades are not interchangeable
//!
//! A selection naming a field no scope provides **refuses**. The case being
//! designed for is a modder's stale expectation, and the alternative is a
//! screen that draws perfectly and shows nothing — so the answer is loud,
//! immediate, and names the field.
//!
//! Coverage drift **reports**. A screen no node reaches, a field no
//! document selects, an intent no document raises: each of those is a
//! developer mid-build, and in a modding world a provider legitimately
//! publishes intents and fields no shipped document uses. A build that
//! refused those would be a build nobody could run, and a check nobody can
//! run is a check that gets deleted rather than fixed.
//!
//! [`Finding::refuses`] is the line, and it is the same line
//! [`Drift::refuses`] draws on the write side — this module carries the
//! write side's grades through unchanged rather than re-deciding them.
//!
//! # Depth, and the honest silence
//!
//! A selection names `hud`; the helpers read `hud.clock`. Only the second
//! can be wrong about a field, so [`Validation::reads`] resolves each
//! dotted read through the provider's reflection and refuses the ones that
//! reach nothing ([`Finding::Unreached`]). What it refuses is exactly what
//! it can *see*: a read that steps into a list, a map, a union or a
//! back-edge stops being resolvable there (§4.2 — a collection is one
//! field in v1), and a read through a helper's parameter or a computed key
//! was never visible at all. Both are **silent** — not a third grade, not
//! a report. A refusal a correct document cannot avoid is a check that
//! gets switched off, which costs more than the gap it closed.
//!
//! # The unread direction
//!
//! "Provided, but read by nothing" is a question about the *whole shipped
//! set*, not about one document: a field selected by one document and not
//! by its three siblings is read. So [`Validation`] accumulates — every
//! document goes in, and [`Validation::finish`] is where the unread fields
//! and the unraised intents are worked out, over every scope any mount
//! named. celia's dead root `status` is the worked example: a fact the host
//! computes every frame, that nothing anywhere reads.
//!
//! # What this is not
//!
//! It reads no files and parses nothing. A document arrives here as a list
//! of [`Field`]s and a list of [`Declared`] raises, because the vocabulary
//! a document is written in belongs to the surface framework and this crate
//! depends on nothing (`APPLICATION.md` §2). The conversion — and the
//! harness that walks a repo's shipped documents at `cargo test` time —
//! live on the surface side of the seam, where the parser does.

use std::collections::BTreeSet;
use std::fmt;

use crate::intent::{Declared, Drift};
use crate::schema::{Difference, Field, Kind, Mismatch};
use crate::store::{Scope, Store};

// --- what a check found ----------------------------------------------------

/// One disagreement between what a document says and what the store
/// publishes.
///
/// Every variant names the offender — the field, the intent, the screen —
/// because a refusal that says only "the document is wrong" is the failure
/// this whole contract exists to move earlier.
#[derive(Clone, Debug, PartialEq)]
pub enum Finding {
    /// The document selects a field none of the scopes it mounts under
    /// provides. **Refuses** — the modder's case.
    Unprovided {
        document: String,
        field: String,
        scopes: Vec<Scope>,
    },
    /// A scope provides the field under another shape. **Refuses**, and the
    /// [`Mismatch`] names the dotted path down to where the two stopped
    /// agreeing (§4.7: structurally, never nominally).
    Shape {
        document: String,
        scope: Scope,
        at: Mismatch,
    },
    /// More than one scope in the mount provides this field; the nearer one
    /// wins. **Reports** — untold_lore's app-global `heading` and the pause
    /// scope's `heading` are both legitimate, and §4.6's binding syntax is
    /// where the document says which it meant: a selection that names its
    /// scope has one provider and reports nothing.
    Shadowed {
        document: String,
        field: String,
        scope: Scope,
        by: Scope,
    },
    /// The scope provides a field no shipped document selects. **Reports**
    /// — §4.1's unread direction.
    Unread { scope: Scope, field: String },
    /// The document raises an intent none of the scopes it mounts under
    /// accepts. **Refuses** — the write side of the modder's case, and the
    /// button that draws, clicks and reaches nobody.
    Unaccepted {
        document: String,
        intent: String,
        scopes: Vec<Scope>,
    },
    /// The scope accepts the intent, and the document raises it wrongly.
    /// The [`Drift`] carries its own grade.
    Raise {
        document: String,
        scope: Scope,
        at: Drift,
    },
    /// The scope accepts an intent no shipped document raises. **Reports**.
    Unraised { scope: Scope, intent: String },
    /// The table registers an id this document draws no `screen` block for.
    /// **Reports** — table-coverage drift.
    Undrawn { document: String, screen: String },
    /// The document draws a `screen` no node reaches. **Reports**.
    Unrouted { document: String, screen: String },
    /// A mount names a scope nothing publishes — neither a schema nor a
    /// vocabulary. **Refuses**: the mapping itself is wrong, and every
    /// selection against it would be refused for the wrong reason.
    Unpublished { scope: Scope },
    /// The document reads a path *through* a name a scope provides, and
    /// the shape it provides has no field where the path goes.
    /// **Refuses** — this is [`Unprovided`](Finding::Unprovided) one level
    /// down, and it exists because a selection carries names only (§4.6):
    /// with a `host_state {}` block a provider's rename was caught by
    /// comparing two copies of the shape, and a selection has no second
    /// copy. Without this, `hud.clock` against a `Hud` that lost `clock`
    /// reads `Void` and nothing says so — a false expectation, which is
    /// the one outcome §4.1 promises never to produce.
    ///
    /// Only a read that resolves *to a leaf* can be graded. A path that
    /// steps into a list, a map, a union or a back-edge is not checkable
    /// (§4.2: a collection is one field in v1), and a computed or
    /// through-a-parameter read is not visible at all; both are silent
    /// rather than reported, because a false refusal is worse than the
    /// gap being closed.
    Unreached {
        document: String,
        /// The path as the document wrote it — `hud.clock`.
        path: String,
        scope: Scope,
        /// The dotted path down to the field that is not there, which is
        /// what §4.1 asks a refusal to name.
        field: String,
    },
    /// The document selects from a scope it does not mount under
    /// (§4.6). **Refuses** — it is the modder's stale expectation one
    /// level up from a missing field, and every field under it would
    /// otherwise be refused for a reason that does not name the cause.
    ///
    /// The scope arrives as the document's own word for it, because a
    /// name that resolves to no scope has no [`Scope`] to be.
    Unmounted {
        document: String,
        scope: String,
        scopes: Vec<Scope>,
    },
}

impl Finding {
    /// Whether this finding refuses the document or merely reports it —
    /// §4.1's two grades, and the only question a caller has to ask.
    pub fn refuses(&self) -> bool {
        match self {
            Finding::Unprovided { .. }
            | Finding::Shape { .. }
            | Finding::Unreached { .. }
            | Finding::Unaccepted { .. }
            | Finding::Unpublished { .. }
            | Finding::Unmounted { .. } => true,
            Finding::Raise { at, .. } => at.refuses(),
            Finding::Shadowed { .. }
            | Finding::Unread { .. }
            | Finding::Unraised { .. }
            | Finding::Undrawn { .. }
            | Finding::Unrouted { .. } => false,
        }
    }

    /// The document this is about, when it is about one. The unread and
    /// unraised directions are about the shipped set rather than about any
    /// single document, so they name a scope instead.
    pub fn document(&self) -> Option<&str> {
        match self {
            Finding::Unprovided { document, .. }
            | Finding::Shape { document, .. }
            | Finding::Shadowed { document, .. }
            | Finding::Unreached { document, .. }
            | Finding::Unaccepted { document, .. }
            | Finding::Raise { document, .. }
            | Finding::Undrawn { document, .. }
            | Finding::Unrouted { document, .. }
            | Finding::Unmounted { document, .. } => Some(document),
            Finding::Unread { .. } | Finding::Unraised { .. } | Finding::Unpublished { .. } => None,
        }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Finding::Unprovided {
                document,
                field,
                scopes,
            } => write!(
                f,
                "`{document}` selects `{field}`, which nothing it mounts under provides ({})",
                list(scopes)
            ),
            Finding::Shape {
                document,
                scope,
                at,
            } => write!(f, "`{document}` against {scope} — {at}"),
            Finding::Shadowed {
                document,
                field,
                scope,
                by,
            } => write!(
                f,
                "`{document}` selects `{field}`, which {scope} provides and {by} provides too; \
                 the nearer one wins, and nothing in the document says which was meant"
            ),
            Finding::Unreached {
                document,
                path,
                scope,
                field,
            } => write!(
                f,
                "`{document}` reads `{path}`, and what {scope} provides has no `{field}`"
            ),
            Finding::Unread { scope, field } => write!(
                f,
                "{scope} provides `{field}`, and no shipped document selects it"
            ),
            Finding::Unaccepted {
                document,
                intent,
                scopes,
            } => write!(
                f,
                "`{document}` raises `{intent}`, which nothing it mounts under accepts ({})",
                list(scopes)
            ),
            Finding::Raise {
                document,
                scope,
                at,
            } => write!(f, "`{document}` against {scope} — {at}"),
            Finding::Unraised { scope, intent } => write!(
                f,
                "{scope} accepts `{intent}`, and no shipped document raises it"
            ),
            Finding::Undrawn { document, screen } => write!(
                f,
                "`{screen}` is registered, and `{document}` draws no screen for it"
            ),
            Finding::Unrouted { document, screen } => {
                write!(f, "`{document}` draws a screen `{screen}` no node reaches")
            }
            Finding::Unpublished { scope } => write!(
                f,
                "a document mounts under {scope}, and nothing publishes it — neither a schema \
                 nor a vocabulary"
            ),
            Finding::Unmounted {
                document,
                scope,
                scopes,
            } => write!(
                f,
                "`{document}` selects from `{scope}`, which is not a scope it mounts under ({})",
                list(scopes)
            ),
        }
    }
}

fn list(scopes: &[Scope]) -> String {
    match scopes.is_empty() {
        true => "it mounts under nothing at all".to_string(),
        false => scopes
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Everything one check found, in both grades.
///
/// Kept together rather than split into a `Result`, because §4.1's point is
/// that the two grades travel *together*: the same run that refuses a stale
/// selection also reports the four fields nobody reads, and a caller that
/// only ever saw the refusals would never fix the drift.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Findings {
    findings: Vec<Finding>,
}

impl Findings {
    /// Everything found, refusals and reports alike, in the order the
    /// documents were checked.
    pub fn all(&self) -> &[Finding] {
        &self.findings
    }

    /// Only what refuses.
    pub fn refusals(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.refuses())
    }

    /// Only what reports.
    pub fn reports(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| !f.refuses())
    }

    /// Whether anything here refuses the documents. The one question a
    /// load asks, and the one a consumer's CI test asserts on.
    pub fn refuses(&self) -> bool {
        self.findings.iter().any(Finding::refuses)
    }

    /// Whether the documents and the store agree about everything,
    /// coverage included.
    pub fn is_empty(&self) -> bool {
        self.findings.is_empty()
    }
}

impl fmt::Display for Findings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut wrote = false;
        for (heading, mut group) in [
            (
                "refuses:",
                Box::new(self.refusals()) as Box<dyn Iterator<Item = &Finding>>,
            ),
            ("reports:", Box::new(self.reports())),
        ] {
            let Some(first) = group.next() else {
                continue;
            };
            if wrote {
                f.write_str("\n")?;
            }
            wrote = true;
            write!(f, "{heading}\n  {first}")?;
            for finding in group {
                write!(f, "\n  {finding}")?;
            }
        }
        match wrote {
            true => Ok(()),
            false => f.write_str("the documents and the store agree"),
        }
    }
}

// --- the check itself ------------------------------------------------------

/// One run of §4.1's validation over a set of documents.
///
/// It accumulates, because the unread direction cannot be answered one
/// document at a time. The shape of a use is always the same: build it over
/// the store, hand it each document's selection, raises and screens, and
/// [`finish`](Validation::finish).
///
/// The document↔scope mapping is an **input**: each call names the scopes
/// that document mounts under, nearest first. Nothing here invents it —
/// which scope a given document selects against is the binding's answer to
/// give (`APPLICATION_BUILD.md` P4), and until the binding exists the
/// consumer supplies it. Nearest-first because that is the order a mount
/// path resolves in: a view's own scope before the instance root's before
/// the process rung.
pub struct Validation<'a> {
    store: &'a Store,
    /// Every scope any mount named, so the unread direction knows which
    /// scopes to sweep. Ordered, so a report reads the same twice.
    named: BTreeSet<Scope>,
    read: BTreeSet<(Scope, String)>,
    raised: BTreeSet<(Scope, String)>,
    findings: Vec<Finding>,
}

impl<'a> Validation<'a> {
    /// Start a check against the facts one store publishes.
    pub fn new(store: &'a Store) -> Self {
        Self {
            store,
            named: BTreeSet::new(),
            read: BTreeSet::new(),
            raised: BTreeSet::new(),
            findings: Vec::new(),
        }
    }

    /// Whether the store publishes anything for this scope — a schema, a
    /// vocabulary, or both.
    ///
    /// What a caller assembling a mount's scope list asks before naming a
    /// node that may own no scope at all: most view nodes own none, and
    /// naming one that publishes nothing is
    /// [`Finding::Unpublished`] — a refusal, and the wrong answer for a
    /// screen that simply reads from the instance root above it.
    pub fn publishes(&self, scope: Scope) -> bool {
        self.store.reflection(scope).is_some() || self.store.intents(scope).is_some()
    }

    /// Check one document's selection — the fields it says it reads —
    /// against the scopes it mounts under, nearest first (§4.1, §4.2).
    ///
    /// Per-field, because per-field selection is what gives the most
    /// specific refusal (§4.2). A field no scope provides is named on its
    /// own rather than as part of "the document does not match"; a field
    /// two scopes provide is a report, because both are legitimate and only
    /// §4.6's binding syntax can say which was meant.
    pub fn selects(&mut self, document: &str, scopes: &[Scope], selection: &[Field]) {
        self.name(scopes);
        for wanted in selection {
            self.select_one(document, scopes, &wanted.name, Some(wanted));
        }
    }

    /// Check a selection that names fields and **nothing else** (§4.6).
    ///
    /// The difference from [`selects`](Self::selects) is the whole point of
    /// the inversion. A `host_state {}` block restates the provider's
    /// shapes, so a shape can be *compared* — and can drift, which is what
    /// [`Finding::Shape`] is for. A selection states only what it reads, so
    /// there is no second copy of the shape to disagree with the first: the
    /// field is either provided or it is not, and if it is, its shape is
    /// the provider's by construction.
    ///
    /// Everything else is the same question, asked the same way, and
    /// answered in the same two grades: a name nothing provides refuses and
    /// is named; a name two scopes provide reports; a name that is provided
    /// is read, so it is not also unread.
    pub fn selects_named(&mut self, document: &str, scopes: &[Scope], names: &[String]) {
        self.name(scopes);
        for name in names {
            self.select_one(document, scopes, name, None);
        }
    }

    /// One field of a selection, against the scopes nearest first.
    ///
    /// `wanted` is `Some` only when the document restated the shape, which
    /// only a `host_state {}` block does.
    fn select_one(&mut self, document: &str, scopes: &[Scope], name: &str, wanted: Option<&Field>) {
        let wanted_name = name;
        {
            let mut providers = scopes.iter().filter(|scope| {
                self.store
                    .reflection(**scope)
                    .is_some_and(|kind| kind.field_at(wanted_name).is_ok())
            });
            let Some(scope) = providers.next().copied() else {
                self.findings.push(Finding::Unprovided {
                    document: document.to_string(),
                    field: wanted_name.to_string(),
                    scopes: scopes.to_vec(),
                });
                return;
            };
            for by in providers {
                self.findings.push(Finding::Shadowed {
                    document: document.to_string(),
                    field: wanted_name.to_string(),
                    scope,
                    by: *by,
                });
            }
            let provided = self
                .store
                .reflection(scope)
                .and_then(|kind| kind.field_at(wanted_name).ok())
                .expect("the provider was just found");
            // One field against one field, so the mismatch's path is rooted
            // at the field's own name — which is what §4.1 asks a refusal
            // to say. Presence, at-mount value and grain are the provider's
            // own declarations and are deliberately not compared (§4.7).
            if let Some(wanted) = wanted {
                let want = Kind::Record(vec![wanted.clone()]);
                let got = Kind::Record(vec![provided.clone()]);
                if let Err(at) = want.compare(&got) {
                    self.findings.push(Finding::Shape {
                        document: document.to_string(),
                        scope,
                        at,
                    });
                }
            }
            // A dotted path reads its root field; nothing below the root is
            // separately subscribable, so nothing below it can be unread.
            let root = wanted_name.split('.').next().unwrap_or(wanted_name);
            self.read.insert((scope, root.to_string()));
        }
    }

    /// Check the paths a document reads *through* the names a selection
    /// bound — §4.1's guarantee at leaf depth.
    ///
    /// [`selects_named`](Self::selects_named) answers "is `hud` provided?"
    /// and stops, because that is all a selection says. This answers "and
    /// is there a `clock` on it?", which is the question a `host_state {}`
    /// block used to answer by restating the shape. `paths` are dotted
    /// reads off names *this* selection bound, nearest-first over the same
    /// scopes, so a read and the selection that bound it can never
    /// disagree about which provider was meant.
    ///
    /// Three outcomes, and two of them are silence:
    ///
    /// - the path resolves to a leaf — nothing to say;
    /// - the path resolves as far as a list, a map, a union or a
    ///   back-edge and stops there — **silent**, because §4.2 makes a
    ///   collection one field and what is inside one is not checkable;
    /// - the path names a field the shape does not have — **refuses**,
    ///   naming it.
    ///
    /// A root no scope provides is silent too: the selection that bound
    /// it has already refused, named, and a second complaint about the
    /// same name would bury the first.
    pub fn reads(&mut self, document: &str, scopes: &[Scope], paths: &[String]) {
        for path in paths {
            let Some((root, _)) = path.split_once('.') else {
                continue;
            };
            let provider = scopes.iter().copied().find(|scope| {
                self.store
                    .reflection(*scope)
                    .is_some_and(|kind| kind.field_at(root).is_ok())
            });
            let Some(scope) = provider else { continue };
            let Some(reflection) = self.store.reflection(scope) else {
                continue;
            };
            let Err(at) = reflection.field_at(path) else {
                continue;
            };
            if *at.difference() != Difference::Missing {
                // The path left the records — §4.2's collection, a union's
                // live variant, a back-edge. Not checkable to the leaf,
                // and saying so anyway would refuse correct documents.
                continue;
            }
            self.findings.push(Finding::Unreached {
                document: document.to_string(),
                path: path.clone(),
                scope,
                field: at.field().to_string(),
            });
        }
    }

    /// The document selects from a scope name that is not on its mount
    /// (§4.6) — the refusal one level up from a missing field.
    pub fn unmounted(&mut self, document: &str, scope: &str, scopes: &[Scope]) {
        self.name(scopes);
        self.findings.push(Finding::Unmounted {
            document: document.to_string(),
            scope: scope.to_string(),
            scopes: scopes.to_vec(),
        });
    }

    /// Check one document's raises against the same scopes (§4.4).
    ///
    /// Positional, because a document's `events {}` block names no
    /// parameters. The first scope that accepts the name is the one the
    /// raise is checked against, which is the same nearest-first rule the
    /// read side follows.
    pub fn raises(&mut self, document: &str, scopes: &[Scope], declared: &[Declared]) {
        self.name(scopes);
        for raise in declared {
            let accepting = scopes.iter().find(|scope| {
                self.store
                    .intents(**scope)
                    .is_some_and(|vocabulary| vocabulary.intent(&raise.name).is_some())
            });
            let Some(scope) = accepting.copied() else {
                self.findings.push(Finding::Unaccepted {
                    document: document.to_string(),
                    intent: raise.name.clone(),
                    scopes: scopes.to_vec(),
                });
                continue;
            };
            let vocabulary = self.store.intents(scope).expect("the scope was just found");
            for at in vocabulary.check_one(raise) {
                self.findings.push(Finding::Raise {
                    document: document.to_string(),
                    scope,
                    at,
                });
            }
            self.raised.insert((scope, raise.name.clone()));
        }
    }

    /// Check one document's `screen` blocks against the ids the table says
    /// that document draws.
    ///
    /// Both directions report (§4.1): a screen nobody routes to and a route
    /// nobody draws are each table-coverage drift, and a game that would
    /// not boot over a screen it has not routed yet is a game whose check
    /// gets deleted.
    pub fn draws(&mut self, document: &str, declared: &[&str], registered: &[&str]) {
        for screen in registered {
            if !declared.contains(screen) {
                self.findings.push(Finding::Undrawn {
                    document: document.to_string(),
                    screen: (*screen).to_string(),
                });
            }
        }
        for screen in declared {
            if !registered.contains(screen) {
                self.findings.push(Finding::Unrouted {
                    document: document.to_string(),
                    screen: (*screen).to_string(),
                });
            }
        }
    }

    /// Finish, adding the two directions that are about the shipped set
    /// rather than about any one document: a field provided and read by
    /// nothing, and an intent accepted and raised by nothing.
    pub fn finish(mut self) -> Findings {
        for scope in self.named.clone() {
            let reflection = self.store.reflection(scope);
            let vocabulary = self.store.intents(scope);
            if reflection.is_none() && vocabulary.is_none() {
                self.findings.push(Finding::Unpublished { scope });
                continue;
            }
            if let Some(Kind::Record(fields)) = reflection {
                for field in fields {
                    if !self.read.contains(&(scope, field.name.clone())) {
                        self.findings.push(Finding::Unread {
                            scope,
                            field: field.name.clone(),
                        });
                    }
                }
            }
            if let Some(vocabulary) = vocabulary {
                for intent in vocabulary.intents() {
                    if !self.raised.contains(&(scope, intent.name.clone())) {
                        self.findings.push(Finding::Unraised {
                            scope,
                            intent: intent.name.clone(),
                        });
                    }
                }
            }
        }
        Findings {
            findings: self.findings,
        }
    }

    fn name(&mut self, scopes: &[Scope]) {
        self.named.extend(scopes.iter().copied());
    }
}

#[cfg(test)]
mod tests;
