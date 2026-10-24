//! Editing behaviour on real shaped text (the bundled Inter) and on hand-made glyph clusters where the shaper's
//! clusters are finer than graphemes, which Inter will not produce on its own.

use kit_core::kurbo::Point;
use kit_core::peniko::Color;
use kit_text::{
    Affinity, Applied, Caret, Cluster, Composition, CosmicText, Dir, Edit, EditorState,
    FieldGeometry, Goal, LineEdge, Masking, MaxWidth, Mode, Motion, RichText, Selection, Shaped,
    TextConfig, TextStyle, Unit, Vertical, VisualLine, Wrap, inter,
};

fn text_system() -> CosmicText {
    CosmicText::new(inter::source(), TextConfig::default()).expect("the bundled Inter loads")
}

fn geometry(state: &EditorState, masking: Masking, width: f32) -> FieldGeometry {
    let mut text = text_system();
    let view = state.view();
    let shown = Shaped::new(&view, masking);
    let (wrap, max) = match view.mode {
        Mode::SingleLine => (Wrap::None, MaxWidth::Unbounded),
        Mode::MultiLine => (Wrap::WordOrGlyph, MaxWidth::Wrap(f64::from(width))),
    };
    let style = TextStyle::new(16.0, Color::WHITE).wrap(wrap);
    let layout = text.layout(&RichText::plain(shown.text()), &style, max);
    text.field_geometry(layout.id, shown, &view, width)
        .expect("live layout")
}

fn is_boundary(text: &str, at: usize) -> bool {
    text.is_char_boundary(at)
        && unicode_segmentation::UnicodeSegmentation::graphemes(&text[..at], true).count()
            + unicode_segmentation::UnicodeSegmentation::graphemes(&text[at..], true).count()
            == unicode_segmentation::UnicodeSegmentation::graphemes(text, true).count()
}

/// "e" + a combining acute that the shaper kept as its own cluster, then "x": a click or drag over the mark's half
/// of the cluster must not put the caret between the letter and its accent.
#[test]
fn a_hit_never_lands_inside_a_grapheme_even_when_the_shaper_clusters_finer() {
    let state = EditorState::single_line("e\u{301}x");
    let view = state.view();
    let cluster = |start, end, x, w| Cluster {
        start,
        end,
        x,
        w,
        rtl: false,
    };
    let line = VisualLine {
        top: 0.0,
        height: 20.0,
        start: 0,
        end: 4,
        ends_paragraph: true,
        clusters: vec![
            cluster(0, 1, 0.0, 10.0),
            cluster(1, 3, 10.0, 4.0),
            cluster(3, 4, 14.0, 10.0),
        ],
    };
    let geometry =
        FieldGeometry::from_lines(Shaped::new(&view, Masking::Plain), vec![line], &view, 100.0);
    for step in -10..60 {
        let at = geometry.hit(Point::new(f64::from(step) * 0.5, 5.0)).offset;
        assert!(
            is_boundary(&view.text, at),
            "x={} gave offset {at}, inside a grapheme",
            f64::from(step) * 0.5
        );
    }
}

/// Real shaping of a flag, a skin-toned thumb and a mark: whatever the font does with them, every pointer position
/// and every keyboard step ends on a grapheme boundary.
#[test]
fn pointer_and_keys_only_reach_grapheme_boundaries_in_real_text() {
    let text = "a\u{1F44D}\u{1F3FD}e\u{301}\u{1F1EF}\u{1F1F5}z";
    let mut state = EditorState::single_line(text);
    let geometry = geometry(&state, Masking::Plain, 400.0);
    for step in 0..400 {
        let at = geometry.hit(Point::new(f64::from(step) * 0.5, 5.0)).offset;
        assert!(is_boundary(text, at), "pointer at {step}/2 gave {at}");
    }
    state.apply(Edit::Move {
        to: Motion::DocStart,
        extend: false,
    });
    let mut stops = Vec::new();
    for _ in 0..10 {
        state.apply(Edit::Move {
            to: Motion::Right,
            extend: false,
        });
        stops.push(state.value().selection().head);
    }
    assert_eq!(
        stops,
        [1, 9, 12, 20, 21, 21, 21, 21, 21, 21],
        "one grapheme per press: a, thumb+tone, e+accent, flag, z"
    );
}

