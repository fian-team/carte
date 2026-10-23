//! A notes app on `structure`, drawn with egui.
//!
//! ```text
//! cargo run --example notes
//! ```
//!
//! Five screens, one of them under two parents:
//!
//! ```text
//! home ─┬─ note ─┬─ discard     a prompt: the editor stays visible under it
//!       │        └─ settings    also under home: one screen, two parents
//!       ├─ settings
//!       └─ trash                guarded: refused while the trash is empty
//! ```
//!
//! How the work divides:
//!
//! - [`Model`] is the app: the notes, the draft being edited, and which
//!   child each screen is showing. It is the router's context, so the path
//!   is derived from it every frame rather than pushed and popped.
//! - The UI never writes the model. It pushes [`Action`]s onto the outbox,
//!   and [`Store::tick`] hands them to the one place that applies them.
//! - The store holds what the screens draw: [`Shared`] for the whole
//!   process, and [`Editor`], which exists only while `note` is
//!   on the path.
//! - Each frame draws from committed state, then ticks (apply actions, run
//!   producers, commit), then resolves the path. When the path moved it
//!   ticks once more, so a scope that just mounted is filled before it is
//!   drawn.
//!
//! The panel on the right shows the router and the store as they run.

use eframe::egui;
use structure::schema::{reflect_of, Field, Kind, Lit, Schema};
use structure::{
    set, Barrier, Departure, Escape, EscapeOutcome, Node, Occlusion, Outbox, Producer, Refusal,
    RouteId, RouteTable, Router, Scope, Store, SubscriberId,
};

fn main() -> eframe::Result {
    let app = App::new().expect("the table and the store are valid");
    eframe::run_native(
        "notes",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(app))),
    )
}

// --- the app ---------------------------------------------------------------

struct Note {
    body: String,
    trashed: bool,
}

impl Note {
    fn new(body: &str) -> Self {
        Self {
            body: body.to_string(),
            trashed: false,
        }
    }
}

/// The first non-empty line of a note.
fn title_of(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Untitled")
        .to_string()
}

/// Everything the app knows. The router's `Cx`.
struct Model {
    notes: Vec<Note>,
    /// The note `note` is editing, while it is on the path.
    open: Option<usize>,
    /// The open note's text as edited; written into the note on save.
    draft: String,
    /// The child `home` is showing, if any.
    home_child: Option<RouteId>,
    /// The child `note` is showing, if any.
    note_child: Option<RouteId>,
    dark: bool,
}

impl Model {
    fn new() -> Self {
        Self {
            notes: vec![
                Note::new("Welcome\n\nOpen a note, edit it, and press Escape."),
                Note::new("Shopping\n\nbread\nleeks"),
            ],
            open: None,
            draft: String::new(),
            home_child: None,
            note_child: None,
            dark: true,
        }
    }

    fn dirty(&self) -> bool {
        self.open.is_some_and(|i| self.notes[i].body != self.draft)
    }

    fn apply(&mut self, action: &Action) {
        match action {
            Action::NewNote => {
                self.notes.push(Note::new(""));
                self.open = Some(self.notes.len() - 1);
                self.home_child = Some("note");
            }
            Action::Open(i) => {
                self.open = Some(*i);
                self.home_child = Some("note");
            }
            Action::OpenSettings => self.home_child = Some("settings"),
            Action::OpenTrash => self.home_child = Some("trash"),
            Action::OpenNoteSettings => self.note_child = Some("settings"),
            Action::Closed { parent: "home" } => self.home_child = None,
            Action::Closed { parent: "note" } => self.note_child = None,
            Action::Closed { .. } => {}
            Action::Edit(text) => self.draft = text.clone(),
            Action::Save => {
                if let Some(i) = self.open {
                    self.notes[i].body = self.draft.clone();
                }
            }
            Action::AskDiscard => self.note_child = Some("discard"),
            // `note`'s `leave` drops the draft.
            Action::Close => self.home_child = None,
            Action::SaveAndClose => {
                self.apply(&Action::Save);
                self.apply(&Action::Close);
            }
            Action::Trash(i) => self.notes[*i].trashed = true,
            Action::Restore(i) => self.notes[*i].trashed = false,
            Action::Purge(i) => {
                self.notes.remove(*i);
                // The guard would refuse the walk into an empty trash anyway,
                // but `home` would go on claiming it, and the trash would
                // reopen the moment something was thrown away.
                if self.notes.iter().all(|n| !n.trashed) && self.home_child == Some("trash") {
                    self.home_child = None;
                }
            }
            Action::SetDark(dark) => self.dark = *dark,
        }
    }
}

