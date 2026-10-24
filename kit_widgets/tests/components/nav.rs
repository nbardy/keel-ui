//! Spatial navigation through the real router, real layout and real widgets: a `NavAction` from a gamepad or mapped
//! key moves focus to the nearest control in that direction.
use core::time::Duration;
use kit_core::kurbo::Vec2;
use kit_core::peniko::{Brush, Color};
use kit_core::{Axis, Key, Theme, Time, WidgetId};
use kit_input::{Focus, NavAction, NavDirection};
use kit_state::{Act, Cx, Ident, Intent, Life};
use kit_test::{Asked, Handle, TestApp};
use kit_view::{
    A11y, Anchor, AnchorH, AnchorV, Availability, Bindings, Layer, Node, PortalAnchor, Role,
    SafeArea, ScrollSpec, Style, Toggle, Tooltip, column, interactive, nothing, portal, rect, row,
    scroll_area,
};

fn cell_id(key: &str) -> WidgetId {
    WidgetId::of(&("cell", key))
}
fn brush() -> Brush {
    Brush::Solid(Color::from_rgba8(70, 90, 120, 255))
}
fn a11y(label: &str) -> A11y {
    A11y::Labeled {
        role: Role::Button,
        label: label.into(),
        toggle: Toggle::NotToggle,
    }
}
/// A control of fixed size, so positions do not depend on glyph widths.
fn cell(key: &str, width: f64, height: f64) -> Node<Intent<TestApp>> {
    interactive(
        cell_id(key),
        Bindings::activate(Intent::Ask(Asked(key.into()))),
        a11y(key),
        rect(Style::column().sized(width, height), brush()),
    )
}
fn cells(keys: &[&str]) -> Vec<Node<Intent<TestApp>>> {
    keys.iter().map(|k| cell(k, 40.0, 40.0)).collect()
}
fn spacer(width: f64, height: f64) -> Node<Intent<TestApp>> {
    rect(Style::column().sized(width, height), brush())
}
fn on(key: &str) -> Focus {
    Focus::On(cell_id(key))
}
fn go(
    h: &mut Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>,
    direction: NavDirection,
) {
    h.nav(NavAction::Move(direction));
}
fn settle(h: &mut Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>) {
    for _ in 0..120 {
        h.tick(|app| app.now = Time(app.now.0 + Duration::from_millis(16)));
    }
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Gone;
impl Ident<TestApp> for Gone {
    fn life(&self, _: &TestApp) -> Life {
        Life::Live
    }
}

/// An inventory: eight slots in rows of three, so the last row is short. `with_removal` adds a "Remove" control (key
/// `x`) that takes slot "4" off the screen.
fn inventory(cx: &Cx<'_, TestApp>, _: &Theme) -> Node<Intent<TestApp>> {
    let gone = cx.state(
        &Gone,
        || false,
        |s: &mut bool, _: &Gone, e: bool, _: &TestApp| {
            *s = e;
            Act::none()
        },
    );
    let slot = |k: &str| match (k, *gone) {
        ("4", true) => nothing(),
        _ => cell(k, 40.0, 40.0),
    };
    let rows = [["0", "1", "2"], ["3", "4", "5"], ["6", "7", ""]];
    let rows = rows.map(|r| row(r.into_iter().filter(|k| !k.is_empty()).map(slot)).gap(8.0));
    let remove = interactive(
        cell_id("remove"),
        Bindings::activate(gone.send(true)).with_shortcut(Key::Character('x')),
        a11y("remove"),
        rect(Style::column().sized(40.0, 40.0), brush()),
    );
    column([column(rows).gap(8.0), remove]).pad(10.0)
}

/// Moves in all four directions land on the nearest slot, stop at the edges instead of wrapping, and cross the short
/// last row to the slot beneath. Guards the scoring rule: with no weight on the cross axis "down from 5" picks 6
/// (the earlier slot) instead of 7 (the one nearest across).
#[test]
fn a_grid_is_walked_by_rows_and_columns_and_stops_at_its_edges() {
    let mut h = Handle::new(TestApp::new(), (360.0, 360.0), inventory);
    go(&mut h, NavDirection::Down);
    assert_eq!(
        h.focus(),
        on("0"),
        "from nowhere, Down enters at the first control"
    );
    let walk = [
        (NavDirection::Right, "1"),
        (NavDirection::Right, "2"),
        (NavDirection::Right, "2"),
        (NavDirection::Down, "5"),
        (NavDirection::Down, "7"),
        (NavDirection::Left, "6"),
        (NavDirection::Left, "6"),
        (NavDirection::Right, "7"),
        (NavDirection::Down, "remove"),
        (NavDirection::Down, "remove"),
        (NavDirection::Left, "remove"),
        (NavDirection::Up, "6"),
        (NavDirection::Up, "3"),
        (NavDirection::Up, "0"),
        (NavDirection::Up, "0"),
    ];
    for (direction, expect) in walk {
        go(&mut h, direction);
        assert_eq!(h.focus(), on(expect), "{direction:?}");
    }
    assert!(h.app().asked.is_empty(), "moving focus activates nothing");
}

#[test]
fn up_from_nowhere_enters_at_the_last_control_and_activate_presses_the_focused_one() {
    let mut h = Handle::new(TestApp::new(), (360.0, 360.0), inventory);
    go(&mut h, NavDirection::Up);
    assert_eq!(h.focus(), on("remove"));
    go(&mut h, NavDirection::Up);
    assert_eq!(
        h.focus(),
        on("6"),
        "the slot straight above the Remove control"
    );
    h.nav(NavAction::Activate);
    assert_eq!(h.app().asked, [Asked("6".into())]);
    h.nav(NavAction::Previous);
    assert_eq!(h.focus(), on("5"));
    h.nav(NavAction::Next);
    assert_eq!(h.focus(), on("6"));
}

/// Regression guard for the failure mode this feature exists to prevent: the focused slot is removed while the
/// player holds the d-pad. Focus is dropped (the control is gone) and the next move re-enters at a defined place
/// rather than stranding the player or panicking.
#[test]
fn a_focused_control_that_disappears_lets_the_next_move_re_enter() {
    let mut h = Handle::new(TestApp::new(), (360.0, 360.0), inventory);
    for direction in [NavDirection::Down, NavDirection::Right, NavDirection::Down] {
        go(&mut h, direction);
    }
    assert_eq!(h.focus(), on("4"));
    h.press_key(Key::Character('x'));
    assert!(!h.shows(Role::Button, "4"));
    go(&mut h, NavDirection::Right);
    assert_eq!(h.focus(), on("0"));
}

/// A settings column: the disabled row between two enabled ones is never landed on, from either side.
fn settings(_: &Cx<'_, TestApp>, _: &Theme) -> Node<Intent<TestApp>> {
    // Disabled but still bound to an action: only its availability keeps focus off it.
    let locked = Node::Interactive {
        identity: cell_id("brightness").into(),
        on: Bindings::activate(Intent::Ask(Asked("brightness".into()))),
        a11y: a11y("brightness"),
        tooltip: Tooltip::None,
        availability: Availability::Disabled,
        child: Box::new(spacer(40.0, 40.0)),
    };
    column([
        cell("volume", 40.0, 40.0),
        locked,
        cell("gamma", 40.0, 40.0),
    ])
    .gap(8.0)
    .pad(10.0)
}
#[test]
fn a_disabled_control_is_skipped() {
    let mut h = Handle::new(TestApp::new(), (200.0, 300.0), settings);
    go(&mut h, NavDirection::Down);
    assert_eq!(h.focus(), on("volume"));
    go(&mut h, NavDirection::Down);
    assert_eq!(h.focus(), on("gamma"));
    go(&mut h, NavDirection::Up);
    assert_eq!(h.focus(), on("volume"));
}

/// The screen's controls sit to the left of a modal; navigation stays inside the modal, edges included.
fn dialog(_: &Cx<'_, TestApp>, _: &Theme) -> Node<Intent<TestApp>> {
    let anchor = PortalAnchor::Viewport(Anchor {
        h: AnchorH::Start,
        v: AnchorV::Start,
        offset: Vec2::new(200.0, 10.0),
        safe_area: SafeArea::Ignore,
    });
    let choices = row(cells(&["yes", "no"])).gap(8.0);
    column([
        column(cells(&["a", "b", "c"])).gap(8.0).pad(10.0),
        portal(Layer::Modal, anchor, choices),
    ])
}
#[test]
fn a_modal_traps_navigation() {
    let mut h = Handle::new(TestApp::new(), (360.0, 240.0), dialog);
    go(&mut h, NavDirection::Down);
    assert_eq!(h.focus(), on("yes"), "entry is inside the modal");
    go(&mut h, NavDirection::Left);
    assert_eq!(
        h.focus(),
        on("yes"),
        "the screen to the left is not reachable"
    );
    go(&mut h, NavDirection::Right);
    assert_eq!(h.focus(), on("no"));
    go(&mut h, NavDirection::Down);
    assert_eq!(h.focus(), on("no"));
}

/// A 100px-high list of twelve 30px rows with two columns of controls beside it. Beside the list: "near" at the
/// height row 5 has once the list has scrolled to show it, and "far" at the height row 5 has in the layout.
fn list(_: &Cx<'_, TestApp>, _: &Theme) -> Node<Intent<TestApp>> {
    let keys: Vec<String> = (0..12).map(|i| format!("row{i}")).collect();
    let rows = keys.iter().map(|k| cell(k, 100.0, 30.0));
    let list = scroll_area(
        WidgetId::of(&"list"),
        ScrollSpec::new(Axis::Vertical),
        Style::column(),
        column(rows),
    )
    .size(100.0, 100.0);
    let beside = column([
        spacer(40.0, 70.0),
        cell("near", 40.0, 30.0),
        spacer(40.0, 50.0),
        cell("far", 40.0, 30.0),
    ]);
    row([list, beside]).gap(10.0)
}
fn list_offset(
    h: &Handle<TestApp, impl Fn(&Cx<'_, TestApp>, &Theme) -> Node<Intent<TestApp>>>,
) -> f64 {
    h.scroll_offset(WidgetId::of(&"list")).y
}
#[test]
fn moving_to_an_off_screen_row_scrolls_it_in_and_later_moves_use_its_screen_position() {
    let mut h = Handle::new(TestApp::new(), (240.0, 240.0), list);
    go(&mut h, NavDirection::Down);
    go(&mut h, NavDirection::Down);
    go(&mut h, NavDirection::Down);
    settle(&mut h);
    assert_eq!(h.focus(), on("row2"));
    assert_eq!(
        list_offset(&h),
        0.0,
        "row 2 ends at 90px: already in view, nothing scrolls"
    );
    for _ in 0..3 {
        go(&mut h, NavDirection::Down);
    }
    settle(&mut h);
    assert_eq!(h.focus(), on("row5"));
    let offset = list_offset(&h);
    assert!(
        (offset - 80.0).abs() < 1.0,
        "row 5 spans 150..180: the smallest scroll that shows it is 80, got {offset}"
    );
    // Row 5 is now drawn at 70..100, level with "near" (laid out at 70..100). Its layout position, 150..180, is
    // level with "far": using that would send the player to the wrong control.
    go(&mut h, NavDirection::Right);
    assert_eq!(h.focus(), on("near"));
}
