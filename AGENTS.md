# AGENTS.md

## Project Overview

Carte is a router and scoped store for Rust applications. It provides:

- A route table: a DAG of ids with tiers (instance root, view,
  structural), areas, occlusion and guards, validated at startup.
- A router that derives the path every frame by walking the table, runs
  lifecycle, arbitrates Escape by depth and reports departures.
- A store of scoped facts whose lifetime is node presence on the path,
  written only through producers inside a frame-transactional tick.
- Schema reflection and intent vocabularies, so a consumer's reads and
  raises can be validated against what a scope publishes.
- An outbox: the only way a route or a UI changes anything.

It is used in three ways:

1. **Library**: embedded in a Rust application under any UI layer (the
   primary use case).
2. **Example**: `cargo run --example notes` runs a five-screen notes app
   over egui with an inspector panel showing the router and the store.
3. **Web stage**: `web/` runs the same notes app under a Masque document,
   compiled to `wasm32-unknown-emscripten` for the website. It is its
   own workspace, and its native tests (`cd web && cargo test`) are the
   check that matters here.

### Carte depends on nothing, and never on Masque

Carte is one of two independent libraries. The other is Masque, the UI
language at `../masque`. `[dependencies]` here is empty and
`cargo tree -p carte` shows nothing at all. `tests/independence.rs` fails
if a shipped dependency table names anything, or if any table, including
dev-dependencies, names `masque`, `contract` or `driver`. Keep it that
way. The example's UI toolkit is a dev-dependency, and it must not be
Masque.

Everything that joins the two lives in lorekeeper:

- `lorekeeper/driver` is the binding. `driver::route` holds what stayed
  behind when the router moved here, because its signatures name surface
  types: the `Route` trait, `RouteEvent`, `Chrome`, and the router's
  `event`/`draw` dispatch. It mounts instances, delivers frames and
  performs the crossing.
- `lorekeeper/contract` is the document contract: a document's `select`
  and `events {}` parsed and fed to `Validation`, plus the `Documents`
  harness a consumer runs under `cargo test`.
- `lorekeeper/editable-derive` provides `#[derive(Schema)]` and
  `#[derive(Intent)]`. A proc macro emits paths, so it names this crate
  only in the code it writes, never in its `Cargo.toml`.

What Carte offers the binding is plain public API that names no surface
type: `RouteId` as `&'static str`, `RaiseArg` as data, `Kind` as the
reflection, and `Node` as the walk-facing seam.

## The rules

`docs/APPLICATION.md` holds Carte's axioms: §1 what Carte is, §2 the
dependency seam, §3 routing, §4 the contract, §5 the store. A bare §
citation in a doc comment means that file. §6–§9 (the binding, what the
split dissolves, open questions, precedents) stayed in Masque's
`docs/internal/APPLICATION.md`, and a citation of them says "Masque's".
Work-package numbers (WP-4.1, P4, Appendix A) refer to
`masque/docs/internal/APPLICATION_BUILD.md`.

Many axioms end with a *drift indicator*: the shape in code that means
the rule is being broken. Check a change against the indicators of the
axioms it touches.

## Architecture

```
RouteTable          -- static shape, validated once (src/table.rs)
  -> Router::new    -- one handler per id (src/router.rs)
  -> per frame:
       draw from committed state     (Store::read, Router::visible_views)
       input -> Outbox<A>            (pushes, Router::escape, Store::raise)
       Store::tick                   (drain -> apply + producers -> commit -> Notice)
       Router::resolve               (walk; guards ask; scopes mount/unmount)
       if the path moved, tick again so new scopes are filled before drawing
```

### Key modules

| Module | Responsibility |
|---|---|
| `src/lib.rs` | Re-exports, `set!`/`set_num!`, and the shared vocabulary: `RouteId`, `Area`, `RaiseArg`, `Handled`, `Occlusion`, `Escape`, `Departure` |
| `src/table.rs` | `RouteTable`: edges, tiers, areas, occlusion and guards as table data; `validate` and `TableError` |
| `src/router.rs` | `Router` and the `Node` seam: the walk, lifecycle, depth arbitration, `drawing`/`visible_views`, `escape`, `ask` |
| `src/guard.rs` | `Guard` (a function over the store) and `Refusal` (one sentence) |
| `src/outbox.rs` | `Outbox<A>`: ordered actions, drained by the tick |
| `src/store.rs` | `Store`, `Scope`, producers, subscribers, `tick`, `Barrier`, `Writer`, `Notice` |
| `src/schema.rs` | `Schema`, `Kind`, `Field`, at-mount values, grains, structural comparison |
| `src/intent.rs` | `Intents`, `Vocabulary`, `Raise`, typed argument reading, `Drift` |
| `src/validate.rs` | `Validation` and `Findings`: selections and raises graded as refusals or reports |

## Coding Conventions

- **Rust 2021 edition**, `rust-version = "1.80"`.
- **No dependencies.** If a change seems to need one, the change belongs
  on the binding's side of the seam.