/// Everything the UI and the screens can ask for.
#[derive(Clone, Debug)]
enum Action {
    NewNote,
    Open(usize),
    OpenSettings,
    OpenTrash,
    OpenNoteSettings,
    /// `parent` stops showing its child: what a pop turns into.
    Closed {
        parent: RouteId,
    },
    Edit(String),
    Save,
    AskDiscard,
    Close,
    SaveAndClose,
    Trash(usize),
    Restore(usize),
    Purge(usize),
    SetDark(bool),
}

// --- the store's scopes ----------------------------------------------------

/// What any screen may read. The process scope, which holds one record:
/// a scope is provided by exactly one type.
#[derive(Clone, Debug, PartialEq)]
struct Shared {
    /// `(index, title)` of every note not in the trash.
    notes: Vec<(usize, String)>,
    /// `(index, title)` of every note in it.
    trashed: Vec<(usize, String)>,
    dark: bool,
}

impl Schema for Shared {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("notes", reflect_of::<Vec<(usize, String)>>()),
            Field::new("trashed", reflect_of::<Vec<(usize, String)>>()),
            Field::new("dark", Kind::Bool),
        ])
    }

    fn at_mount(_: Option<&Lit>) -> Self {
        Self {
            notes: Schema::at_mount(None),
            trashed: Schema::at_mount(None),
            dark: Schema::at_mount(None),
        }
    }
}

/// What the editor draws. Mounted when `note` joins the path and gone when
/// it leaves, so nothing about one editing session survives into the next.
#[derive(Clone, Debug, PartialEq)]
struct Editor {
    title: String,
    body: String,
    dirty: bool,
    words: usize,
}

impl Schema for Editor {
    fn reflect() -> Kind {
        Kind::Record(vec![
            Field::new("title", Kind::Str),
            Field::new("body", Kind::Str),
            Field::new("dirty", Kind::Bool),
            Field::new("words", Kind::Int),
        ])
    }

    fn at_mount(_: Option<&Lit>) -> Self {
        Self {
            title: Schema::at_mount(None),
            body: Schema::at_mount(None),
            dirty: Schema::at_mount(None),
            words: Schema::at_mount(None),
        }
    }
}

const EDITOR: Scope = Scope::Node("note");

/// The one writer of each field, claimed at startup. `library` and `prefs`
/// write the same scope and claim different fields of it.
struct Producers {
    library: Producer<Shared>,
    prefs: Producer<Shared>,
    editor: Producer<Editor>,
}

impl Producers {
    fn new(store: &mut Store) -> Result<Self, structure::StoreError> {
        Ok(Self {
            library: store.producer(Scope::Process, &["notes", "trashed"])?,
            prefs: store.producer(Scope::Process, &["dark"])?,
            editor: store.producer(EDITOR, &["title", "body", "dirty", "words"])?,
        })
    }

    /// Project the model into the store. Writes that change nothing are
    /// dropped by the store, so this runs every tick.
    fn produce(&self, model: &Model, barrier: &mut Barrier<'_>) {
        let listed = |trashed: bool| -> Vec<(usize, String)> {
            let notes = model.notes.iter().enumerate();
            notes
                .filter(|(_, n)| n.trashed == trashed)
                .map(|(i, n)| (i, title_of(&n.body)))
                .collect()
        };
        let mut library = barrier.writer(&self.library).expect("process scope");
        set!(library, notes, listed(false)).expect("claimed");
        set!(library, trashed, listed(true)).expect("claimed");

        let mut prefs = barrier.writer(&self.prefs).expect("process scope");
        set!(prefs, dark, model.dark).expect("claimed");

        // Refused while `note` is off the path: the scope does not exist.
        if let Ok(mut editor) = barrier.writer(&self.editor) {
            set!(editor, title, title_of(&model.draft)).expect("claimed");
            set!(editor, body, model.draft.clone()).expect("claimed");
            set!(editor, dirty, model.dirty()).expect("claimed");
            set!(editor, words, model.draft.split_whitespace().count()).expect("claimed");
        }
    }
}

