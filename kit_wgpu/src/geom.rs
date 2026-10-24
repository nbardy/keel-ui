//! Device-pixel integer rectangles (scissor state).
use kit_core::kurbo::Rect;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) struct DevRect {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl DevRect {
    pub fn full(width: u32, height: u32) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: width as i32,
            y1: height as i32,
        }
    }
    /// Nearest-pixel rounding: an axis-aligned rect clip lands on the pixel grid like the rect itself would.
    pub fn rounded(r: Rect) -> Self {
        Self {
            x0: r.x0.round() as i32,
            y0: r.y0.round() as i32,
            x1: r.x1.round() as i32,
            y1: r.y1.round() as i32,
        }
    }
    pub fn intersect(self, o: Self) -> Self {
        Self {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        }
    }
    pub fn is_empty(self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }
    pub fn as_rect(self) -> Rect {
        Rect::new(
            f64::from(self.x0),
            f64::from(self.y0),
            f64::from(self.x1),
            f64::from(self.y1),
        )
    }
    /// `(x, y, w, h)` for `set_scissor_rect`; only call on non-empty rects inside the target.
    pub fn scissor(self) -> (u32, u32, u32, u32) {
        (
            self.x0 as u32,
            self.y0 as u32,
            (self.x1 - self.x0) as u32,
            (self.y1 - self.y0) as u32,
        )
    }
}

pub(crate) fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}
