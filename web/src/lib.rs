//! The notes stage: `examples/notes.rs` run by the real router and store,
//! its views a Masque document drawn into a CPU raster surface whose pixels
//! a page copies into a `<canvas>`.
//!
//! This is the binding the introduction describes: the only code that
//! knows both sides. Each input goes to the document; what the document
//! raised becomes actions on the outbox (or an Escape, or a question for a
//! guard); the app settles; and what the store committed is pushed back
//! into the document as host state, with the router's visible views as its
//! route path. [`Stage::snapshot`] is the same frame as the page's inspector
//! reads it.
//!
//! The raster half follows Masque's own browser stage (`masque/web`), which
//! hosts any one document; this one hosts one app.

pub mod app;
mod json;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use masque::runtime::config::RuntimeConfig;
use masque::runtime::value::Value;
use masque::skia::SkiaEnv;
use masque::skia_safe::{surfaces, AlphaType, Color, ColorType, ImageInfo};
use masque::widget::event::{Event, KeyModifiers};
use masque::widget::point::Point;
use masque::widget::style::CursorRole;
use masque::widget::Surface;
use masque::Masque;
use carte::Scope;

use app::{Action, App, Editor, Shared, EDITOR};
use json::Json;

/// The document: every screen in the route table, and an outlet.
pub const DOCUMENT: &str = include_str!("../notes.masque");

const FONT_FAMILY: &str = "Geist";
const FONTS: &[&[u8]] = &[
    include_bytes!("../../../masque/web/fonts/Geist-400.ttf"),
    include_bytes!("../../../masque/web/fonts/Geist-700.ttf"),
];

/// [`Stage::frame`] drew new pixels.
pub const FRAME_REPAINTED: u32 = 1;
/// The pointer is over something clickable.
pub const FRAME_CURSOR_POINTER: u32 = 2;
/// The app settled into a new frame: the page should take a new
/// [`Stage::snapshot`].
pub const FRAME_SETTLED: u32 = 4;

/// The DOM `keyCode` of Escape.
const ESCAPE: u32 = 27;

/// A pointer event, as the page reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointer {
    Move,
    Down,
    Up,
    /// The pointer left the canvas: a move to nowhere, so every hovered
    /// widget hears `mouse_leave`.
    Leave,
}

/// A key event, as the page reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Down,
    Up,
}

/// Modifier bits shared with the page: ctrl 1, alt 2, shift 4, meta 8.
pub fn modifiers(bits: u32) -> KeyModifiers {
    KeyModifiers {
        ctrl: bits & 1 != 0,
        alt: bits & 2 != 0,
        shift: bits & 4 != 0,
        meta: bits & 8 != 0,
    }
}

/// What the document can raise, read where it is raised: an event handler
/// must be `Send + Sync`, and a `Value` holds `Rc`s.
#[derive(Debug)]
enum Ask {
    Do(Action),
    /// Escape, or a Back button.
    Back,
    /// A door with a guard: the router is asked before the action is
    /// pushed, as a greyed-out button would be.
    OpenTrash,
}

/// The events the document may raise, and the ask each becomes.
fn read_event(name: &str, args: &[Value]) -> Option<Ask> {
    let index = || match args.first() {
        Some(Value::Integer(i)) if *i >= 0 => Some(*i as usize),
        _ => None,
    };
    Some(match name {
        "new_note" => Ask::Do(Action::NewNote),
        "open" => Ask::Do(Action::Open(index()?)),
        "open_settings" => Ask::Do(Action::OpenSettings),
        "open_trash" => Ask::OpenTrash,
        "note_settings" => Ask::Do(Action::OpenNoteSettings),
        "edit" => match args.first() {
            Some(Value::String(text)) => Ask::Do(Action::Edit(text.clone())),
            _ => return None,
        },
        "save" => Ask::Do(Action::Save),
        "save_and_close" => Ask::Do(Action::SaveAndClose),
        "close" => Ask::Do(Action::Close),
        "back" => Ask::Back,
        "trash" => Ask::Do(Action::Trash(index()?)),
        "restore" => Ask::Do(Action::Restore(index()?)),
        "purge" => Ask::Do(Action::Purge(index()?)),
        "set_dark" => match args.first() {
            Some(Value::Boolean(dark)) => Ask::Do(Action::SetDark(*dark)),
            _ => return None,
        },
        _ => return None,
    })
}