- **Doc comments** on public items using `///` and module-level `//!`.
  Module docs say what the module makes unrepresentable and which axiom
  that serves, with a § citation.
- **Startup errors over runtime surprises.** A contradictory table, a
  missing handler, two producers claiming one field: each fails at
  construction with a typed error naming the offender, never at the frame
  that would trip over it.
- **Refusals name what they refuse**: the field, the intent, the route.
- **Testing**: small modules keep `#[cfg(test)] mod tests` inline; large
  ones put them in a sibling file (`src/store/tests.rs`). Test names are
  sentences stating the property (`a_second_guard_on_one_door_fails_at_startup`).
  Integration tests go in `tests/`.

## Integration Guide

The examples are from `examples/notes.rs`. Read it end to end before
integrating: it is about 800 lines and exercises every piece.

### 1. The table

Register every id with `at_root` or `under` (once per parent: a screen
under two parents has two edges). Declare tiers with `mounts`,
`mounts_supplied` or `structural`, areas with `in_area` plus one
`default_area`, occlusion with `occludes`, and guards with `guard`.
`Router::new` validates the table.

### 2. Nodes

Implement `Node<Cx, A>` for each screen. `Cx` is your application state
and `A` your action type.

- `resolve_child(&Cx)` returns the one child this node is showing, read
  from `Cx`. Never store the path. Derive it.
- `escape` returns `Pop`, `Prompt` (after pushing the action that opens
  the prompt), `Ignore` or `Fall`.
- `child_popped` pushes the action that makes `resolve_child` stop
  claiming that child. A pop is a request, and the model is what changes.
- `enter` and `leave` run when the id joins or leaves the path. Route
  state is exactly what may die on `leave` (§3.9). Anything else lives
  in `Cx`.
- `update`, `escape` and friends take `&Cx`. Routes ask through the
  outbox, and never mutate the session.

### 3. The store

```rust
let mut store = Store::over(&table);
store.provides::<Shared>(Scope::Process)?;
store.provides::<Editor>(Scope::Node("note"))?;
let editor = store.producer::<Editor>(Scope::Node("note"), &["title", "body", "dirty", "words"])?;
```

Provide each scope's type once. A `Scope::Node` scope exists only while
its node is on the path. Writing to an unmounted scope is refused at
`barrier.writer`, which is how a producer skips it. Claim every field
with exactly one producer.

### 4. The frame

```rust
fn tick(&mut self) {
    let Self { model, store, outbox, producers, .. } = self;
    let notice = store.tick(outbox, |actions, barrier| {
        for action in actions {
            model.apply(action);
        }
        producers.produce(model, barrier);
    });
    // notice.changed(), notice.woke(id)
}

fn settle(&mut self) -> bool {
    let asked = !self.outbox.is_empty();
    self.tick();
    let moved = self.router.resolve(&mut self.model, &mut self.store);
    if moved {
        self.tick(); // fill scopes that just mounted
    }
    asked || moved
}
```

Draw from committed state before settling. Producers run every tick
regardless, because the store swallows sets that change nothing. Floor
continuous values with `set_num!` and a declared grain, or they wake
every subscriber every frame (§5.5).

### 5. Input

- UI actions: push onto the outbox, then settle.
- Escape: `router.escape(&cx, &mut outbox)`, then settle. Handle
  `EscapeOutcome::Unclaimed` yourself, if at all.
- Named raises from a document: `store.raise::<I, A>(scope, &raise,
  &mut outbox)` validates the raise against the scope's vocabulary and
  pushes the typed intent as an `A` (`A: From<I>`).
- Gray a control the guard would refuse: `router.ask(id, &store)`.
  `router.refused()` is the refusal the last walk stopped at.

### 6. Validation

Build a `Validation` over the store, feed it every shipped document's
selection (`selects`, `selects_named`, `reads`) and raises (`raises`),
then `finish`. Assert on `Findings::refuses()` in a test, and print the
reports. Carte parses nothing; turning documents into fields and
`Declared` raises is the surface side's job (lorekeeper's `contract`).

## Project Structure

```
carte/
  Cargo.toml                -- empty [dependencies], on purpose
  README.md
  AGENTS.md
  LICENSE
  docs/
    APPLICATION.md          -- Carte's rules (§1–§5)
  src/
    lib.rs                  -- re-exports, set!/set_num!, shared vocabulary
    table.rs                -- RouteTable
    router.rs               -- Router, Node, the walk
    guard.rs                -- Guard, Refusal
    outbox.rs               -- Outbox
    store.rs, store/tests.rs
    schema.rs, schema/tests.rs
    intent.rs, intent/tests.rs
    validate.rs, validate/tests.rs
  examples/
    notes.rs                -- the notes app over eframe
  tests/
    independence.rs         -- the no-dependencies guarantee
  web/                      -- the notes stage under Masque, its own workspace
    notes.masque
    src/                    -- app.rs (the app), lib.rs (the Stage), main.rs (extern "C")
```