// --- the route table and its screens ---------------------------------------

fn table() -> RouteTable {
    let mut table = RouteTable::new();
    table
        .at_root("home")
        .under("note", "home")
        .under("discard", "note")
        .under("settings", "home")
        .under("settings", "note")
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

type Screen = dyn Node<Model, Action>;

/// The list of notes. Shows whichever child the model says it is showing.
struct Home;

impl Node<Model, Action> for Home {
    fn resolve_child(&self, model: &Model) -> Option<RouteId> {
        model.home_child
    }
    fn occludes(&self) -> Occlusion {
        Occlusion::View
    }
    fn update(&mut self, _: &Model, _: &mut Outbox<Action>, _: f32) {}
    fn escape(&mut self, _: &Model, _: &mut Outbox<Action>) -> Escape {
        Escape::Fall
    }
    fn take_leave_request(&mut self) -> bool {
        false
    }
    fn child_popped(&mut self, out: &mut Outbox<Action>, _: RouteId) {
        out.push(Action::Closed { parent: "home" });
    }
    fn enter(&mut self, _: &mut Model) {}
    fn leave(&mut self, _: &mut Model) {}
    fn depart(&mut self, _: Option<RouteId>) -> Departure {
        Departure::Cut
    }
}

/// The editor. Escape with unsaved changes prompts instead of leaving.
struct NoteScreen;

impl Node<Model, Action> for NoteScreen {
    fn resolve_child(&self, model: &Model) -> Option<RouteId> {
        model.note_child
    }
    fn occludes(&self) -> Occlusion {
        Occlusion::View
    }
    fn update(&mut self, _: &Model, _: &mut Outbox<Action>, _: f32) {}
    fn escape(&mut self, model: &Model, out: &mut Outbox<Action>) -> Escape {
        match model.dirty() {
            true => {
                out.push(Action::AskDiscard);
                Escape::Prompt
            }
            false => Escape::Pop,
        }
    }
    fn take_leave_request(&mut self) -> bool {
        false
    }
    fn child_popped(&mut self, out: &mut Outbox<Action>, _: RouteId) {
        out.push(Action::Closed { parent: "note" });
    }
    fn enter(&mut self, model: &mut Model) {
        model.draft = match model.open {
            Some(i) => model.notes[i].body.clone(),
            None => String::new(),
        };
    }
    fn leave(&mut self, model: &mut Model) {
        model.open = None;
        model.draft.clear();
        model.note_child = None;
    }
    fn depart(&mut self, _: Option<RouteId>) -> Departure {
        Departure::Cut
    }
}

/// A screen with no children that leaves on Escape: the prompt, settings
/// and the trash.
struct Leaf(Occlusion);

impl Node<Model, Action> for Leaf {
    fn resolve_child(&self, _: &Model) -> Option<RouteId> {
        None
    }
    fn occludes(&self) -> Occlusion {
        self.0
    }
    fn update(&mut self, _: &Model, _: &mut Outbox<Action>, _: f32) {}
    fn escape(&mut self, _: &Model, _: &mut Outbox<Action>) -> Escape {
        Escape::Pop
    }
    fn take_leave_request(&mut self) -> bool {
        false
    }
    fn child_popped(&mut self, _: &mut Outbox<Action>, _: RouteId) {}
    fn enter(&mut self, _: &mut Model) {}
    fn leave(&mut self, _: &mut Model) {}
    fn depart(&mut self, _: Option<RouteId>) -> Departure {
        Departure::Cut
    }
}

// --- the frame -------------------------------------------------------------

struct App {
    model: Model,
    router: Router<Model, Action, Screen>,
    store: Store,
    outbox: Outbox<Action>,
    producers: Producers,
    /// Subscribed to `note.dirty` and `trashed`, to show per-field wakes.
    watcher: SubscriberId,
    woken: Vec<String>,
    last_commit: Vec<String>,
    last_escape: Option<EscapeOutcome>,
}

impl App {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let table = table();
        let mut store = Store::over(&table);
        store.provides::<Shared>(Scope::Process)?;
        store.provides::<Editor>(EDITOR)?;
        let producers = Producers::new(&mut store)?;
        let watcher = store.subscriber();
        store.subscribe(watcher, EDITOR, "dirty")?;
        store.subscribe(watcher, Scope::Process, "trashed")?;

        let routes: Vec<(RouteId, Box<Screen>)> = vec![
            ("home", Box::new(Home)),
            ("note", Box::new(NoteScreen)),
            ("discard", Box::new(Leaf(Occlusion::None))),
            ("settings", Box::new(Leaf(Occlusion::View))),
            ("trash", Box::new(Leaf(Occlusion::View))),
        ];
        let router = Router::new(table, routes, |_: &Model| "home")?;

        let mut app = Self {
            model: Model::new(),
            router,
            store,
            outbox: Outbox::new(),
            producers,
            watcher,
            woken: Vec::new(),
            last_commit: Vec::new(),
            last_escape: None,
        };
        app.settle();
        Ok(app)
    }

    /// Apply what this frame asked for, then re-derive the path. Returns
    /// whether anything moved, so the caller knows to draw again.
    fn settle(&mut self) -> bool {
        let asked = !self.outbox.is_empty();
        self.tick();
        let moved = self.router.resolve(&mut self.model, &mut self.store);
        if moved {
            // Fill the scopes that just mounted before they are drawn.
            self.tick();
        }
        asked || moved
    }

    fn tick(&mut self) {
        let Self {
            model,
            store,
            outbox,
            producers,
            ..
        } = self;
        let notice = store.tick(outbox, |actions, barrier| {
            for action in actions {
                model.apply(action);
            }
            producers.produce(model, barrier);
        });
        if notice.is_empty() {
            return;
        }
        let changed = notice.changed().iter();
        self.last_commit = changed
            .map(|c| format!("{}.{}", c.scope, c.field))
            .collect();
        if notice.woke(self.watcher) {
            let watched = [(EDITOR, "dirty"), (Scope::Process, "trashed")];
            for c in notice.changed() {
                if watched.contains(&(c.scope, c.field.as_str())) {
                    self.woken.push(format!("{}.{}", c.scope, c.field));
                }
            }
        }
    }

    fn inspector(&self, ctx: &egui::Context) {
        egui::SidePanel::right("inspector")
            .min_width(260.0)
            .show(ctx, |ui| {
                ui.heading("Router");
                ui.label("path");
                ui.monospace(self.router.path().join(" › "));
                ui.label("visible views");
                ui.monospace(self.router.visible_views().join(", "));
                if let Some((id, why)) = self.router.refused() {
                    ui.label(format!("refused at {id}: {why}"));
                }
                if let Some(outcome) = self.last_escape {
                    ui.label(format!("last Escape: {outcome:?}"));
                }
                ui.separator();
                ui.heading("Store");
                ui.label(match self.store.is_mounted(EDITOR) {
                    true => format!("editor scope: mounted, v{}", self.store.version(EDITOR)),
                    false => "editor scope: not mounted".to_string(),
                });
                ui.label("last commit changed");
                for field in &self.last_commit {
                    ui.monospace(field);
                }
                ui.separator();
                ui.label("watcher woke for");
                for field in self.woken.iter().rev().take(8) {
                    ui.monospace(field);
                }
            });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let dark = self
            .store
            .read::<Shared>(Scope::Process)
            .is_some_and(|p| p.dark);
        ctx.set_visuals(match dark {
            true => egui::Visuals::dark(),
            false => egui::Visuals::light(),
        });

        let mut back = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        self.inspector(ctx);
        let mut view = View {
            store: &self.store,
            router: &self.router,
            out: &mut self.outbox,
            back: &mut back,
        };
        for id in self.router.visible_views() {
            match id {
                "home" => home(ctx, &mut view),
                "note" => note(ctx, &mut view),
                "discard" => discard(ctx, &mut view),
                "settings" => settings(ctx, &mut view),
                "trash" => trash(ctx, &mut view),
                _ => {}
            }
        }

        // Escape and every Back button go through the router: the deepest
        // screen decides what leaving means.
        if back {
            self.last_escape = Some(self.router.escape(&self.model, &mut self.outbox));
        }
        if self.settle() {
            ctx.request_repaint();
        }
    }
}

