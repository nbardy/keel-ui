//! `Screen` regions on a real engine: controls sit at their region's corner of the safe area, with no pixel
//! arithmetic in the caller.
mod common;

use common::text_system;
use kit_core::kurbo::{Insets, Rect, Size};
use kit_core::peniko::{Brush, Color};
use kit_core::{Identity, Viewport};
use kit_layout::TaffyLayout;
use kit_view::{
    A11y, Bindings, LaidOut, NoWorld, Node, Role, Screen, Style, column, interactive, rect, row,
};

fn button(label: &str, width: f64, height: f64) -> Node<()> {
    interactive(
        Identity::Positional,
        Bindings::none(),
        A11y::Labeled {
            role: Role::Button,
            label: label.to_owned(),
            toggle: kit_view::Toggle::NotToggle,
        },
        rect(
            Style::column().sized(width, height),
            Brush::Solid(Color::WHITE),
        ),
    )
}

fn lay_out(screen: Screen<()>, viewport: Viewport) -> LaidOut<()> {
    TaffyLayout::new()
        .layout(Node::from(screen), viewport, &mut text_system(), &NoWorld)
        .expect("screen lays out")
}

fn at(laid: &LaidOut<()>, label: &str) -> Rect {
    kit_view::testing::control(laid, Role::Button, label)
        .unwrap_or_else(|| panic!("no control {label:?}"))
        .rect
}

fn notched() -> Viewport {
    Viewport {
        safe_area: Insets::new(44.0, 0.0, 0.0, 34.0),
        ..Viewport::new(Size::new(800.0, 600.0), 1.0)
    }
}

/// A bottom-right cluster is flush to the safe-area corner (the notch and home-indicator insets), and a top-left one
/// to the opposite corner, whatever the cluster's own size.
#[test]
fn corner_regions_sit_flush_in_the_safe_area() {
    let viewport = notched();
    let laid = lay_out(
        Screen::new()
            .top_left(row![button("menu", 40.0, 30.0), button("mute", 40.0, 30.0)].gap(8.0))
            .bottom_right(
                column![button("fire", 70.0, 50.0), button("reload", 90.0, 20.0)]
                    .gap(6.0)
                    .align_items(kit_view::taffy::AlignItems::FLEX_END),
            ),
        viewport,
    );
    let safe = viewport.safe_rect();
    let (menu, reload, fire) = (at(&laid, "menu"), at(&laid, "reload"), at(&laid, "fire"));
    assert!(
        (menu.x0 - safe.x0).abs() < 0.5 && (menu.y0 - safe.y0).abs() < 0.5,
        "{menu:?} vs {safe:?}"
    );
    assert!(
        (at(&laid, "mute").x0 - (menu.x1 + 8.0)).abs() < 0.5,
        "gap between siblings"
    );
    assert!(
        (reload.x1 - safe.x1).abs() < 0.5 && (reload.y1 - safe.y1).abs() < 0.5,
        "{reload:?} vs {safe:?}"
    );
    assert!(
        (fire.x1 - safe.x1).abs() < 0.5,
        "End alignment right-aligns the narrower control too"
    );
    assert!((reload.y0 - (fire.y1 + 6.0)).abs() < 0.5);
}

/// The top region takes the width between the corner regions: a fluid control there does not overlap either corner.
#[test]
fn top_region_fills_between_its_corners() {
    let viewport = Viewport::new(Size::new(800.0, 600.0), 1.0);
    let bar = rect(Style::column().sized(1.0, 12.0), Brush::Solid(Color::WHITE))
        .width(kit_view::Percent(100.0));
    let laid = lay_out(
        Screen::new()
            .top_left(button("left", 100.0, 40.0))
            // Fluid content needs a column that fills: a shrink-wrapped parent has no width to take a percentage of.
            .top(
                column![interactive(
                    Identity::Positional,
                    Bindings::none(),
                    A11y::Labeled {
                        role: Role::Panel,
                        label: "gauge".into(),
                        toggle: kit_view::Toggle::NotToggle,
                    },
                    bar
                )]
                .fill_width(),
            )
            .top_right(button("right", 60.0, 40.0)),
        viewport,
    );
    let gauge = kit_view::testing::control(&laid, Role::Panel, "gauge")
        .expect("gauge")
        .rect;
    let (left, right) = (at(&laid, "left"), at(&laid, "right"));
    assert!(
        (gauge.x0 - left.x1).abs() < 0.5,
        "{gauge:?} starts where the left corner ends"
    );
    assert!(
        (gauge.x1 - right.x0).abs() < 0.5,
        "{gauge:?} ends where the right corner starts"
    );
}

/// J22: a row of controls that is allowed to wrap and is capped at its parent's width breaks onto further lines inside
/// the shrink-wrapped panel of a bottom region, instead of running off the screen. (The native HUD's plan panel,
/// shop and inventory rely on exactly this composition; before `.wrap()`/`.max_width()` a column count was chosen
/// from the viewport width by hand.)
#[test]
fn a_wrapping_row_in_a_bottom_region_breaks_lines_instead_of_overflowing_the_screen() {
    let viewport = Viewport::new(Size::new(300.0, 400.0), 1.0);
    let buttons = (0..8).map(|i| button(&format!("b{i}"), 40.0, 40.0));
    let panel = column![
        row(buttons)
            .gap(4.0)
            .wrap()
            .max_width(kit_view::Percent(100.0))
    ]
    .pad(8.0);
    let laid = lay_out(Screen::new().bottom(panel), viewport);
    let rects: Vec<Rect> = (0..8).map(|i| at(&laid, &format!("b{i}"))).collect();
    assert!(
        rects.iter().all(|r| r.x0 >= -0.5 && r.x1 <= 300.5),
        "a control left the 300px screen: {rects:?}"
    );
    let mut lines: Vec<i64> = rects.iter().map(|r| r.y0.round() as i64).collect();
    lines.dedup();
    assert!(
        lines.len() >= 2,
        "eight 40px controls cannot share a 300px line: {rects:?}"
    );
}
