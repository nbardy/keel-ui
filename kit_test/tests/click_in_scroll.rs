//! `Handle::click` on controls inside scroll areas: the press lands where the control is drawn, through every
//! enclosing scroll offset, and a control scrolled out of view is reported instead of clicked somewhere else.
use kit_core::kurbo::Point;
use kit_core::kurbo::Vec2;
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, Theme, WidgetId};
use kit_input::{DeltaUnit, InputEvent};
use kit_state::{Cx, Intent};
use kit_test::{Asked, Handle, TestApp};
use kit_view::{
    A11y, Bindings, Node, Role, Style, TextStyle, Toggle, column, interactive, rect, scroll, text,
};

fn button(theme: &Theme, name: &str) -> Node<Intent<TestApp>> {
    let face = text(name, TextStyle::new(14.0, theme.colors.text));
    let a11y = A11y::Labeled {
        role: Role::Button,
        label: name.into(),
        toggle: Toggle::NotToggle,
    };
    let on_press = Bindings::activate(Intent::Ask(Asked(name.into())));
    interactive(WidgetId::key(name), on_press, a11y, face)
}

fn buttons(theme: &Theme, names: &[&str]) -> Node<Intent<TestApp>> {
    column(names.iter().map(|name| button(theme, name)))
}

/// Ten buttons in a 100 px high vertical scroll area: far taller than the viewport.
fn list(_: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let names: Vec<String> = (0..10).map(|i| format!("Item {i}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    column([scroll(
        WidgetId::key("list"),
        Axis::Vertical,
        Style::column().sized(100.0, 100.0),
        buttons(theme, &names),
    )])
}

fn wheel(at: Point, dy: f64) -> InputEvent {
    InputEvent::Wheel {
        at,
        delta: Vec2::new(0.0, dy),
        unit: DeltaUnit::Pixels,
    }
}

#[test]
fn a_control_in_a_scrolled_area_is_clicked_where_it_is_drawn() {
    let mut h = Handle::new(TestApp::new(), (200.0, 200.0), list);
    // Past the end: the list clamps to its last row, so "Item 9" is in view and far below its laid rect.
    h.input(wheel(Point::new(50.0, 50.0), 1.0e9));
    h.click(Role::Button, "Item 9");
    assert_eq!(h.app().asked, [Asked("Item 9".into())]);
}

#[test]
#[should_panic(expected = "scrolled out of view")]
fn a_control_scrolled_out_of_view_panics_instead_of_clicking_elsewhere() {
    let mut h = Handle::new(TestApp::new(), (200.0, 200.0), list);
    h.input(wheel(Point::new(50.0, 50.0), 1.0e9));
    h.click(Role::Button, "Item 0");
}

/// A list inside a list: the outer area is scrolled to its end, and so is the inner one, whose last row must be
/// clicked through both offsets. Dropping either offset puts the press outside the outer viewport.
fn nested(_: &Cx<'_, TestApp>, theme: &Theme) -> Node<Intent<TestApp>> {
    let filler = rect(
        Style::column().sized(100.0, 200.0),
        Brush::Solid(Color::from_rgba8(0, 0, 0, 255)),
    );
    let inner = scroll(
        WidgetId::key("inner"),
        Axis::Vertical,
        Style::column().sized(100.0, 60.0),
        buttons(
            theme,
            &[
                "Inner 0", "Inner 1", "Inner 2", "Inner 3", "Inner 4", "Inner 5",
            ],
        ),
    );
    column([scroll(
        WidgetId::key("outer"),
        Axis::Vertical,
        Style::column().sized(100.0, 100.0),
        column([filler, inner]),
    )])
}

#[test]
fn a_control_in_a_list_inside_a_scrolled_list_is_clicked_through_both_offsets() {
    let mut h = Handle::new(TestApp::new(), (200.0, 200.0), nested);
    h.input(wheel(Point::new(50.0, 50.0), 1.0e9));
    h.input(wheel(Point::new(50.0, 60.0), 1.0e9));
    h.click(Role::Button, "Inner 5");
    assert_eq!(h.app().asked, [Asked("Inner 5".into())]);
}
