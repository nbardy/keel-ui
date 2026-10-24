//! `Node::map` must rewrite every message-bearing slot, however deeply nested; a missed slot would silently deliver
//! the child enum's message to the wrong handler.
use kit_core::kurbo::Rect;
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, WidgetId};
use kit_view::testing::laid_out;
use kit_view::*;

#[derive(Clone, Debug, PartialEq)]
enum Child {
    Tap,
    Hover(bool),
    Dropped(u64),
    Edit(Edit),
}
#[derive(Clone, Debug, PartialEq)]
enum Parent {
    Inner(Child),
}

fn r() -> Rect {
    Rect::new(0.0, 0.0, 10.0, 10.0)
}

#[test]
fn map_reaches_bindings_drop_closures_and_text_fields_through_nesting() {
    let black = Brush::Solid(Color::from_rgba8(0, 0, 0, 255));
    let button = interactive(
        WidgetId::key("b"),
        Bindings::activate(Child::Tap)
            .with_hover(Child::Hover(true), Child::Hover(false))
            .with_drop(1, |d| Child::Dropped(d.payload.value)),
        A11y::Hidden,
        rect(Style::column(), black),
    );
    let field = text_field(
        WidgetId::key("f"),
        "Name",
        &EditorState::single_line("hi"),
        TextStyle::new(12.0, Color::from_rgba8(0, 0, 0, 255)),
        Child::Edit,
    );
    let tree: Node<Child> = layer(
        0.5,
        scroll(
            WidgetId::key("s"),
            Axis::Vertical,
            Style::column(),
            column(vec![button, field]),
        ),
    );
    // layer, scroll, column, interactive, rect, field
    let laid = laid_out(tree.map(Parent::Inner), &[r(); 6]);

    let LaidKind::Interactive { on, .. } = &laid.find(WidgetId::key("b")).expect("button").kind
    else {
        panic!("button is interactive")
    };
    assert_eq!(on.activate, Some(Parent::Inner(Child::Tap)));
    let hover = on.hover.as_ref().expect("hover kept");
    assert_eq!(
        (&hover.enter, &hover.leave),
        (
            &Parent::Inner(Child::Hover(true)),
            &Parent::Inner(Child::Hover(false))
        )
    );
    let drop = on.drop.as_ref().expect("drop kept");
    let dropped = kit_view::Dropped {
        payload: DragPayload { kind: 1, value: 42 },
        source: WidgetId::key("x"),
        target: WidgetId::key("b"),
    };
    assert_eq!((drop.on_drop)(dropped), Parent::Inner(Child::Dropped(42)));

    let LaidKind::TextField { on_edit, .. } = &laid.find(WidgetId::key("f")).expect("field").kind
    else {
        panic!("field")
    };
    assert_eq!(
        on_edit(Edit::Submit),
        Parent::Inner(Child::Edit(Edit::Submit))
    );
}

#[test]
fn assemble_rejects_a_placed_list_of_the_wrong_length() {
    use kit_view::testing::text_system;
    let tree: Node<Child> = column(vec![spacer(), spacer()]);
    let placed = [Placed {
        rect: r(),
        shown: Shown::Visible,
    }];
    let viewport = kit_core::Viewport::new(kit_core::kurbo::Size::new(10.0, 10.0), 1.0);
    let err = LaidOut::assemble(tree, viewport, &placed, &mut text_system()).err();
    assert_eq!(
        err,
        Some(LayoutError::PlacedCount {
            expected: 3,
            got: 1
        })
    );
}