// --- the views -------------------------------------------------------------

/// What a view may touch: committed state to read, the router to ask, and
/// the outbox to ask for things on.
struct View<'a> {
    store: &'a Store,
    router: &'a Router<Model, Action, Screen>,
    out: &'a mut Outbox<Action>,
    back: &'a mut bool,
}

fn home(ctx: &egui::Context, v: &mut View<'_>) {
    let Some(library) = v.store.read::<Shared>(Scope::Process) else {
        return;
    };
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Notes");
            if ui.button("New note").clicked() {
                v.out.push(Action::NewNote);
            }
            if ui.button("Settings").clicked() {
                v.out.push(Action::OpenSettings);
            }
            // The same guard the walk asks, so the reason shown is the
            // reason the walk would give.
            let door = v.router.ask("trash", v.store);
            let label = format!("Trash ({})", library.trashed.len());
            let button = ui.add_enabled(door.is_ok(), egui::Button::new(label));
            let button = match &door {
                Err(why) => button.on_disabled_hover_text(why.sentence()),
                Ok(()) => button,
            };
            if button.clicked() {
                v.out.push(Action::OpenTrash);
            }
        });
        ui.separator();
        if library.notes.is_empty() {
            ui.weak("No notes.");
        }
        for (i, title) in &library.notes {
            ui.horizontal(|ui| {
                if ui.link(title).clicked() {
                    v.out.push(Action::Open(*i));
                }
                if ui.small_button("trash").clicked() {
                    v.out.push(Action::Trash(*i));
                }
            });
        }
    });
}

