//! Scroll bars over a scroll area's clipped content: a thin pill while the area is hovered or moving, widened with a
//! track while the pointer is on the bar or holds the thumb. Geometry and visibility come from `kit_input`; this
//! only inks them with theme tokens.
use crate::Draw;
use kit_core::kurbo::{Rect, RoundedRectRadii, Size};
use kit_core::peniko::Brush;
use kit_core::{Border, Shadow, Theme, Time, WidgetId};
use kit_input::{BAR_STRIP, BAR_THIN, Bar, BarView, UiMemory};
use kit_view::ScrollSpec;

pub(crate) fn scrollbars(
    memory: &UiMemory,
    theme: &Theme,
    (id, spec): (WidgetId, &ScrollSpec),
    rect: Rect,
    content: Size,
    now: Time,
) -> Vec<Draw> {
    memory
        .scroll_bars(id, spec, rect, content, now)
        .into_iter()
        .filter(|b| b.opacity > 0.0)
        .flat_map(|b| bar(theme, b))
        .collect()
}

fn bar(theme: &Theme, b: BarView) -> Vec<Draw> {
    let inset = theme.spacing.xs;
    let thickness = match b.hot {
        true => BAR_STRIP - 2.0 * inset,
        false => BAR_THIN,
    };
    // Thin toward the area's outer edge: the right edge for a vertical bar, the bottom for a horizontal one.
    let thin = |r: Rect| match b.bar {
        Bar::Vertical => Rect::new(r.x1 - inset - thickness, r.y0, r.x1 - inset, r.y1),
        Bar::Horizontal => Rect::new(r.x0, r.y1 - inset - thickness, r.x1, r.y1 - inset),
    };
    let pill = |r: Rect, color: kit_core::peniko::Color| Draw::RoundRect {
        rect: r,
        radii: RoundedRectRadii::from_single_radius(thickness / 2.0),
        fill: Brush::Solid(color.multiply_alpha(b.opacity)),
        border: Border::None,
        shadow: Shadow::None,
    };
    let thumb = pill(thin(b.thumb), theme.colors.text_muted);
    match b.hot {
        true => vec![pill(thin(b.strip), theme.colors.surface_inset), thumb],
        false => vec![thumb],
    }
}
