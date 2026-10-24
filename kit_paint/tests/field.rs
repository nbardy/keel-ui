//! What a focused text field paints: selection behind the glyphs, a blinking caret, a composition's underline.
use kit_core::kurbo::{Point, Rect, Size};
use kit_core::peniko::{Brush, Color};
use kit_core::{ColorVision, MotionPref, Theme, Time, Viewport, WidgetId};
use kit_input::*;
use kit_paint::*;
use kit_view::testing::laid_out;
use kit_view::*;
use std::time::Duration;

fn theme(motion: MotionPref) -> Theme {
    Theme::new(ColorVision::Normal, motion)
}

fn at(ms: u64) -> Time {
    Time(Duration::from_millis(ms))
}

/// A focused field showing `state`, painted at `now`.
fn painted(state: &EditorState, motion: MotionPref, now: Time) -> DrawList {
    let id = WidgetId::key("f");
    let field = text_field(id, "F", state, TextStyle::new(16.0, Color::WHITE), |e| e);
    let laid = laid_out(
        column(vec![field]),
        &[
            Rect::new(0.0, 0.0, 400.0, 100.0),
            Rect::new(0.0, 0.0, 200.0, 30.0),
        ],
    );
    let mut memory = UiMemory::new(Viewport::new(Size::new(400.0, 100.0), 1.0));
    for phase in [Phase::Down, Phase::Up] {
        Router::default().route(
            &laid,
            &mut memory,
            InputEvent::Pointer(PointerEvent {
                id: PointerId(0),
                kind: PointerKind::Mouse,
                phase,
                at: Point::new(5.0, 5.0),
                pressure: 1.0,
            }),
            Time::default(),
        );
    }
    paint(&laid, &memory, &theme(motion), now)
}

fn index_of(list: &DrawList, wanted: impl Fn(&Draw) -> bool) -> Option<usize> {
    list.items.iter().position(wanted)
}

fn is_selection(draw: &Draw) -> bool {
    let selection = theme(MotionPref::Full).colors.selection;
    matches!(draw, Draw::Fill { brush: Brush::Solid(c), .. } if *c == selection)
}

fn selected_world() -> EditorState {
    let mut state = EditorState::single_line("hello world");
    state.apply(Edit::Move {
        to: Motion::WordLeft,
        extend: true,
    });
    state
}

#[test]
fn the_selection_is_painted_behind_the_glyphs() {
    let list = painted(&selected_world(), MotionPref::Full, at(0));
    let selection = index_of(&list, is_selection).expect("a selection rect");
    let glyphs = index_of(&list, |d| matches!(d, Draw::Glyphs { .. })).expect("glyphs");
    assert!(selection < glyphs, "the highlight must not cover the text");
}

#[test]
fn the_caret_blinks_unless_motion_is_reduced() {
    let state = EditorState::single_line("abc");
    let caret = |motion, ms| {
        painted(&state, motion, at(ms))
            .items
            .iter()
            .any(|d| matches!(d, Draw::Caret { .. }))
    };
    assert!(caret(MotionPref::Full, 0));
    assert!(!caret(MotionPref::Full, 600), "off half of every second");
    assert!(caret(MotionPref::Full, 1100));
    for ms in [0, 600, 1100] {
        assert!(
            caret(MotionPref::Reduced, ms),
            "reduced motion: always shown, at {ms} ms"
        );
    }
}

#[test]
fn an_unfocused_field_shows_neither_selection_nor_caret() {
    let id = WidgetId::key("f");
    let state = selected_world();
    let field = text_field(id, "F", &state, TextStyle::new(16.0, Color::WHITE), |e| e);
    let laid = laid_out(
        column(vec![field]),
        &[
            Rect::new(0.0, 0.0, 400.0, 100.0),
            Rect::new(0.0, 0.0, 200.0, 30.0),
        ],
    );
    let memory = UiMemory::new(Viewport::new(Size::new(400.0, 100.0), 1.0));
    let list = paint(&laid, &memory, &theme(MotionPref::Full), at(0));
    assert!(index_of(&list, is_selection).is_none());
    assert!(index_of(&list, |d| matches!(d, Draw::Caret { .. })).is_none());
}

/// A long single line scrolls so the caret stays inside the field instead of drawing past its right edge.
#[test]
fn a_long_line_scrolls_to_keep_the_caret_in_the_box() {
    let state =
        EditorState::single_line("the quick brown fox jumps over the lazy dog and keeps going");
    let list = painted(&state, MotionPref::Reduced, at(0));
    let Some(Draw::Caret { rect, .. }) =
        list.items.iter().find(|d| matches!(d, Draw::Caret { .. }))
    else {
        panic!("a caret");
    };
    assert!(
        rect.x0 <= 200.0,
        "caret at {} is outside the 200 px field",
        rect.x0
    );
}
