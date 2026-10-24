# Carte — The Rules

> Carte's part of the application model: the routing axioms (§3), the
> contract a scope publishes (§4) and the store (§5). They moved here on
> 2026-10-24 from Masque's design record (`masque/docs/internal/APPLICATION.md`).
> The numbers did not change, so a §3–§5 citation means the same thing in
> either repository. The rest of that record stays in Masque: why the split
> happened (§1), the binding (§6), what the split dissolves in the
> consumers (§7), the open questions (§8) and the precedents (§9). A
> citation of one of those from here says "Masque's".
>
> The build plan these sections were written against is
> `masque/docs/internal/APPLICATION_BUILD.md`, and its work-package
> numbers (WP-2.2, P4, Appendix A) are what this crate's doc comments
> mean by them.

---

## 1. What Carte is

Carte is routing fused with a scoped, schema'd state store: what exists,
what is true about it, and who is told when it changes. It is one of two
frameworks composed by a thin binding. The other is the surface
framework, Masque, which decides how named surfaces look given validated
selections of state. The binding mounts instances, delivers frames and
performs the crossing. In a game that binding is lorekeeper's `driver`
crate.

The name was chosen with Masque's, as a pair that reads as *structure*
vs *surface*: a map and a mask. Until then the crate's working name was
`structure` (Masque's §8.4 records the criteria).

## 2. The seam

The engineering guarantee is a dependency edge: **Carte depends on
nothing of the surface framework, and vice versa; only the binding
depends on both.** Cargo enforces it. Carte's `[dependencies]` is empty,
so `cargo tree -p carte` shows nothing at all, and
`tests/independence.rs` fails the build if a shipped table ever names a
dependency, or any table names Masque or the crates that bind it.

The same edge holds from the other side. The surface framework's
route-facing constructs (`screen`, `outlet()`, selections) take *names*
— strings and validated field paths — never Carte's types. So Carte
publishes everything it knows in a vocabulary of its own: route ids are
`&'static str`, a schema reflects itself as a `Kind`, and a raise
arrives as a name and a list of `RaiseArg` data. What a document is
written in belongs to the surface framework, and converting between the
two is the binding's job.

## 3. Axioms — Carte

### 3.1 Composed, not integrated

Two frameworks with a seam, not
one framework with a router at its center. The decisive argument is
the mod surface: the route table and schemas are compiled Rust the
developer owns; documents are the hot-reloading consumer surface
modders touch. Integration would force that line to be drawn
*inside* one framework, where it will smear.
*Drift indicator:* a surface-framework crate importing a Carte
type; a route decision made by document code.

### 3.2 One path space; a node is an instance root or a view

The
application has a single route table (a DAG; a path is a walk,
derived per frame by `resolve_child` — no push/pop stack). Every
node declares its tier: an **instance root** mounts its own
document (menu, world, library — crossing one is teardown/mount
with a marked passage), a **view** selects a screen/outlet within
the enclosing instance (main menu → worlds list is two views in one
instance). Music, backdrops, session lifecycle and the UI read the
same path.
Three refinements from the consumer audit. A node may instead be
**structural** — mounting nothing, selecting nothing — existing to
own a scope whose lifetime spans its children (celia's `session`
node holds the focused character precisely because that fact must
outlive both the lobby and the arena). An instance root may carry
an **area** attribute — the ground its music and backdrops stand
on; untold_lore's `side_of` becomes this attribute, *not* the tier
itself (its four roots share three areas, and roster → world must
not read as a marked passage). And because the table is a DAG, one
node may have parents in several instances (settings under title
and under pause); the screen it selects resolves against the
*enclosing* instance's document.
*Drift indicator:* a host hand-driving a second document "exactly
as the router drives the first" (untold_lore's `project_root` was
the smell); an instance-boundary fact (`side_of`) living in game
code.

### 3.3 The router derives; hosts apply

