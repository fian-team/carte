# Carte

**A router and scoped store for Rust applications.**

Carte answers what is on screen and what is true about it. You register
your screens in a route table, and every frame the router works out the
path from your application's own state: nothing pushes or pops screens.
A node can guard its door with a refusal written as a sentence, and it
decides what Escape means. Each screen's facts live in a scope that is
mounted when its node joins the path and dropped when it leaves. Your UI
never writes those facts directly. It asks by pushing actions onto an
outbox, and the store applies them, runs your producers and commits once
per frame, so no consumer ever sees half a frame. Scopes and the intents
they accept describe themselves, so a UI's reads and raises can be checked
against them before anything draws. The crate has no dependencies.

> **Status:** pre-1.0 and not yet on crates.io. The API changes without
> deprecation cycles.

## Where Carte fits

- **Game screen flow.** Title, pause, settings, an exit prompt over the
  workspace it is leaving, a door that refuses with a reason. Instance
  roots carry an area, so music and backdrops read the same path the UI
  does.
- **Tool and editor apps.** Modal prompts over an editor that keeps its
  draft, and one screen (settings) reachable from two parents without
  knowing which one it is under.
- **Any UI toolkit.** The router and store name no widget type. The
  `notes` example draws with egui, and `web/` runs the same app under a
  Masque document.
- **Checking UI against data under `cargo test`.** A document that reads
  a field nothing provides is refused by name. A screen no node reaches,
  or a field or intent nothing uses, is reported.

The store holds facts about what should be presented. It does not hold
your simulation, and it does not hold bulk data like meshes or tiles
(`docs/APPLICATION.md` §5.2). Carte is also not a URL router: there is no
history stack, because the path is derived rather than navigated.

## Getting started

Add the crate:

```toml
[dependencies]
carte = { git = "https://github.com/fian-team/carte" }
```

Carte's `[dependencies]` is empty and `tests/independence.rs` keeps it
that way, so it adds nothing to your build but itself.

The fragments below come from `examples/notes.rs`, a five-screen notes
app. Run it with:

```sh
cargo run --example notes
```

Start with the table. It is the static shape of your screens, validated
at startup:

```rust
use carte::{RouteTable, Refusal, Scope, Store};

fn table() -> RouteTable {
    let mut table = RouteTable::new();
    table
        .at_root("home")
        .under("note", "home")
        .under("discard", "note")
        .under("settings", "home")
        .under("settings", "note") // one screen, two parents
        .under("trash", "home")
        .guard("trash", trash_has_something);
    table
}

fn trash_has_something(store: &Store) -> Result<(), Refusal> {
    match store.read::<Shared>(Scope::Process) {
        Some(library) if !library.trashed.is_empty() => Ok(()),
        _ => Err(Refusal::new("The trash is empty.")),
    }
}
```

