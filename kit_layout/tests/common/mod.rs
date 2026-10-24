#![allow(dead_code)]
use kit_core::kurbo::{Point, Rect};
use kit_core::peniko::{Brush, Color};
use kit_view::*;

pub use kit_view::testing::text_system;

/// Projects world x/y straight to the screen; negative x is behind the camera.
pub struct Plane;
impl WorldProjector for Plane {
    fn project(&self, point: WorldPoint) -> Projection {
        let [x, y, _] = point.0;
        if x < 0.0 {
            Projection::Culled
        } else {
            Projection::OnScreen(Point::new(x, y))
        }
    }
}

pub fn white() -> Brush {
    Brush::Solid(Color::from_rgba8(255, 255, 255, 255))
}

pub fn block(w: f64, h: f64) -> Node<()> {
    rect(Style::column().sized(w, h), white())
}

pub fn pre_order(l: &LaidOut<()>) -> Vec<(Rect, Option<Shown>)> {
    fn walk(n: &LaidNode<()>, out: &mut Vec<(Rect, Option<Shown>)>) {
        let shown = match &n.kind {
            LaidKind::World { shown, .. } => Some(*shown),
            _ => None,
        };
        out.push((n.rect, shown));
        for c in n.children() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(&l.root, &mut out);
    out
}

/// A flex container with its children packed at the start of the cross axis.
pub fn flex(direction: taffy::FlexDirection, wrap: taffy::FlexWrap, gap: f64) -> Style {
    let mut s = Style::row()
        .gap(gap)
        .align_items(taffy::AlignItems::FLEX_START);
    s.layout.flex_direction = direction;
    s.layout.flex_wrap = wrap;
    s
}

pub fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

/// Fixed height, width left to the parent.
pub fn tall(h: f64) -> Style {
    let mut s = Style::column();
    s.layout.size.height = px(h);
    s
}