fn note(ctx: &egui::Context, v: &mut View<'_>) {
    let Some(editor) = v.store.read::<Editor>(EDITOR) else {
        return;
    };
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading(&editor.title);
            if editor.dirty {
                ui.weak("edited");
            }
        });
        let mut body = editor.body.clone();
        let edit = egui::TextEdit::multiline(&mut body)
            .desired_width(f32::INFINITY)
            .desired_rows(16);
        if ui.add(edit).changed() {
            v.out.push(Action::Edit(body));
        }
        ui.horizontal(|ui| {
            ui.label(format!("{} words", editor.words));
            if ui
                .add_enabled(editor.dirty, egui::Button::new("Save"))
                .clicked()
            {
                v.out.push(Action::Save);
            }
            if ui.button("Settings").clicked() {
                v.out.push(Action::OpenNoteSettings);
            }
            if ui.button("Close").clicked() {
                *v.back = true;
            }
        });
    });
}

fn discard(ctx: &egui::Context, v: &mut View<'_>) {
    egui::Window::new("Discard changes?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("This note has unsaved changes.");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    v.out.push(Action::SaveAndClose);
                }
                if ui.button("Discard").clicked() {
                    v.out.push(Action::Close);
                }
                if ui.button("Keep editing").clicked() {
                    *v.back = true;
                }
            });
        });
}

fn settings(ctx: &egui::Context, v: &mut View<'_>) {
    let Some(prefs) = v.store.read::<Shared>(Scope::Process) else {
        return;
    };
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Settings");
        let mut dark = prefs.dark;
        if ui.checkbox(&mut dark, "Dark mode").changed() {
            v.out.push(Action::SetDark(dark));
        }
        ui.weak("Reached from the list and from the editor. Back returns to whichever it was.");
        if ui.button("Back").clicked() {
            *v.back = true;
        }
    });
}

