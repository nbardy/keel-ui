//! Vector icons authored once as `const` path ops on a 32x32 canvas. The same ops draw the native kit shape
//! (`Icon::node`) and the web SVG (`Icon::svg`), so picker and queue can never drift apart.
use crate::common::solid;
use kit_core::ShapeSpec;
use kit_core::kurbo::{BezPath, Rect, Stroke};
use kit_core::peniko::Color;
use kit_view::{Node, Paint, Style, shape};

/// One path command in icon units. Subpaths open with `Move`; `Line`/`Quad` continue the open one.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum PathEl {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad { control: [f32; 2], to: [f32; 2] },
    Close,
}

/// Icons are authored on a 32x32 canvas.
pub const ICON_VIEW_BOX: Rect = Rect::new(0.0, 0.0, 32.0, 32.0);

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Ink {
    Fill,
    /// Stroke width in canvas units.
    Stroke(f64),
}

/// A `const`-authored icon. [`Icon::new`] is a `const fn` that rejects a draw command with no open subpath: bound to a
/// `const` that is a compile error, and anywhere else it is a panic at the first use, never a silently repaired path.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Icon {
    ops: &'static [PathEl],
    ink: Ink,
}

impl Icon {
    pub const fn new(ops: &'static [PathEl], ink: Ink) -> Self {
        let mut open = false;
        let mut i = 0;
        while i < ops.len() {
            match ops[i] {
                PathEl::Move(_) => open = true,
                PathEl::Line(_) | PathEl::Quad { .. } => {
                    assert!(open, "icon path draws before a Move (or after a Close)");
                }
                PathEl::Close => open = false,
            }
            i += 1;
        }
        Self { ops, ink }
    }

    fn path(&self) -> BezPath {
        let p = |v: [f32; 2]| (f64::from(v[0]), f64::from(v[1]));
        let mut path = BezPath::new();
        for op in self.ops {
            match *op {
                PathEl::Move(to) => path.move_to(p(to)),
                PathEl::Line(to) => path.line_to(p(to)),
                PathEl::Quad { control, to } => path.quad_to(p(control), p(to)),
                PathEl::Close => path.close_path(),
            }
        }
        path
    }

    /// The icon as a shape node scaled to fill `style`'s box.
    pub(crate) fn node<Msg>(&self, style: Style, color: Color) -> Node<Msg> {
        let paint = match self.ink {
            Ink::Fill => Paint::Fill(solid(color)),
            Ink::Stroke(width) => Paint::Stroke {
                brush: solid(color),
                stroke: Stroke::new(width),
            },
        };
        shape(
            style,
            ShapeSpec::Path {
                path: self.path(),
                view_box: ICON_VIEW_BOX,
            },
            paint,
        )
    }
}