#[test]
fn shift_extend_then_typing_replaces_the_selection() {
    let mut state = EditorState::single_line("hello world");
    state.apply(Edit::Move {
        to: Motion::WordLeft,
        extend: true,
    });
    assert_eq!(state.value().selected(), "world");
    state.apply(Edit::Type("there".into()));
    assert_eq!(state.text(), "hello there");
    assert_eq!(state.value().selection(), Selection::caret(11));
}

#[test]
fn a_plain_arrow_over_a_selection_collapses_it_to_that_edge() {
    let mut state = EditorState::single_line("hello world");
    state.apply(Edit::Move {
        to: Motion::WordLeft,
        extend: true,
    });
    state.apply(Edit::Move {
        to: Motion::Left,
        extend: false,
    });
    assert_eq!(state.value().selection(), Selection::caret(6));
}

#[test]
fn word_delete_takes_a_word_in_one_undo_step() {
    let mut state = EditorState::single_line("one two  three");
    state.apply(Edit::Delete(Dir::Back, Unit::Word));
    assert_eq!(state.text(), "one two  ");
    state.apply(Edit::Delete(Dir::Back, Unit::Word));
    assert_eq!(state.text(), "one ");
    state.apply(Edit::Undo);
    assert_eq!(state.text(), "one two  ");
}

#[test]
fn undo_after_a_paste_restores_the_text_and_selection() {
    let mut state = EditorState::single_line("abc");
    state.apply(Edit::Move {
        to: Motion::Left,
        extend: true,
    });
    let before = (state.text().to_owned(), state.value().selection());
    let rev = state.value().rev();
    let applied = state.apply(Edit::Paste {
        rev,
        text: "XYZ".into(),
    });
    assert!(matches!(applied, Applied::Ok(_)));
    assert_eq!(state.text(), "abXYZ");
    state.apply(Edit::Undo);
    assert_eq!((state.text().to_owned(), state.value().selection()), before);
    state.apply(Edit::Redo);
    assert_eq!(state.text(), "abXYZ");
}

/// The clipboard answers later: if the text changed in between, the paste was computed against text that no
/// longer exists and is refused, loudly, instead of landing at offsets that now mean something else.
#[test]
fn a_paste_for_a_stale_revision_is_refused() {
    let mut state = EditorState::single_line("abc");
    let asked_at = state.value().rev();
    state.apply(Edit::Type("d".into()));
    let applied = state.apply(Edit::Paste {
        rev: asked_at,
        text: "XYZ".into(),
    });
    assert_eq!(
        applied,
        Applied::Stale {
            current: state.value().rev()
        }
    );
    assert_eq!(state.text(), "abcd");
}

#[test]
fn a_pasted_paragraph_stays_one_line_in_a_single_line_field_and_keeps_its_breaks_in_an_area() {
    let mut line = EditorState::single_line("");
    let rev = line.value().rev();
    line.apply(Edit::Paste {
        rev,
        text: "a\r\nb\u{8}c".into(),
    });
    assert_eq!(line.text(), "a bc");
    let mut area = EditorState::multi_line("");
    let rev = area.value().rev();
    area.apply(Edit::Paste {
        rev,
        text: "a\r\nb".into(),
    });
    assert_eq!(area.text(), "a\nb");
}

#[test]
fn a_composition_is_not_text_until_it_is_committed() {
    let mut state = EditorState::single_line("hi ");
    let rev = state.value().rev();
    state.apply(Edit::Preedit("ni".into()));
    assert_eq!(state.composition(), &Composition::Preedit("ni".into()));
    assert_eq!((state.text(), state.value().rev()), ("hi ", rev));
    state.apply(Edit::Preedit("\u{4f60}".into()));
    assert_eq!((state.text(), state.value().rev()), ("hi ", rev));
    // The platform commits through Type; the composition ends with it.
    state.apply(Edit::Type("\u{4f60}".into()));
    assert_eq!(state.text(), "hi \u{4f60}");
    assert_eq!(state.composition(), &Composition::Idle);
    state.apply(Edit::Preedit("x".into()));
    state.apply(Edit::Preedit(String::new()));
    assert_eq!(state.composition(), &Composition::Idle);
}