A route's only way to
change the world is an action on the outbox, applied by the host's
services. Transitions that boot servers or tear down sessions are
host reactions to a path change, never route-tier behavior. (This
is the half of untold_lore's crossing axiom 4 (`ul-client/src/crossing.rs`) that survives; the half that
falls is "the route tier knows nothing of areas.")
The converse also holds and is not a violation: some hosts react
to facts *upstream* of the path — a server-authoritative game
gates its simulation on the session's stage, from which the path
itself derives (celia's fight tick must keep reconciling under
`[arena, pause]`). The path is the central derived fact of
*presentation*; it is not always the source, and re-gating such a
host on the path would be wrong-way dataflow.

### 3.4 A node guards its own door

A node may declare
preconditions over store fields; a precondition that fails is a
machine-readable **refusal** — a sentence, written once and read
everywhere it surfaces (the panel row that grays, the toast that
explains). Ask-then-mount survives from ROOMS.md: entering is a
request the guard rules on, never a fait accompli. Without this,
every game keeps a shadow rooms table on its side of the seam.
*Drift indicator:* a `can_enter_x` boolean in a scope schema; a
consumer composing its own refusal sentence.

### 3.5 The path is the only answer to what is on screen, and the session is not a route

One path, one derivation, read by the UI, the music, the backdrops and
the cursor alike. No second vocabulary stands beside it: not a `mode`
string in a document, not a screen enum in a host, not a stance boolean
on a game. And the connection, the snapshot pump, the hosted server and
the autosave tick every frame whatever the path is, so "opening a screen
stopped the client reading the wire" is unrepresentable rather than
repeatedly fixed.
*Drift indicator:* a field answering "what is up" that is not the path;
a service ticked from a route's `update`.

### 3.6 A surface is a route if and only if it takes input exclusively

Everything else is decoration: ordinary root-scope state with no node,
no lifetime and no exclusion, free to co-occur. A toast, a tip, a notice
and a lightning flash are decoration; settings, pause, a journal, a
wardrobe and an exit prompt are routes. This is the test that keeps the
table small and keeps orthogonal overlays out of it.
*Drift indicator:* a node for something that never takes the keyboard;
a toast queue modelled as a stack of routes.

### 3.7 A game adds routes, never lifecycle states

The one enumeration a game may not extend is the session lifecycle
(`SessionLevel`): a state that gates one player's entry on another's
readiness is a lobby, whatever it is called. Locking a surface list was
only ever a proxy for locking this, and it cost a string side-channel to
work around. A game adds any route it likes.
*Drift indicator:* a fourth session level; a "waiting" screen that holds
a connected player.

### 3.8 A node claims at most one child

`resolve_child` returns an `Option`, so the walk is total and ordered
with no arbiter above it: a node either claims one child or ends the
path. That is the whole of "by construction": n independent booleans
admit 2ⁿ states of which n+1 are legal, and every entry point has to
re-assert the invariant; one `Option<RouteId>` cannot hold two values.
Three things follow. The precedence ladder is local, about ten lines
beside the state it reads. A prompt is resolved by the node it is a
prompt for, where "am I leaving" already lives. And a game's root
resolver shrinks to its lifecycle.
*Drift indicator:* a pairwise exclusion list; a resolver that reads a
sibling's state to decide its own.

### 3.9 Route state is what dies on `leave`

The test is lifetime, not ownership. Anything that must outlive the
route it is edited in is session state and lives in `Cx`: a half-typed
journal entry survives closing the journal, a wardrobe draft survives
the stance, and a world view that four routes touch is nobody's route
state. A half-typed address on the connect screen is the route's, and
dies with it. `enter` and `leave` mean the id entered or left the path,
not that it stopped being deepest, so a prompt raised over a workspace
leaves the workspace's transaction standing. Reads are shared and writes
are an outbox: `&Cx` for `update`, `event` and `escape`, so a route
cannot mutate the session and can only ask, and interior mutability is
ruled out as the alternative. If two routes touch it, it is not route
state.
*Drift indicator:* a route holding something a sibling reads; `RefCell`
in a route; a draft that resets when a child opens over it.