const EVENTS: &[&str] = &[
    "new_note",
    "open",
    "open_settings",
    "open_trash",
    "note_settings",
    "edit",
    "save",
    "save_and_close",
    "close",
    "back",
    "trash",
    "restore",
    "purge",
    "set_dark",
];

pub struct Stage {
    app: App,
    masque: Masque,
    env: SkiaEnv,
    /// Logical size and device pixel ratio.
    width: f32,
    height: f32,
    dpr: f32,
    /// The last frame, RGBA8 unpremultiplied, `width·dpr × height·dpr`:
    /// what `putImageData` takes.
    pixels: Vec<u8>,
    /// Set by anything that changes what is drawn outside the tree's own
    /// damage flags: a resize.
    dirty: bool,
    asks: Arc<Mutex<Vec<Ask>>>,
    /// The sentence the last refused door gave, until the next ask.
    refusal: Option<String>,
    /// The app settled since the page last drew a frame.
    settled: bool,
    error: Option<String>,
}

impl Stage {
    /// The app, mounted and settled, under the document. `dark` is the
    /// page's theme, which the app starts in.
    pub fn new(dark: bool) -> Result<Self, String> {
        let app = App::new(dark).map_err(|e| e.to_string())?;
        let asks: Arc<Mutex<Vec<Ask>>> = Arc::new(Mutex::new(Vec::new()));
        let mut config = RuntimeConfig::new().with_strict_vocabulary();
        for &name in EVENTS {
            let asks = asks.clone();
            config = config.with_event_handler(name, move |args| {
                if let Some(ask) = read_event(name, args) {
                    asks.lock().expect("ask queue lock poisoned").push(ask);
                }
                Ok(Value::Void)
            });
        }
        let mut masque = Masque::from_source(DOCUMENT, config).map_err(|e| e.to_string())?;
        masque.register_font_data(FONT_FAMILY, FONTS);
        masque.set_default_font(FONT_FAMILY);

        let mut stage = Self {
            app,
            masque,
            env: SkiaEnv::new(raster(1, 1)),
            width: 1.0,
            height: 1.0,
            dpr: 1.0,
            pixels: Vec::new(),
            dirty: true,
            asks,
            refusal: None,
            settled: true,
            error: None,
        };
        stage.resize(1.0, 1.0, 1.0);
        stage.project();
        Ok(stage)
    }

    /// What the strict vocabulary found in the document: keys and values
    /// the builder ignores, one per line.
    pub fn notes(&self) -> String {
        let violations = self.masque.vocabulary_violations();
        let lines: Vec<String> = violations.iter().map(|v| v.to_string()).collect();
        lines.join("\n")
    }

    /// The last frame error, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The canvas's logical size and device pixel ratio.
    pub fn resize(&mut self, width: f32, height: f32, dpr: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        self.dpr = if dpr > 0.0 { dpr } else { 1.0 };
        let (w, h) = self.pixel_size();
        self.env.surface = raster(w, h);
        self.env.set_dpi_scale(self.dpr);
        self.pixels = vec![0; (w * h * 4) as usize];
        self.dirty = true;
    }

    /// The pixel buffer's size in device pixels.
    pub fn pixel_size(&self) -> (i32, i32) {
        (
            (self.width * self.dpr).round().max(1.0) as i32,
            (self.height * self.dpr).round().max(1.0) as i32,
        )
    }

    pub fn pointer(&mut self, kind: Pointer, x: f32, y: f32, mods: KeyModifiers) {
        let name = match kind {
            Pointer::Move => "mouse_move",
            Pointer::Down => "mouse_down",
            Pointer::Up => "mouse_up",
            Pointer::Leave => "mouse_move",
        };
        let point = match kind {
            Pointer::Leave => Point::new(-1.0e6, -1.0e6),
            _ => Point::new(x, y),
        };
        self.call(&Event::with_point(name.to_string(), point).with_modifiers(mods));
    }