#[test]
fn the_composition_is_shaped_inline_and_underlined_without_touching_the_value() {
    let mut state = EditorState::single_line("ab");
    state.apply(Edit::Move {
        to: Motion::Left,
        extend: false,
    });
    state.apply(Edit::Preedit("WWW".into()));
    let plain = geometry(&EditorState::single_line("ab"), Masking::Plain, 400.0);
    let composing = geometry(&state, Masking::Plain, 400.0);
    let underline = composing.composition_rects();
    assert_eq!(underline.len(), 1);
    assert!(underline[0].width() > 15.0, "three wide letters");
    // "b" is pushed right by the composition, and the caret stands at the composition's end.
    assert!(
        composing.caret().x0
            > plain
                .caret_at(Caret {
                    offset: 1,
                    affinity: Affinity::After
                })
                .x0
                + 15.0
    );
    assert_eq!(state.text(), "ab");
}

/// Up, Up over a short middle line, Down, Down returns to the column it started from instead of the short line's end.
#[test]
fn vertical_movement_remembers_its_column_across_a_short_line() {
    let state = EditorState::multi_line("mmmmmm\nii\nmmmmmm");
    let geometry = geometry(&state, Masking::Plain, 400.0);
    let start = Caret {
        offset: 4,
        affinity: Affinity::After,
    };
    let (middle, goal) = geometry.vertical(start, Goal::Free, Vertical::Down);
    assert_eq!(middle.offset, 9, "the short line ends at its end");
    let (bottom, _) = geometry.vertical(middle, goal, Vertical::Down);
    assert_eq!(
        bottom.offset, 14,
        "back at the original column on the long line"
    );
    let (_, forgotten) = geometry.vertical(middle, Goal::Free, Vertical::Down);
    assert_ne!(forgotten, Goal::Free);
    let (lost, _) = geometry.vertical(middle, Goal::Free, Vertical::Down);
    assert_ne!(
        lost.offset, 14,
        "without the remembered column it would not return"
    );
}

#[test]
fn soft_wrapped_lines_move_by_visual_line_and_the_wrap_point_has_two_homes() {
    let state = EditorState::multi_line("aaaa bbbb cccc dddd");
    let geometry = geometry(&state, Masking::Plain, 60.0);
    let first = Caret {
        offset: 2,
        affinity: Affinity::After,
    };
    let end = geometry.line_edge(first, LineEdge::End);
    assert_eq!(
        end.affinity,
        Affinity::Before,
        "End stays on the wrapped line"
    );
    let at_end = geometry.caret_at(end);
    let same_offset_next_line = geometry.caret_at(Caret {
        offset: end.offset,
        affinity: Affinity::After,
    });
    assert!(
        same_offset_next_line.y0 > at_end.y0,
        "the same boundary on the next line"
    );
    let (down, _) = geometry.vertical(first, Goal::Free, Vertical::Down);
    assert!(
        down.offset > end.offset - 1,
        "Down from line 0 lands on line 1"
    );
    let (top, _) = geometry.vertical(first, Goal::Free, Vertical::Up);
    assert_eq!(top.offset, 0, "Up from the first line goes to the start");
}

/// Bullets are three bytes each and the value is not: offsets must come back in value space.
#[test]
fn a_masked_field_reports_value_offsets_not_bullet_offsets() {
    let value = "h\u{e9}llo";
    let state = EditorState::single_line(value);
    let geometry = geometry(&state, Masking::Bullets, 400.0);
    let far_right = geometry.hit(Point::new(300.0, 5.0)).offset;
    assert_eq!(far_right, value.len());
    let mut seen: Vec<usize> = (0..120)
        .map(|s| geometry.hit(Point::new(f64::from(s) * 0.5, 5.0)).offset)
        .collect();
    seen.dedup();
    assert_eq!(
        seen,
        [0, 1, 3, 4, 5, 6],
        "five graphemes, byte offsets of the value"
    );
}
