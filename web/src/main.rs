//! The stage's `extern "C"` surface, for emscripten's `cwrap`: Ogham's
//! own stage's (`ogham/web/src/main.rs`), with the app in place of a
//! source to build, and a snapshot of what Carte sees.
//!
//! One stage per module instance. Strings come out through
//! `stage_out_ptr()`, for the length a call returned. The buffers behind
//! `stage_out_ptr` and `stage_pixels_ptr` are valid until the next call
//! into the stage.

use std::cell::RefCell;

use notes_stage::{modifiers, Key, Pointer, Stage};

thread_local! {
    static STAGE: RefCell<Option<Stage>> = const { RefCell::new(None) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn with_stage<R>(f: impl FnOnce(&mut Stage) -> R) -> Option<R> {
    STAGE.with(|s| s.borrow_mut().as_mut().map(f))
}

/// Put `text` in the out buffer and return its length.
fn out(text: &str) -> u32 {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        o.clear();
        o.extend_from_slice(text.as_bytes());
        o.len() as u32
    })
}

/// Mount the app under its document. `dark` is the page's theme. 0 on
/// success; 1 on an error, whose text `stage_error` returns.
#[no_mangle]
pub extern "C" fn stage_new(dark: u32) -> u32 {
    match Stage::new(dark != 0) {
        Ok(stage) => {
            STAGE.with(|s| *s.borrow_mut() = Some(stage));
            0
        }
        Err(e) => {
            ERROR.with(|err| *err.borrow_mut() = Some(e));
            1
        }
    }
}

/// The last error into the out buffer; 0 when there is none.
#[no_mangle]
pub extern "C" fn stage_error() -> u32 {
    let mounting = ERROR.with(|e| e.borrow().clone());
    let running = with_stage(|s| s.error().map(String::from)).flatten();
    mounting.or(running).map(|e| out(&e)).unwrap_or(0)
}

/// The strict vocabulary's findings into the out buffer, one per line.
#[no_mangle]
pub extern "C" fn stage_notes() -> u32 {
    with_stage(|s| out(&s.notes())).unwrap_or(0)
}

/// What Carte sees, as JSON, into the out buffer.
#[no_mangle]
pub extern "C" fn stage_snapshot() -> u32 {
    with_stage(|s| out(&s.snapshot())).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn stage_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn stage_resize(width: f32, height: f32, dpr: f32) {
    with_stage(|s| s.resize(width, height, dpr));
}

#[no_mangle]
pub extern "C" fn stage_set_dark(dark: u32) {
    with_stage(|s| s.set_dark(dark != 0));
}

/// `kind`: 0 move, 1 down, 2 up, 3 leave. `mods`: ctrl 1, alt 2,
/// shift 4, meta 8.
#[no_mangle]
pub extern "C" fn stage_pointer(kind: u32, x: f32, y: f32, mods: u32) {
    let kind = match kind {
        1 => Pointer::Down,
        2 => Pointer::Up,
        3 => Pointer::Leave,
        _ => Pointer::Move,
    };
    with_stage(|s| s.pointer(kind, x, y, modifiers(mods)));
}

/// 1 when the document took the wheel, 0 when the page should scroll.
#[no_mangle]
pub extern "C" fn stage_wheel(x: f32, y: f32, dx: f32, dy: f32) -> u32 {
    with_stage(|s| s.wheel(x, y, dx, dy) as u32).unwrap_or(0)
}

/// `kind`: 0 down, 1 up. `character`: a Unicode scalar, 0 for none.
#[no_mangle]
pub extern "C" fn stage_key(kind: u32, key_code: u32, character: u32, mods: u32) {
    let kind = if kind == 1 { Key::Up } else { Key::Down };
    let character = char::from_u32(character).filter(|c| *c != '\0');
    with_stage(|s| s.key(kind, key_code, character, modifiers(mods)));
}

/// Advance `dt_ms` and draw on damage. Returns the `FRAME_*` bits, or
/// `u32::MAX` on a runtime error (read it with `stage_error`).
#[no_mangle]
pub extern "C" fn stage_frame(dt_ms: f32) -> u32 {
    with_stage(|s| s.frame(dt_ms / 1000.0).unwrap_or(u32::MAX)).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn stage_pixels_ptr() -> *const u8 {
    with_stage(|s| s.pixels().as_ptr()).unwrap_or(std::ptr::null())
}

#[no_mangle]
pub extern "C" fn stage_pixel_width() -> u32 {
    with_stage(|s| s.pixel_size().0 as u32).unwrap_or(0)
}

#[no_mangle]
pub extern "C" fn stage_pixel_height() -> u32 {
    with_stage(|s| s.pixel_size().1 as u32).unwrap_or(0)
}

fn main() {}
