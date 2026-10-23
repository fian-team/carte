//! The notes app: `examples/notes.rs` without egui.
//!
//! The model, the actions, the scopes, the producers, the route table and
//! the screens are the example's, kept as close to it as a second copy
//! can be; an example is not a library, so they cannot be imported. What
//! differs is the frame: there is no `update` here, because the views are
//! a Masque document (`notes.masque`) and [`crate::Stage`] drives them.

use structure::schema::{reflect_of, Field, Kind, Lit, Schema};
use structure::{
    set, Barrier, Departure, Escape, EscapeOutcome, Node, Occlusion, Outbox, Producer, Refusal,
    RouteId, RouteTable, Router, Scope, Store,
};

// --- the app ---------------------------------------------------------------

pub struct Note {
    pub body: String,
    pub trashed: bool,
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
pub fn title_of(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Untitled")
        .to_string()
}

/// Everything the app knows. The router's `Cx`.
pub struct Model {
    pub notes: Vec<Note>,
    /// The note `note` is editing, while it is on the path.
    pub open: Option<usize>,
    /// The open note's text as edited; written into the note on save.
    pub draft: String,
    /// The child `home` is showing, if any.
    pub home_child: Option<RouteId>,
    /// The child `note` is showing, if any.
    pub note_child: Option<RouteId>,
    pub dark: bool,
}

impl Model {
    fn new(dark: bool) -> Self {
        Self {
            notes: vec![
                Note::new("Groceries\n\nMilk, eggs, coffee."),
                Note::new("Reading list\n\nInvisible Cities, then the atlas."),
                Note::new("Ideas\n\nA map that draws itself."),
            ],
            open: None,
            draft: String::new(),
            home_child: None,
            note_child: None,
            dark,
        }
    }

    pub fn dirty(&self) -> bool {
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
pub enum Action {
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
pub struct Shared {
    /// `(index, title)` of every note not in the trash.
    pub notes: Vec<(usize, String)>,
    /// `(index, title)` of every note in it.
    pub trashed: Vec<(usize, String)>,
    pub dark: bool,
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
pub struct Editor {
    pub title: String,
    pub body: String,
    pub dirty: bool,
    pub words: usize,
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

pub const EDITOR: Scope = Scope::Node("note");

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

pub struct App {
    pub model: Model,
    pub router: Router<Model, Action, Screen>,
    pub store: Store,
    pub outbox: Outbox<Action>,
    producers: Producers,
    /// `scope.field` for each field the last non-empty commit changed.
    pub last_commit: Vec<String>,
    pub last_escape: Option<EscapeOutcome>,
}

impl App {
    pub fn new(dark: bool) -> Result<Self, Box<dyn std::error::Error>> {
        let table = table();
        let mut store = Store::over(&table);
        store.provides::<Shared>(Scope::Process)?;
        store.provides::<Editor>(EDITOR)?;
        let producers = Producers::new(&mut store)?;

        let routes: Vec<(RouteId, Box<Screen>)> = vec![
            ("home", Box::new(Home)),
            ("note", Box::new(NoteScreen)),
            ("discard", Box::new(Leaf(Occlusion::None))),
            ("settings", Box::new(Leaf(Occlusion::View))),
            ("trash", Box::new(Leaf(Occlusion::View))),
        ];
        let router = Router::new(table, routes, |_: &Model| "home")?;

        let mut app = Self {
            model: Model::new(dark),
            router,
            store,
            outbox: Outbox::new(),
            producers,
            last_commit: Vec::new(),
            last_escape: None,
        };
        app.settle();
        Ok(app)
    }

    /// Ask for something, as a view does.
    pub fn ask(&mut self, action: Action) {
        self.outbox.push(action);
    }

    /// Escape, or a Back button: the deepest screen decides what leaving
    /// means.
    pub fn escape(&mut self) {
        self.last_escape = Some(self.router.escape(&self.model, &mut self.outbox));
    }

    /// Apply what this frame asked for, then re-derive the path. Returns
    /// whether anything moved, so the caller knows to draw again.
    pub fn settle(&mut self) -> bool {
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
            .map(|c| match c.scope {
                Scope::Process => format!("process.{}", c.field),
                Scope::Node(id) => format!("{id}.{}", c.field),
            })
            .collect();
    }
}

// --- the same frames, with no window ---------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(app: &mut App, action: Action) {
        app.ask(action);
        app.settle();
    }

    #[test]
    fn escape_with_unsaved_changes_prompts_over_the_editor() {
        let mut app = App::new(false).unwrap();
        ask(&mut app, Action::Open(0));
        ask(&mut app, Action::Edit("changed".into()));
        app.escape();
        app.settle();
        assert_eq!(app.router.path(), ["home", "note", "discard"]);
        assert_eq!(app.router.visible_views(), ["note", "discard"]);
        ask(&mut app, Action::Close);
        assert_eq!(app.router.path(), ["home"]);
        assert!(!app.store.is_mounted(EDITOR));
    }

    #[test]
    fn the_trash_refuses_while_empty() {
        let app = App::new(false).unwrap();
        let refusal = app.router.ask("trash", &app.store).unwrap_err();
        assert_eq!(refusal.sentence(), "The trash is empty.");
    }
}