## 4. Axioms — the contract (provider-owned, GraphQL-shaped)

### 4.1 The provider owns the schema; the consumer owns a selection

Each scope's schema is a Rust type declared with the
route node that provides it. A document declares what it *consumes*
— the `host_state {}` block survives with its meaning inverted,
from "here is what exists" to "here is my selection." Validation
runs at document load **and at every hot reload**: a selection
naming a field that does not exist is a load-time refusal that
names the field. A consumer edit can produce a loud, immediate,
named error — never a false expectation. Optionality is stated in
the schema ("this may be absent while X"), not by convention. So
are **defaults**: a field's at-mount value is declared, because a
silent zero-default can render as an invisible chrome
(untold_lore's `launch_fade` lesson). Validation has two grades: a
selection naming a missing field **refuses** (the modder's case —
loud, immediate, named), while table-coverage drift (a screen no
node reaches, a published intent no shipped document raises)
**reports** without refusing — the second is a developer mid-build,
and in a modding world a provider legitimately publishes intents
no shipped document uses. A hot-reload refusal rejects the new
document and names the field without tearing down the running
instance.

### 4.2 Selection is per-field

Per-field selection gives the most
specific refusal (the failure being designed against is a modder's
stale expectation), the finest invalidation, and per-field diffing.
Collections (`Vec<Row>`) are one field in v1 — a row change
invalidates the list's listeners. That coarseness is accepted and
named; the future answer is keyed collections (GraphQL-list /
React-key shaped), and nobody patches around it inside a consumer.
untold_lore hits this coarseness on day one (`entries`, the
wardrobe tree; regency's debug rows), so keyed collections are a
fast follow, not a someday.

### 4.3 One schema, derived reflection

Typed consumers (Rust) read
the schema struct directly at compile time; dynamic consumers
(documents) are validated against a *reflection* of it. The
reflection is **derived** (a derive macro — the move
`#[derive(Editable)]` already proved), never hand-maintained. This
is what makes the untold_lore triple-list (struct ↔ `.masque` ↔
projection, guarded by `every_declared_key_is_projected`)
unrepresentable rather than merely guarded: the struct *is* the
schema, and the guard tests delete.

### 4.4 Raises are the same contract, write-side

A scope publishes
the intents it accepts exactly as it publishes the fields it
provides; a document's raises validate at the same load. The
two-way raise/handler guard test becomes structural and deletes.

### 4.5 The mod surface is the consumer side, with a provider-side dial

Consumers cannot amend contracts, period. What schema a
consumer class *sees* is a provider decision (the SQL grants/views
idea): shipped documents and third-party mods need not see the same
schema, and the difference is declared, not an honor system. The
moment third-party documents exist, the schema is a public API and
its evolution policy must be stated explicitly — even if the
statement is "mods pin a game version; removals break loudly at
load." (Open, Masque's §8.)

### 4.6 A selection binds top-level names

A document's selection
binds the fields it selects to top-level identifiers, so a helper
reads `hud.clock` exactly as it did when the names were declared
locally. This is a migration property, deliberately: with it,
regency's document split touches zero helper bodies; without it,
the split is a three-thousand-line touch of every function in the
file. The binding syntax is part of the contract, not sugar.

### 4.7 A shared module selects against more than one scope

A
module mounted under two providers (untold_lore's sea panel lives
under the world root and under the editor) states its selection
once, as a **fragment**, and the fragment validates against each
providing scope at each mount. Validation is structural —
field-by-field against the derived reflection — never nominal,
because names cannot cross the Rust/document boundary; a record
whose field went missing refuses at load like any other selection
error, including the engine-published record shapes a game's
document consumes (Masque's §8.2).

## 5. Axioms — the store

### 5.1 Two consumption verbs

GraphQL has queries *and*
subscriptions; so does this protocol, from day one. **Subscribe**:
push, sparse, per-field notification, selective rerender — the
document idiom. **Read**: pull, per-frame, bulk, over the same
validated selection — the renderer idiom. A consumer declares which
it is. "Protocol" means *contract*, not serialization: an in-process
typed consumer pays direct field reads and a version check, not
dispatch through a map.

### 5.2 The store/simulation fence

The store carries **facts about
what should be presented** — the path, frame parameters (camera
config, lens, submersion, exposure, backdrop side, time of day),
projected view rows — and **never the presentation's bulk data**.
Meshes, bone buffers, terrain tiles, fifty thousand scattered rocks
flow through their own derivation disciplines; the renderer
consumes the store for parameters and the world for geometry. This
axiom carries "no lobby" force, because the failure mode — the
store becomes the god object, now with a protocol — is the 2026-08
audit's findings reincarnated one abstraction up. "Everything is a
consumer" is an attractor; this is the fence.
*Drift indicator:* per-frame bulk data (a vertex list, a tile, a
pose) appearing as a schema field; a consumer reading the store
inside a hot loop it previously read a snapshot in.

### 5.3 Writes are actions; sets are equality-checked

The store is
single-writer per field: producers (the host's services tier) set;
consumers read and subscribe; everything else goes through the
outbox. The store equality-checks every set and swallows no-ops —
which is lorekeeper INTENT §22 ("dirtiness is derived from values,
never remembered by flags") implemented once, centrally, replacing
every hand-rolled diff idiom (`drive_music`, `backdrop_showing`,
`drive_crossing` are all bespoke instances). The one exception
survives from the audio lesson: state living *outside* the process
(an audio bus, the OS cursor) is still diffed against a read-back
of the world, because the store can only be authoritative for what
it owns.

### 5.4 Frame-transactional commit

Writes accumulate during a tick
and commit at a frame barrier; notifications are batched and
delivered once per frame; consumers rerender after commit. No
consumer may observe field A new and field B old from the same
tick (the classic FRP glitch), and ten sets in one tick are one
rerender, not ten. This is an axiom of the protocol because it is
unrecoverable as a retrofit.
Within the tick the order is pinned: outbox drain, then producer
sets, then the barrier — an intent raised this tick lands in this
tick's commit, so a click's echo costs zero frames (celia's tile
focus is the regression test for this sentence). Producers run
inside the barrier and read working state; consumers only ever
see committed state.

### 5.5 Facts, not presentation

Eased values, glides, presences —
easing lives in consumers; the store carries the fact being eased
toward. (The existing tick/draw and client-local-presentation
disciplines, inherited unamended.) And producers set facts at the
granularity consumers act on: the sky clock floors to the minute,
a countdown to the second — a raw per-frame float defeats the
equality check and wakes every subscriber every frame.
Quantization is the producer's duty, and a schema may declare a
field's grain.

### 5.6 A frozen consumer is the ghost

A departing instance is an
unsubscribed-but-retained consumer: it keeps drawing its
last-delivered values until the binding drops it. The crossing's
freeze rules (untold_lore's `ul-client/src/crossing.rs`, section 3) stop being special machinery and
become what unsubscription *is*. Two riders from the audit: a
ghosted instance retains its draw slot (Masque's §6.2) until dropped — the
world must keep painting under the departing document; and an
instance whose backdrop is host-painted ghosts as a **captured
composite frame**, because its painter cannot re-run against
torn-down simulation state (celia's arena; `Departure::Bleed`'s
existing semantics, promoted to the instance tier).

### 5.7 Derivation is a producer

Derived facts — view rows, joins
over the snapshot, an item card assembled from three panes — are
ordinary producers subscribed to the fields they derive from,
running inside the same tick's barrier (§5.4), so no consumer ever
sees a torn derivation. This is where untold_lore's `reproject`
joins go: they do not dissolve, they relocate and gain a trigger.
If producer-on-producer ordering ever grows hair, the answer is a
declared derived field (the signals family), never ad-hoc
sequencing inside a consumer.