fn trash(ctx: &egui::Context, v: &mut View<'_>) {
    let Some(library) = v.store.read::<Shared>(Scope::Process) else {
        return;
    };
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("Trash");
        for (i, title) in &library.trashed {
            ui.horizontal(|ui| {
                ui.label(title);
                if ui.small_button("restore").clicked() {
                    v.out.push(Action::Restore(*i));
                }
                if ui.small_button("delete").clicked() {
                    v.out.push(Action::Purge(*i));
                }
            });
        }
        if ui.button("Back").clicked() {
            *v.back = true;
        }
    });
}

// --- the same frames, with no window ---------------------------------------
//
// `cargo test --example notes` drives the app the way `update` does, minus
// egui: ask for something, settle, look at the path.

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(app: &mut App, action: Action) {
        app.outbox.push(action);
        app.settle();
    }

    fn escape(app: &mut App) -> EscapeOutcome {
        let outcome = app.router.escape(&app.model, &mut app.outbox);
        app.settle();
        outcome
    }

    #[test]
    fn escape_with_unsaved_changes_prompts_over_the_editor() {
        let mut app = App::new().unwrap();
        ask(&mut app, Action::Open(0));
        assert_eq!(app.router.path(), ["home", "note"]);
        ask(&mut app, Action::Edit("changed".into()));
        assert!(app.store.read::<Editor>(EDITOR).unwrap().dirty);

        assert_eq!(escape(&mut app), EscapeOutcome::Prompted("note"));
        assert_eq!(app.router.path(), ["home", "note", "discard"]);
        assert_eq!(app.router.visible_views(), ["note", "discard"]);

        assert_eq!(escape(&mut app), EscapeOutcome::Popped("discard"));
        assert_eq!(app.router.path(), ["home", "note"], "keep editing");

        ask(&mut app, Action::Close);
        assert_eq!(app.router.path(), ["home"]);
        assert!(
            !app.store.is_mounted(EDITOR),
            "the editor's scope left with it"
        );
        assert_eq!(
            app.model.notes[0].body,
            Model::new().notes[0].body,
            "discarded"
        );
    }

    #[test]
    fn settings_returns_to_whichever_parent_it_was_opened_from() {
        let mut app = App::new().unwrap();
        ask(&mut app, Action::OpenSettings);
        assert_eq!(app.router.path(), ["home", "settings"]);
        escape(&mut app);
        assert_eq!(app.router.path(), ["home"]);

        ask(&mut app, Action::Open(1));
        ask(&mut app, Action::OpenNoteSettings);
        assert_eq!(app.router.path(), ["home", "note", "settings"]);
        escape(&mut app);
        assert_eq!(app.router.path(), ["home", "note"]);
    }

    #[test]
    fn the_trash_refuses_while_empty_and_closes_when_emptied() {
        let mut app = App::new().unwrap();
        let refusal = app.router.ask("trash", &app.store).unwrap_err();
        assert_eq!(refusal.sentence(), "The trash is empty.");

        ask(&mut app, Action::Trash(1));
        assert!(app.router.ask("trash", &app.store).is_ok());
        ask(&mut app, Action::OpenTrash);
        assert_eq!(app.router.path(), ["home", "trash"]);

        ask(&mut app, Action::Purge(1));
        assert_eq!(app.router.path(), ["home"]);
        ask(&mut app, Action::Trash(0));
        assert_eq!(
            app.router.path(),
            ["home"],
            "throwing away does not reopen it"
        );
    }

    #[test]
    fn a_reopened_note_starts_from_its_saved_text() {
        let mut app = App::new().unwrap();
        ask(&mut app, Action::Open(1));
        ask(&mut app, Action::Edit("draft".into()));
        ask(&mut app, Action::Close);
        ask(&mut app, Action::Open(1));
        let editor = app.store.read::<Editor>(EDITOR).unwrap();
        assert_eq!(editor.title, "Shopping", "seeded on the frame it mounted");
        assert!(!editor.dirty);
    }
}
