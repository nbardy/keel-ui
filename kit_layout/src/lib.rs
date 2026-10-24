//! The kit layout: `TaffyLayout` (Node -> rects): flex, grid and anchors through
//! taffy 0.14. Anchors are the one thing taffy lacks (safe area, screen-space offset), so `place_anchored` resolves them.
//!
//! Conventions:
//! * The root node always fills the viewport (its own `size` is ignored). `Viewport::safe_area` is added to the root's
//!   padding, and anchored children of the root respect it unless they say `SafeArea::Ignore`.
//! * Kit percents are `kit_view::Percent` (0..=100); taffy fractions (0..=1) are only ever written by its `From`.
//! * `Anchor::offset` is a plain screen-space shift (+x right, +y down) applied after the edge alignment.
//! * `Node::World` floats: it takes no space in its parent, its child is centred on the projected point plus the
//!   anchor offset, and `Culled` projection culls the whole subtree (`Shown::Culled`, zero rects).
//! * Wrappers (`Interactive`, `Layer`) take their child's box; their rects equal the child's.

mod kind;
mod portals;
mod taffy_engine;

pub use taffy_engine::{Reuse, TaffyLayout, place_anchored};
