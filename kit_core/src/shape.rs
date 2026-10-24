use kurbo::{BezPath, Ellipse, Rect, RoundedRect, RoundedRectRadii, Shape as _, Vec2};
use peniko::{Brush, Color};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
    Both,
}

/// Concrete shape in absolute logical px (what a `DrawList` carries).
#[derive(Clone, PartialEq, Debug)]
pub enum Shape {
    Rect(Rect),
    RoundRect(RoundedRect),
    Ellipse(Ellipse),
    Path(BezPath),
}

impl Shape {
    /// The outline as a bezier path; curves flatten no coarser than `tolerance` (the shape's own units). The one
    /// conversion every backend uses, so they cannot disagree about what a shape is.
    pub fn to_bez(&self, tolerance: f64) -> BezPath {
        match self {
            Shape::Rect(r) => r.to_path(tolerance),
            Shape::RoundRect(r) => r.to_path(tolerance),
            Shape::Ellipse(e) => e.to_path(tolerance),
            Shape::Path(p) => p.clone(),
        }
    }
}

/// Shape described relative to a node's laid-out box (what a `Node` carries).
#[derive(Clone, PartialEq, Debug)]
pub enum ShapeSpec {
    Rect,
    RoundRect(RoundedRectRadii),
    Ellipse,
    /// `path` is authored in `view_box` coordinates and scaled to fill the node box.
    Path {
        path: BezPath,
        view_box: Rect,
    },
}

/// Layer clip in absolute coordinates: a scissor rectangle, never a shape mask.
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Clip {
    Unclipped,
    ToRect(Rect),
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Border {
    None,
    Solid { width: f64, color: Color },
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Shadow {
    None,
    Drop {
        offset: Vec2,
        blur: f64,
        spread: f64,
        color: Color,
    },
}

/// Background surface of a panel: fill + corner radii + border + shadow.
#[derive(Clone, PartialEq, Debug)]
pub struct Surface {
    pub fill: Brush,
    pub radii: RoundedRectRadii,
    pub border: Border,
    pub shadow: Shadow,
}
