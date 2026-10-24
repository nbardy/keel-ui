//! A scrolling list of `items` that builds and lays out only the rows in view. The scroll extent is always all rows,
//! so the bar, wheel, drag and momentum behave as for a list that built them all.
//!
//! Scrolling is the ordinary scroll area (`scroll_area`, `ScrollSpec`, the offset in the router's memory under the
//! list's key); this module adds a data-shaped surface to [`virtual_rows`]: the caller hands over the items and a row
//! function from an item, never an index, so a row cannot be tied to its position by accident.
//!
//! Limits (v1): every row has the same `row_height`. Variable heights need a measured-height cache and are not built.
//! There is no list-owned keyboard navigation: a row is reachable by Tab only while it is built, and nothing scrolls
//! a focused-but-unbuilt neighbour into view. For a cursor, use [`ScrollSpec::reveal`] with [`reveal_row`] on the
//! area, or [`crate::listbox`] for short lists.
//!
//! Identity: the rows a window builds change as it moves, so a row's controls must be keyed by the item's data key
//! (`interactive(WidgetId::of(&(list_key, item.id)), ..)`) and its local state by `cx.state(&Row(item.id), ..)`, as in
//! the README's keyed list. A key taken from the position would move state to a different item on every scroll.
use crate::virtual_rows;
use kit_state::{Cx, Host, Ident};
use kit_view::{Node, Style, px};

pub struct VirtualList<'a, Msg, K, I> {
    pub key: K,
    pub items: &'a [I],
    /// Panics unless finite and positive.
    pub row_height: f64,
    /// The viewport's height. Panics unless finite and positive.
    pub height: f64,
    /// One row for one item; called only for the rows in the window, at most `row_height` tall.
    pub row: &'a dyn Fn(&I) -> Node<Msg>,
}

pub fn virtual_items<A: Host, K: Ident<A>, I, Msg>(
    cx: &Cx<'_, A>,
    p: VirtualList<'_, Msg, K, I>,
) -> Node<Msg> {
    assert!(
        p.row_height.is_finite() && p.row_height > 0.0,
        "virtual_list: row_height must be finite and positive"
    );
    assert!(
        p.height.is_finite() && p.height > 0.0,
        "virtual_list: height must be finite and positive"
    );
    let mut viewport = Style::column();
    viewport.layout.size.height = px(p.height);
    viewport.layout.flex_shrink = 0.0;
    let list = virtual_rows(cx, &p.key, p.items.len(), p.row_height, |i| {
        (p.row)(&p.items[i])
    });
    list.styled(viewport)
}