    /// True when the document took the wheel; otherwise the page should
    /// scroll instead.
    pub fn wheel(&mut self, x: f32, y: f32, dx: f32, dy: f32) -> bool {
        self.call(&Event::scroll(Point::new(x, y), dx, dy))
    }

    /// `key_code` is the DOM's `keyCode`; `character` is the DOM's `key`
    /// when it is one character. Escape belongs to the router, not to the
    /// document: the deepest screen decides what it means.
    pub fn key(&mut self, kind: Key, key_code: u32, character: Option<char>, mods: KeyModifiers) {
        match kind {
            Key::Down if key_code == ESCAPE => {
                self.asks.lock().expect("ask queue lock poisoned").push(Ask::Back);
                self.settle();
            }
            Key::Down => {
                self.call(&Event::keydown(key_code, None, mods.clone()));
                let character = character.or((key_code == 32).then_some(' '));
                if let Some(c) = character {
                    self.call(&Event::keypress(key_code, Some(c), mods));
                }
            }
            Key::Up => {
                self.call(&Event::keyup(key_code, character, mods));
            }
        }
    }

    /// The page's theme changed. The app has its own dark mode, in
    /// Settings; this asks for it the same way the toggle does.
    pub fn set_dark(&mut self, dark: bool) {
        self.app.ask(Action::SetDark(dark));
        self.settle();
    }

    fn call(&mut self, event: &Event) -> bool {
        let taken = self.masque.get_ui_mut().call_event(event);
        self.settle();
        taken
    }

    /// Hand what the document raised to the app, settle it, and push what
    /// the store committed back into the document.
    fn settle(&mut self) {
        let asks: Vec<Ask> = std::mem::take(&mut *self.asks.lock().expect("ask queue lock poisoned"));
        if asks.is_empty() {
            return;
        }
        for ask in asks {
            self.refusal = None;
            match ask {
                Ask::Do(action) => self.app.ask(action),
                Ask::Back => self.app.escape(),
                Ask::OpenTrash => match self.app.router.ask("trash", &self.app.store) {
                    Ok(()) => self.app.ask(Action::OpenTrash),
                    Err(why) => self.refusal = Some(why.sentence().to_string()),
                },
            }
        }
        self.app.settle();
        self.project();
        self.settled = true;
    }

    /// The committed store, the guard's answer and the visible views, as
    /// the document's host state.
    fn project(&mut self) {
        let app = &self.app;
        let shared = app.store.read::<Shared>(Scope::Process).cloned();
        let editor = app.store.read::<Editor>(EDITOR).cloned();
        let views = app.router.visible_views();
        let trash = app.router.ask("trash", &app.store);
        let on_note = app.router.path().contains(&"note");
        let refusal = self.refusal.clone().unwrap_or_default();
        self.masque.with_runtime_mut(|rt| {
            rt.set_route_path(&views);
            if let Some(shared) = &shared {
                rt.set_host_state("notes", listed(&shared.notes));
                rt.set_host_state("trashed", listed(&shared.trashed));
                rt.set_host_state("dark", shared.dark);
            }
            if let Some(editor) = &editor {
                rt.set_host_state("title", editor.title.clone());
                rt.set_host_state("body", editor.body.clone());
                rt.set_host_state("dirty", editor.dirty);
                rt.set_host_state("words", editor.words);
            }
            rt.set_host_state("trash_open", trash.is_ok());
            rt.set_host_state("refusal", refusal);
            rt.set_host_state("from_note", on_note);
        });
    }

