// Ported from egui 0.36.2 `crates/egui/src/widgets/{progress_bar,spinner}.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Validated determinate progress and a time-driven spinner; the host supplies frames and motion preference.
use crate::common::{fill_parent, ink, radii, solid};
use kit_core::kurbo::{Arc, Point, Rect, Shape, Stroke, Vec2};
use kit_core::{MotionPref, ShapeSpec, Theme};
use kit_state::{Cx, Host, Intent};
use kit_view::{
    AnchorH, AnchorV, Node, Paint, Percent, Style, Weight, clipped, nothing, px, rounded, shape,
    stack, text,
};

/// Progress must be finite and in the inclusive range 0..=1.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ProgressError;
pub struct ProgressBar<'a> {
    pub theme: &'a Theme,
    progress: f64,
    pub label: Option<&'a str>,
}
impl<'a> ProgressBar<'a> {
    pub fn new(theme: &'a Theme, progress: f64) -> Result<Self, ProgressError> {
        if !(0.0..=1.0).contains(&progress) {
            return Err(ProgressError);
        }
        Ok(Self {
            theme,
            progress,
            label: None,
        })
    }
}
pub fn progress<Msg>(p: ProgressBar<'_>) -> Node<Msg> {
    let (th, c) = (p.theme, &p.theme.colors);
    let (height, radius) = (th.controls.mini.1, th.radii.control);
    let mut fill = fill_parent();
    fill.layout.size.width = Percent(p.progress * 100.0).into();
    // egui keeps the rounded fill at least one diameter wide, including at zero.
    fill.layout.min_size.width = px(2.0 * radius.min(height / 2.0));
    let offset = Vec2::new(th.spacing.md, 0.0);
    let label_style = Style::row().anchored(AnchorH::Start, AnchorV::Center, offset);
    let caption = match p.label {
        Some(label) => {
            text(label, ink(th.type_scale.label, c.text, Weight::NORMAL)).styled(label_style)
        }
        None => nothing(),
    };
    let mut track = Style::anchors();
    track.layout.size.height = px(height);
    track.layout.size.width = Percent(100.0).into();
    let background = rounded(fill_parent(), radii(radius), solid(c.surface_inset));
    let filled = rounded(fill, radii(radius), solid(c.accent));
    clipped(stack(track, vec![background, filled, caption]))
}
pub struct Spinner<'a> {
    pub theme: &'a Theme,
}
pub fn spinner<A: Host>(cx: &Cx<'_, A>, p: Spinner<'_>) -> Node<Intent<A>> {
    let size = p.theme.controls.mini.1;
    let time = match cx.app().motion() {
        MotionPref::Full => cx.now().0.as_secs_f64(),
        MotionPref::Reduced => core::f64::consts::FRAC_PI_2,
    };
    // egui: 2px inset, 240-degree sinusoidal sweep and 3px stroke; kurbo owns tessellation.
    let radius = Vec2::splat(size / 2.0 - 2.0);
    let center = Point::new(size / 2.0, size / 2.0);
    let start = time * core::f64::consts::TAU;
    let sweep = 240_f64.to_radians() * time.sin();
    let path = Arc::new(center, radius, start, sweep, 0.0).to_path(p.theme.spacing.xs);
    let arc = ShapeSpec::Path {
        path,
        view_box: Rect::new(0.0, 0.0, size, size),
    };
    let paint = Paint::Stroke {
        brush: solid(p.theme.colors.text),
        stroke: Stroke::new(3.0),
    };
    shape(Style::row().sized(size, size), arc, paint)
}
