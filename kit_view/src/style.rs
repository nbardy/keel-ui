//! Box style: taffy's own [`taffy::Style`] used directly, plus the two things taffy has no word for (screen anchors with
//! a safe area, and a container that positions its children by anchor). Re-exported as `kit_view::taffy`, so a
//! consumer names taffy types without a dependency of its own.
//!
//! Percent trap: taffy's percent constructors take a `0..=1` fraction. Kit code writes every percent through
//! [`Percent`] (`0..=100`, CSS units), the one place the two scales meet; never call taffy's `percent` directly.
use kit_core::kurbo::Vec2;
pub use taffy;
use taffy as tf;

/// A CSS percentage, `0..=100` (`Percent(50.0)` is half), not a `0..=1` fraction. Converting it is the only place a
/// kit percent becomes a taffy one; `taffy::Dimension::percent(100.0)` written by hand would be 100x too large.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Percent(pub f64);
impl Percent {
    fn fraction(self) -> f32 {
        (self.0 / 100.0) as f32
    }
}
impl From<Percent> for tf::Dimension {
    fn from(p: Percent) -> Self {
        Self::percent(p.fraction())
    }
}
impl From<Percent> for tf::LengthPercentageAuto {
    fn from(p: Percent) -> Self {
        Self::percent(p.fraction())
    }
}
impl From<Percent> for tf::LengthPercentage {
    fn from(p: Percent) -> Self {
        Self::percent(p.fraction())
    }
}

/// Logical px as a taffy length (taffy lays out in `f32`).
pub fn px<T: tf::style_helpers::FromLength>(v: f64) -> T {
    tf::style_helpers::length(v as f32)
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum AnchorH {
    Start,
    Center,
    End,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum AnchorV {
    Start,
    Center,
    End,
}
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum SafeArea {
    /// Anchor to the safe rect (inside notch/home indicator).
    Respect,
    /// Anchor to the full box.
    Ignore,
}
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Anchor {
    pub h: AnchorH,
    pub v: AnchorV,
    pub offset: Vec2,
    pub safe_area: SafeArea,
}

/// How a node is placed by its parent.
#[derive(Clone, PartialEq, Debug)]
pub enum Placement {
    /// By the parent's taffy layout, from the item fields of [`Style::layout`] (flex grow/shrink/basis, align-self,
    /// grid row/column). A grid row/column outside a grid parent is `LayoutError::CellOutsideGrid` (taffy would
    /// silently ignore it).
    Flow,
    /// Against the parent's box (taffy has no safe-area anchor).
    Anchored(Anchor),
}

/// How a node arranges its children.
#[derive(Clone, PartialEq, Debug)]
pub enum Content {
    /// taffy lays them out from the container fields of [`Style::layout`] (display, flex or grid).
    Taffy,
    /// Each child is positioned against this box by its `Anchor`; a `Flow` child sits top-left of the safe area
    /// (HUD layout). An auto-sized anchors box below the root fills its host: it has no content size.
    Anchors,
}

/// A node's box. `layout.border` is not drawn (a `Surface` carries the visible border) and the layout engine owns the
/// root's: it is the viewport's safe area.
#[derive(Clone, PartialEq, Debug)]
pub struct Style {
    pub layout: tf::Style,
    pub placement: Placement,
    pub content: Content,
}

impl Default for Style {
    fn default() -> Self {
        Self::column()
    }
}

impl Style {
    fn flex(direction: tf::FlexDirection) -> Self {
        Self {
            layout: tf::Style {
                display: tf::Display::Flex,
                flex_direction: direction,
                align_items: Some(tf::AlignItems::STRETCH),
                justify_content: Some(tf::JustifyContent::FLEX_START),
                // Wrapped lines pack at the start (taffy's default spreads them over the container's free cross space).
                align_content: Some(tf::AlignContent::FLEX_START),
                ..tf::Style::DEFAULT
            },
            placement: Placement::Flow,
            content: Content::Taffy,
        }
    }
    pub fn row() -> Self {
        Self::flex(tf::FlexDirection::Row)
    }
    pub fn column() -> Self {
        Self::flex(tf::FlexDirection::Column)
    }
    pub fn anchors() -> Self {
        Self {
            layout: tf::Style::DEFAULT,
            placement: Placement::Flow,
            content: Content::Anchors,
        }
    }
    /// Tracks are taffy's (`taffy::style_helpers::{length, fr, flex, auto}`; `flex(1.0)` is `minmax(0, 1fr)`).
    pub fn grid(
        columns: Vec<tf::GridTemplateComponent<String>>,
        rows: Vec<tf::GridTemplateComponent<String>>,
    ) -> Self {
        Self {
            layout: tf::Style {
                display: tf::Display::Grid,
                grid_template_columns: columns,
                grid_template_rows: rows,
                ..tf::Style::DEFAULT
            },
            placement: Placement::Flow,
            content: Content::Taffy,
        }
    }
    pub fn sized(mut self, width: f64, height: f64) -> Self {
        self.layout.size = tf::Size {
            width: px(width),
            height: px(height),
        };
        self
    }
    pub fn padded(self, all: f64) -> Self {
        self.padded_xy(all, all)
    }
    pub fn padded_xy(mut self, x: f64, y: f64) -> Self {
        self.layout.padding = tf::Rect {
            left: px(x),
            right: px(x),
            top: px(y),
            bottom: px(y),
        };
        self
    }
    pub fn margined(mut self, all: f64) -> Self {
        self.layout.margin = tf::Rect {
            left: px(all),
            right: px(all),
            top: px(all),
            bottom: px(all),
        };
        self
    }
    pub fn placed(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }
    pub fn anchored(self, h: AnchorH, v: AnchorV, offset: Vec2) -> Self {
        self.placed(Placement::Anchored(Anchor {
            h,
            v,
            offset,
            safe_area: SafeArea::Respect,
        }))
    }
    pub fn grow(mut self, grow: f32) -> Self {
        self.layout.flex_grow = grow;
        self
    }
    /// Gap between flex children or grid cells (both axes).
    pub fn gap(mut self, gap: f64) -> Self {
        self.layout.gap = tf::Size {
            width: px(gap),
            height: px(gap),
        };
        self
    }
    /// How this box sits on its parent's cross axis (an anchored box ignores it).
    pub fn align_self(mut self, align: tf::AlignSelf) -> Self {
        self.layout.align_self = Some(align);
        self
    }
    pub fn align_items(mut self, align: tf::AlignItems) -> Self {
        self.layout.align_items = Some(align);
        self
    }
    /// Break flex children onto further lines when they do not fit.
    pub fn wrap(mut self) -> Self {
        self.layout.flex_wrap = tf::FlexWrap::Wrap;
        self
    }
    pub fn justify(mut self, justify: tf::JustifyContent) -> Self {
        self.layout.justify_content = Some(justify);
        self
    }
}
