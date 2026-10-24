//! `virtual_rows`: a vertical list of `row_count` fixed-height rows that builds only the rows in view, plus
//! `OVERSCAN` on each side. Offsets, momentum, bars, stick-to-end and scroll-to come from the scroll area itself.
//!
//! Rows must be keyed by data (`.key(&item.id)` or `interactive(WidgetId::of(..), ..)`): a positional id comes from
//! the tree position, which slides as the window of built rows moves.
use kit_core::kurbo::Rect;
use kit_core::{Axis, ScrollView, WidgetId};
use kit_state::{Cx, Host, Ident};
use kit_view::{Node, RevealTarget, ScrollSpec, Style, column, px, scroll_area};
use std::ops::Range;

/// Rows built beyond each edge of the viewport, so a fast fling does not show a gap for a frame.
pub const OVERSCAN: usize = 3;

/// `virtual_list` with a plain vertical area. Size the result like any node (`.size(..)`, `.grow(1.0)`).
pub fn virtual_rows<A: Host, K: Ident<A>, Msg>(
    cx: &Cx<'_, A>,
    key: &K,
    row_count: usize,
    row_height: f64,
    row: impl Fn(usize) -> Node<Msg>,
) -> Node<Msg> {
    let spec = ScrollSpec::new(Axis::Vertical);
    virtual_list(cx, key, spec, row_count, row_height, row)
}

/// A virtual list in an area with `spec` (stick to the end for chat, reveal a row with [`reveal_row`]).
pub fn virtual_list<A: Host, K: Ident<A>, Msg>(
    cx: &Cx<'_, A>,
    key: &K,
    spec: ScrollSpec,
    row_count: usize,
    row_height: f64,
    row: impl Fn(usize) -> Node<Msg>,
) -> Node<Msg> {
    let rows = visible_rows(cx.scroll(key), row_count, row_height);
    let tall = |height: f64, children: Vec<Node<Msg>>| {
        let mut style = Style::column();
        style.layout.size.height = px(height);
        style.layout.flex_shrink = 0.0;
        kit_view::stack(style, children)
    };
    let before = tall(rows.start as f64 * row_height, Vec::new());
    let after = tall((row_count - rows.end) as f64 * row_height, Vec::new());
    let built = rows.map(|i| tall(row_height, vec![row(i)]));
    let content = column(std::iter::once(before).chain(built).chain([after]));
    scroll_area(WidgetId::of(key), spec, Style::column(), content)
}

/// Rows `[start, end)` that intersect the viewport, widened by `OVERSCAN`. An area the router has not measured yet
/// builds the first `OVERSCAN` rows (one frame; the next frame has the real window).
pub fn visible_rows(view: ScrollView, row_count: usize, row_height: f64) -> Range<usize> {
    match view {
        ScrollView::Unseen => 0..OVERSCAN.min(row_count),
        ScrollView::Seen {
            offset, viewport, ..
        } => {
            let first = (offset.y / row_height).floor().max(0.0) as usize;
            let last = ((offset.y + viewport.height) / row_height).ceil().max(0.0) as usize;
            first.saturating_sub(OVERSCAN).min(row_count)..(last + OVERSCAN).min(row_count)
        }
    }
}

/// The content rect of row `index`, for `ScrollSpec::reveal` (the row may not be built yet).
pub fn reveal_row(index: usize, row_height: f64) -> RevealTarget {
    let top = index as f64 * row_height;
    RevealTarget::Content(Rect::new(0.0, top, 0.0, top + row_height))
}
