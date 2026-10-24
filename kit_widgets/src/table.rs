// Ported from egui 0.36.2 `crates/egui/src/grid.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Fixed-stride virtual table, with a stationary header and caller-owned sorting/selection.
//! Grid alignment and visible rows follow egui Grid/ScrollArea::show_rows; the three-way sort cycle is kit behavior.
use crate::common::{ink, surface};
use crate::virtual_rows;
use kit_core::{Border, Shadow, Theme, WidgetId};
use kit_state::{Cx, Host, Ident, Intent};
use kit_view::taffy::{
    AlignItems,
    style_helpers::{flex, length},
};
use kit_view::{
    A11y, Bindings, Node, Role, Style, TextWrap, Toggle, Weight, clipped, column, interactive,
    panel, px, text,
};
use std::{cmp::Ordering, hash::Hash};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    None,
    Ascending(usize),
    Descending(usize),
}
impl Sort {
    pub fn next(self, column: usize) -> Self {
        match self {
            Self::Ascending(i) if i == column => Self::Descending(column),
            Self::Descending(i) if i == column => Self::None,
            Self::None | Self::Ascending(_) | Self::Descending(_) => Self::Ascending(column),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Width {
    Fixed(f64),
    Fraction(f32),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sorting {
    Sortable,
    Plain,
}
pub struct Column<'a> {
    pub label: &'a str,
    pub width: Width,
    pub sorting: Sorting,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection<R> {
    None,
    Row(R),
}
pub struct Table<'a, T, R, Msg, K> {
    pub theme: &'a Theme,
    /// The scroll area's data key; row and header identities are namespaced under it.
    pub key: K,
    pub columns: &'a [Column<'a>],
    /// Original order, restored by Sort::None. Equal sort values retain this order.
    pub rows: &'a [T],
    /// Unique, stable data identity, independent of the displayed row position.
    pub row_id: &'a dyn Fn(&T) -> R,
    pub row_label: &'a dyn Fn(&T) -> String,
    pub cell: &'a dyn Fn(&T, usize) -> String,
    pub compare: &'a dyn Fn(usize, &T, &T) -> Ordering,
    pub sort: Sort,
    pub on_sort: &'a dyn Fn(Sort) -> Msg,
    pub selected: Selection<R>,
    pub on_select: &'a dyn Fn(R) -> Msg,
    /// Positive finite dimensions in logical px; height is the body viewport (excluding header).
    pub row_height: f64,
    pub height: f64,
}
/// Panics for empty columns, invalid dimensions/widths, duplicate row ids, or a sort naming a plain/missing column.
pub fn table<A: Host, K: Ident<A>, T, R: Eq + Hash>(
    cx: &Cx<'_, A>,
    p: Table<'_, T, R, Intent<A>, K>,
) -> Node<Intent<A>> {
    let (th, c, columns) = (p.theme, &p.theme.colors, p.columns);
    assert!(!p.columns.is_empty(), "table: columns must not be empty");
    assert!(
        p.row_height.is_finite() && p.row_height > 0.0 && p.height.is_finite() && p.height > 0.0,
        "table: row and viewport heights must be positive and finite"
    );
    let tracks = columns
        .iter()
        .map(|col| match col.width {
            Width::Fixed(w) => {
                assert!(w.is_finite() && w > 0.0, "table: invalid fixed width");
                length(w as f32)
            }
            Width::Fraction(w) => {
                assert!(w.is_finite() && w > 0.0, "table: invalid fraction");
                flex(w)
            }
        })
        .collect();
    let mut grid = Style::grid(tracks, vec![length(p.row_height as f32)]);
    grid.layout.flex_shrink = 0.0;
    let mut order: Vec<_> = (0..p.rows.len()).collect();
    let mut ids = std::collections::HashSet::new();
    let unique = p.rows.iter().all(|r| ids.insert((p.row_id)(r)));
    assert!(unique, "table: duplicate row id");
    match p.sort {
        Sort::None => {}
        Sort::Ascending(i) | Sort::Descending(i) => {
            let sortable = i < columns.len() && columns[i].sorting == Sorting::Sortable;
            assert!(sortable, "table: sort must name a sortable column");
            order.sort_by(|&a, &b| {
                let cmp = (p.compare)(i, &p.rows[a], &p.rows[b]);
                match p.sort {
                    Sort::Ascending(_) => cmp,
                    Sort::Descending(_) => cmp.reverse(),
                    Sort::None => unreachable!("sort validated above"),
                }
            });
        }
    }
    let cell = |label, weight| {
        let mut s = Style::row().padded_xy(th.spacing.md, 0.0);
        s.layout.align_items = Some(AlignItems::CENTER);
        s.layout.min_size.width = px(0.0);
        let style = ink(th.type_scale.label, c.text, weight).wrap(TextWrap::None);
        let label = text(label, style);
        clipped(kit_view::stack(s, vec![label]))
    };
    let a11y = |role, label, toggle| A11y::Labeled {
        role,
        label,
        toggle,
    };
    let headers = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            let (suffix, toggle) = match p.sort {
                Sort::Ascending(j) if i == j => (" ^", Toggle::On),
                Sort::Descending(j) if i == j => (" v", Toggle::On),
                Sort::None | Sort::Ascending(_) | Sort::Descending(_) => ("", Toggle::Off),
            };
            let face = cell(format!("{}{suffix}", col.label), Weight::BOLD);
            match col.sorting {
                Sorting::Plain => face,
                Sorting::Sortable => interactive(
                    WidgetId::of(&(&p.key, "header", i)),
                    Bindings::activate((p.on_sort)(p.sort.next(i))),
                    a11y(Role::Button, col.label.into(), toggle),
                    face,
                ),
            }
        })
        .collect();
    let frame = |fill, cells| {
        let surface = surface(fill, th.radii.sm, Border::None, Shadow::None);
        panel(grid.clone(), surface, cells)
    };
    let header = frame(c.surface_raised, headers);
    let body = virtual_rows(cx, &p.key, p.rows.len(), p.row_height, |i| {
        let data = &p.rows[order[i]];
        let id = (p.row_id)(data);
        let (fill, toggle) = match &p.selected {
            Selection::Row(selected) if *selected == id => (c.accent, Toggle::On),
            Selection::None | Selection::Row(_) => (c.surface, Toggle::Off),
        };
        let cells = (0..p.columns.len())
            .map(|j| cell((p.cell)(data, j), Weight::NORMAL))
            .collect();
        let face = frame(fill, cells);
        interactive(
            WidgetId::of(&(&p.key, "row", &id)),
            Bindings::activate((p.on_select)(id)),
            a11y(Role::Option, (p.row_label)(data), toggle),
            face,
        )
    });
    let mut viewport = Style::column();
    viewport.layout.size.height = px(p.height);
    viewport.layout.flex_shrink = 0.0;
    column([header, body.styled(viewport)]).fill_width()
}