    /// Advance `dt` seconds and draw if anything changed. Returns
    /// `FRAME_*` bits. A frame with none of them set means the page can
    /// stop its animation loop until the next input.
    pub fn frame(&mut self, dt: f32) -> Result<u32, String> {
        let (width, height) = (self.width, self.height);
        self.masque.set_screen_size(width, height);
        if let Err(e) = self.masque.update() {
            let message = e.to_string();
            self.error = Some(message.clone());
            return Err(message);
        }
        let ui = self.masque.get_ui_mut();
        ui.tick_animations(dt.min(1.0 / 30.0));
        // Read the damage before `layout`, which clears the repaint flag
        // when it runs a full pass.
        let damaged = self.dirty || ui.needs_repaint() || ui.needs_layout();
        ui.layout(width, height);

        let mut bits = 0;
        if ui.hovered_cursor() == CursorRole::Pointer {
            bits |= FRAME_CURSOR_POINTER;
        }
        if std::mem::take(&mut self.settled) {
            bits |= FRAME_SETTLED;
        }
        if damaged {
            self.env.surface.canvas().clear(Color::TRANSPARENT);
            self.env.draw(ui);
            ui.clear_needs_repaint();
            let (w, h) = self.pixel_size();
            let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, None);
            let read =
                self.env
                    .surface
                    .read_pixels(&info, &mut self.pixels, (w * 4) as usize, (0, 0));
            debug_assert!(read, "raster read_pixels failed");
            self.dirty = false;
            bits |= FRAME_REPAINTED;
        }
        Ok(bits)
    }

    /// The last frame's pixels; see [`Self::pixel_size`].
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// What Carte sees, as JSON: the path and the visible views, the model
    /// the path was derived from, the guard's answer, the last commit, and
    /// each scope with its committed fields.
    pub fn snapshot(&self) -> String {
        let app = &self.app;
        let m = &app.model;
        let mut j = Json::object();
        j.strings("path", app.router.path());
        j.strings("visible", app.router.visible_views());
        let mut model = Json::object();
        model.opt_string("home_child", m.home_child);
        model.opt_string("note_child", m.note_child);
        match m.open {
            Some(i) => model.int("open", i as i64),
            None => model.null("open"),
        };
        j.raw("model", &model.finish());
        match app.router.ask("trash", &app.store) {
            Ok(()) => j.null("trash_refusal"),
            Err(why) => j.string("trash_refusal", why.sentence()),
        };
        match &self.refusal {
            Some(why) => j.string("refusal", why),
            None => j.null("refusal"),
        };
        match app.last_escape {
            Some(outcome) => j.string("escape", &format!("{outcome:?}")),
            None => j.null("escape"),
        };
        j.strings("commit", &app.last_commit);

        let mut process = Json::object();
        process.bool("mounted", app.store.is_mounted(Scope::Process));
        process.int("version", app.store.version(Scope::Process) as i64);
        if let Some(shared) = app.store.read::<Shared>(Scope::Process) {
            let mut fields = Json::object();
            fields.int("notes", shared.notes.len() as i64);
            fields.int("trashed", shared.trashed.len() as i64);
            fields.bool("dark", shared.dark);
            process.raw("fields", &fields.finish());
        }
        let mut note = Json::object();
        note.bool("mounted", app.store.is_mounted(EDITOR));
        note.int("version", app.store.version(EDITOR) as i64);
        if let Some(editor) = app.store.read::<Editor>(EDITOR) {
            let mut fields = Json::object();
            fields.string("title", &editor.title);
            fields.bool("dirty", editor.dirty);
            fields.int("words", editor.words as i64);
            note.raw("fields", &fields.finish());
        }
        let mut scopes = Json::object();
        scopes.raw("process", &process.finish());
        scopes.raw("note", &note.finish());
        j.raw("scopes", &scopes.finish());
        j.finish()
    }

    /// The app, for tests.
    pub fn app(&self) -> &App {
        &self.app
    }
}

/// `(index, title)` pairs as the document reads them: `{ index, title }`.
fn listed(notes: &[(usize, String)]) -> Vec<Value> {
    let row = |(i, title): &(usize, String)| {
        let fields: HashMap<String, Value> = HashMap::from([
            ("index".to_string(), Value::Integer(*i as i32)),
            ("title".to_string(), Value::String(title.clone())),
        ]);
        Value::Map(fields.into())
    };
    notes.iter().map(row).collect()
}

fn raster(w: i32, h: i32) -> masque::skia_safe::Surface {
    surfaces::raster_n32_premul((w, h)).expect("raster surface")
}

#[cfg(test)]
mod tests;