Each id resolves to a `Node`. A node answers which child it is showing,
reading your application state (the router's `Cx`), and what Escape
means here:

```rust
impl Node<Model, Action> for NoteScreen {
    fn resolve_child(&self, model: &Model) -> Option<RouteId> {
        model.note_child
    }
    fn escape(&mut self, model: &Model, out: &mut Outbox<Action>) -> Escape {
        match model.dirty() {
            true => {
                out.push(Action::AskDiscard);
                Escape::Prompt
            }
            false => Escape::Pop,
        }
    }
    // occludes, update, enter, leave, child_popped, depart, ...
}
```

Build the store over the same table, declare what each scope provides,
and claim the fields each producer writes:

```rust
let table = table();
let mut store = Store::over(&table);
store.provides::<Shared>(Scope::Process)?;            // lives for the whole process
store.provides::<Editor>(Scope::Node("note"))?;       // lives while `note` is on the path
let prefs = store.producer::<Shared>(Scope::Process, &["dark"])?;

let routes: Vec<(RouteId, Box<dyn Node<Model, Action>>)> = vec![
    ("home", Box::new(Home)),
    ("note", Box::new(NoteScreen)),
    // ...
];
let mut router = Router::new(table, routes, |_: &Model| "home")?;
let mut outbox = Outbox::new();
```

Then, each frame, draw from committed state and settle:

```rust
// Draw: read committed facts, and the views the router says are visible.
for id in router.visible_views() { /* draw screen `id` */ }
let editor = store.read::<Editor>(Scope::Node("note"));

// Input becomes actions; Escape goes to the deepest route first.
outbox.push(Action::Open(0));
let outcome = router.escape(&model, &mut outbox);

// Settle: drain the outbox, apply, produce, commit. Then re-derive the path.
let notice = store.tick(&mut outbox, |actions, barrier| {
    for action in actions {
        model.apply(action);
    }
    let mut w = barrier.writer(&prefs).expect("process scope");
    set!(w, dark, model.dark).expect("claimed");
});
if router.resolve(&mut model, &mut store) {
    // The path moved: tick again so a scope that just mounted is filled
    // before it is drawn.
}
```

`notice` lists the fields that changed and the subscribers they woke, once
per tick. In `web/`, the same app is driven by a Masque document and
compiled to WebAssembly. It has its own workspace, so Masque never enters
Carte's dependency tree.

## Tour

The rules each section cites are in
[`docs/APPLICATION.md`](docs/APPLICATION.md).

### The table is a graph, and the path is a walk

A route table is a DAG of ids. Every node has a tier: an **instance root**
mounts its own document, a **view** selects a screen inside the enclosing
instance, and a **structural** node mounts nothing and exists to own a
scope that outlives its children. Instance roots can stand in an **area**,
the ground their music and backdrop read (§3.2).

```rust
table
    .at_root("title").mounts("title", "ui/title.masque").in_area("title", "front")
    .at_root("world").mounts("world", "ui/world.masque").in_area("world", "world")
    .under("pause", "world")
    .under("settings", "title")
    .under("settings", "pause")
    .default_area("front");
```

The path is not stored. Every frame the router asks the root resolver
where to start, then asks each node on the path for its child, and stops
at the first node that claims none. `resolve_child` returns an `Option`,
so a node claims at most one child and the walk needs no arbiter (§3.8).
To change the path, change the state a resolver reads, by way of an
action (§3.3).

### Lifecycle and route state

`enter` and `leave` mean the id joined or left the path, not that it
became or stopped being the deepest. A prompt opened over an editor
leaves the editor's transaction standing. Anything that must outlive the
route it is edited in belongs in `Cx`, not in the route (§3.9).

### Guards

A guard is an ordinary function over the store. Before the walk
descends onto a child it asks that child's guard, and a refusal stops the
descent. `Router::ask` evaluates the same guard ahead of time, so a
button can gray itself and show the same sentence (§3.4).

```rust
if let Err(why) = router.ask("trash", &store) {
    // "The trash is empty."
}
```

### Escape and departure

Escape goes to the deepest route first. It answers `Pop`, `Prompt`
(raise a child prompt instead of leaving), `Ignore` (keep the key, do
nothing) or `Fall` (offer it to the parent). `router.escape` reports what
happened, and `Unclaimed` is the only case in which the host hears about
Escape at all.

A route also says how it leaves the frame: `Departure::Cut`, or
`Departure::Bleed { seconds }` to hold its last frame under the incoming
one while the host plays a transition.

### Occlusion and stacked views

`Occlusion` is a ladder. `None` lets the screens behind render too (a
prompt over the workspace it is leaving). `View` lets ancestors draw but
not their screens (most children). `Surface` hides everything (a node
that brings its own world). `router.drawing()` lists which routes draw,
and `router.visible_views()` lists which screens render, deepest last.
That list holds two ids when a prompt stacks over its parent.

### Scopes

A scope is a Rust struct keyed by a `Scope`. `Scope::Process` is mounted
once and never dropped. `Scope::Node(id)` is mounted at its declared
at-mount values when `id` joins the path, and dropped when it leaves.
Nothing else mounts a scope (§5), so state cannot outlive its screen by
accident.

Consumers read in one of two ways (§5.1). **Read** is pull: `store.read`
hands back `&T`, and `store.version` says cheaply whether it moved.
**Subscribe** is push: register a `SubscriberId` against a field, and the
tick's `Notice` says when to wake it. The store never calls back into
your code.

### Writes are actions

Each field has exactly one writer, claimed at startup with
`store.producer`. Two producers claiming one field is a startup error.
Writes only happen inside `store.tick`, in a pinned order: drain the
outbox, run producers, commit (§5.4). An action raised this tick lands in
this tick's commit, and ten sets are one notification.

Every set is equality-checked, and a set that changes nothing is
swallowed (§5.3). `set!` ties the schema's field name to the struct's
field in one token, and `set_num!` floors a number to the grain the schema
declares, so a per-frame float doesn't wake every subscriber (§5.5).

```rust
set!(editor, words, model.draft.split_whitespace().count())?;
```

### Schemas and intents

A scope's schema describes itself as a `Kind`, and that reflection is
what a document's selection is checked against (§4.1, §4.3). A scope
publishes the intents it accepts the same way (§4.4). The derives live in
lorekeeper's `editable-derive`, which names this crate only in the code it
generates:

```rust
#[derive(Schema, Clone, Debug, Default, PartialEq)]
pub struct SettingsState {
    pub options: Vec<SettingRow>,
}

#[derive(Intent, Clone, Debug, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SettingsIntent {
    Setting(String),
}

store.provides::<SettingsState>(Scope::Node("settings"))?;
store.accepts::<SettingsIntent>(Scope::Node("settings"))?;

// A named raise from the UI, validated and landed on the outbox as an action:
store.raise::<SettingsIntent, Action>(Scope::Node("settings"), &raise, &mut outbox)?;
```

Without the derive, implement `Schema` by hand. `examples/notes.rs` does
this for both of its scopes.

### Validation

`Validation` puts what documents say next to what the store publishes,
and grades each mismatch (§4.1). Reading a field nothing provides, or
raising an intent nothing accepts, **refuses**. A field no document
reads, or an intent no document raises, **reports**. Feed in every
shipped document, then call `finish`:

```rust
let mut check = Validation::new(&store);
check.selects_named("settings.masque", &[Scope::Node("settings")], &selected);
check.raises("settings.masque", &[Scope::Node("settings")], &declared);
let findings = check.finish();
assert!(!findings.refuses(), "{findings:?}");
```

Carte reads no files. The harness that parses a repository's documents
and feeds them in lives on the surface side, in lorekeeper's `contract`
crate.

The full rules, with the reasoning and the failure each one prevents, are
in [`docs/APPLICATION.md`](docs/APPLICATION.md). The integration guide is
in [`AGENTS.md`](AGENTS.md).

## Related projects

- **[Masque](https://github.com/fian-team/masque)** is a UI language for
  Rust applications. It and Carte are deliberately independent: neither
  depends on the other.
- **[Lorekeeper](https://github.com/fian-team/lorekeeper)** is the engine
  that joins them. Its `driver` crate mounts Masque documents under Carte
  routes, and its `contract` crate checks a document's `select` and
  `events {}` against the store's schemas under `cargo test`. Its
  `editable-derive` crate provides `#[derive(Schema)]` and
  `#[derive(Intent)]`.
- **Untold Lore** is a 3D game whose client routes its screens through
  Carte, and is one of the projects that shapes it.

## License

MIT. See [`LICENSE`](LICENSE).
