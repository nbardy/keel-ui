//! `Gauge` and `meter`: health / mana / XP gauges.
use crate::common::{fill_parent, solid};
use kit_core::Theme;
use kit_core::kurbo::RoundedRectRadii;
use kit_core::peniko::Color;
use kit_view::{Node, Percent, Style, px, rounded, stack};

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum GaugeError {
    /// `max` must be a positive finite number, otherwise the fraction is meaningless.
    BadMax,
    /// `value` is NaN or infinite.
    BadValue,
}

/// A value against a positive maximum. Over-full and negative values are clamped: a health bar showing overheal or
/// a debuffed negative is full or empty, which is the visual meaning (the numeric label carries the real value).
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Gauge {
    value: f64,
    max: f64,
}

impl Gauge {
    pub fn new(value: f64, max: f64) -> Result<Self, GaugeError> {
        if !(max.is_finite() && max > 0.0) {
            return Err(GaugeError::BadMax);
        }
        if !value.is_finite() {
            return Err(GaugeError::BadValue);
        }
        Ok(Self {
            value: value.clamp(0.0, max),
            max,
        })
    }
    pub fn fraction(self) -> f64 {
        self.value / self.max
    }
}

/// Solid horizontal gauge that fills the width its parent gives it (the fill is a percentage of the track).
pub fn meter<Msg>(theme: &Theme, height: f64, gauge: Gauge, color: Color) -> Node<Msg> {
    let radii = RoundedRectRadii::from_single_radius((height / 2.0).min(theme.radii.md));
    let mut fill = fill_parent();
    fill.layout.size.width = Percent(gauge.fraction() * 100.0).into();
    let mut track = Style::anchors();
    track.layout.size.width = Percent(100.0).into();
    track.layout.size.height = px(height);
    stack(
        track,
        vec![
            rounded(fill_parent(), radii, solid(theme.colors.surface_inset)),
            rounded(fill, radii, solid(color)),
        ],
    )
}
