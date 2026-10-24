//! Seam: shared vocabulary of the Keel UI kit. Every other `kit_*` crate depends on this one and on nothing Keel-specific.
//!
//! Geometry is `kurbo` (f64, logical pixels) and colour/brush/blend is `peniko`; both are re-exported so a backend
//! never needs its own conversion layer.

pub use kurbo;
pub use peniko;

mod id;
mod key;
mod scroll;
mod shape;
mod theme;
mod units;

pub use id::{Identity, StableHasher, WidgetId};
pub use key::{Key, Modifiers, NamedKey};
pub use scroll::ScrollView;
pub use shape::{Axis, Border, Clip, Shadow, Shape, ShapeSpec, Surface};
pub use theme::{
    ColorVision, ControlSizes, Motion, MotionPref, Palette, Radii, Spacing, Theme, TypeScale,
};
pub use units::{Epoch, Time, Viewport};
