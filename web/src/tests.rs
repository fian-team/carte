//! The stage driven the way a page drives it: pointer events at points on
//! the canvas, and the router's path read back.

use super::*;

const W: f32 = 360.0;
const H: f32 = 400.0;

fn stage() -> Stage {
    let mut stage = Stage::new(false).expect("the app mounts under its document");
    stage.resize(W, H, 1.0);
    stage.frame(0.016).expect("first frame");
    assert!(stage.notes().is_empty(), "{}", stage.notes());
    stage
}

fn click(stage: &mut Stage, x: f32, y: f32) {
    let none = KeyModifiers::default();
    stage.pointer(Pointer::Move, x, y, none.clone());
    stage.pointer(Pointer::Down, x, y, none.clone());
    stage.pointer(Pointer::Up, x, y, none);
    // Settle the animations, so the next click lands where things rest.
    for _ in 0..60 {
        stage.frame(0.016).expect("frame");
    }
}

fn escape(stage: &mut Stage) {
    stage.key(Key::Down, ESCAPE, None, KeyModifiers::default());
    stage.frame(0.016).expect("frame");
}

fn path(stage: &Stage) -> Vec<&'static str> {
    stage.app().router.path().to_vec()
}

#[test]
fn the_first_frame_draws_the_list() {
    let mut stage = stage();
    assert_eq!(path(&stage), ["home"]);
    let bits = stage.frame(0.016).unwrap();
    assert_eq!(bits & FRAME_SETTLED, 0, "nothing was asked");
    assert!(stage.pixels().iter().any(|&b| b != 0));
}

#[test]
fn a_row_opens_its_note_and_escape_closes_it() {
    let mut stage = stage();
    // The first row sits under the 52px bar.
    click(&mut stage, 100.0, 80.0);
    assert_eq!(path(&stage), ["home", "note"]);
    assert!(stage.app().store.is_mounted(EDITOR));
    assert!(stage.snapshot().contains(r#""title":"Groceries""#), "{}", stage.snapshot());
    escape(&mut stage);
    assert_eq!(path(&stage), ["home"]);
    assert!(!stage.app().store.is_mounted(EDITOR));
}

#[test]
fn the_trash_says_why_not() {
    let mut stage = stage();
    // The trash button, in the bottom bar.
    click(&mut stage, 40.0, H - 26.0);
    assert_eq!(path(&stage), ["home"]);
    assert!(stage.snapshot().contains(r#""refusal":"The trash is empty.""#), "{}", stage.snapshot());
}

#[test]
fn an_edited_note_prompts_and_the_prompt_stacks_over_it() {
    let mut stage = stage();
    click(&mut stage, 100.0, 80.0);
    // Type into the editor, below the bar.
    click(&mut stage, 100.0, 120.0);
    for c in "!".chars() {
        stage.key(Key::Down, c as u32, Some(c), KeyModifiers::default());
        stage.key(Key::Up, c as u32, Some(c), KeyModifiers::default());
    }
    stage.frame(0.016).unwrap();
    assert!(stage.app().model.dirty(), "{}", stage.snapshot());
    escape(&mut stage);
    assert_eq!(path(&stage), ["home", "note", "discard"]);
    assert_eq!(stage.app().router.visible_views(), ["note", "discard"]);
}
