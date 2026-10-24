//! Screen regions: a HUD is nine named places, each holding one flex row/column, instead of per-control pixel pins.
//!
//! `Screen` lays the regions out as a 3x3 grid (columns `fit-content(30%) | minmax(0, 1fr) | fit-content(30%)`, rows
//! `auto | minmax(0, 1fr) | auto`), so the left and right columns are as wide as their widest region up to 30%, the centre column takes what is left (and may be narrower than its
//! content), and a region's content is pushed to its own corner/edge of its cell. The CENTRE and BOTTOM regions span
//! all three columns (their side cells overlap them, pushed to their own edges): the side columns are sized by the
//! TOP corners' toolbars, and on a 430 px phone they left the centre ~56 px, so a bottom row of tiles stacked one per
//! line off the screen and the open dock shop ran off both sides (kit flip, 2026-10-23; `keel_preview`
//! `on_phone_screens_...` guards it). The safe area comes for free: a root container pads by `Viewport::safe_area`, and the
//! screen is the root. Every build emits all nine cells (empty ones are empty stacks), so the tree's shape, and with
//! it positional widget identity, does not depend on which regions a frame happens to use.
use crate::{Node, Percent, Style, nothing, stack};
use taffy::style_helpers::{auto, fit_content, flex, line, span};
use taffy::{AlignItems as Align, JustifyContent as Justify};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Region {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Region {
    /// Grid auto-placement order: row by row.
    const ALL: [Region; 9] = [
        Region::TopLeft,
        Region::Top,
        Region::TopRight,
        Region::Left,
        Region::Center,
        Region::Right,
        Region::BottomLeft,
        Region::Bottom,
        Region::BottomRight,
    ];
    /// The cell: (row line, column line, columns spanned). Explicit, so the spanning bottom may overlap its corners.
    fn cell(self) -> (i16, i16, u16) {
        match self {
            Region::TopLeft => (1, 1, 1),
            Region::Top => (1, 2, 1),
            Region::TopRight => (1, 3, 1),
            Region::Left => (2, 1, 1),
            Region::Center => (2, 1, 3),
            Region::Right => (2, 3, 1),
            Region::BottomLeft => (3, 1, 1),
            Region::Bottom => (3, 1, 3),
            Region::BottomRight => (3, 3, 1),
        }
    }
    /// (vertical, horizontal) placement of the content inside the region's cell.
    fn place(self) -> (Justify, Align) {
        match self {
            Region::TopLeft => (Justify::FLEX_START, Align::FLEX_START),
            Region::Top => (Justify::FLEX_START, Align::CENTER),
            Region::TopRight => (Justify::FLEX_START, Align::FLEX_END),
            Region::Left => (Justify::CENTER, Align::FLEX_START),
            Region::Center => (Justify::CENTER, Align::CENTER),
            Region::Right => (Justify::CENTER, Align::FLEX_END),
            Region::BottomLeft => (Justify::FLEX_END, Align::FLEX_START),
            Region::Bottom => (Justify::FLEX_END, Align::CENTER),
            Region::BottomRight => (Justify::FLEX_END, Align::FLEX_END),
        }
    }
}

/// The nine regions of a screen. Setting a region twice replaces its content.
pub struct Screen<Msg> {
    contents: [Node<Msg>; 9],
}

impl<Msg> Default for Screen<Msg> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Msg> Screen<Msg> {
    pub fn new() -> Self {
        Self {
            contents: std::array::from_fn(|_| nothing()),
        }
    }
    fn at(mut self, region: Region, content: impl Into<Node<Msg>>) -> Self {
        // Discriminants follow `Region::ALL`, which `from` zips with the contents.
        self.contents[region as usize] = content.into();
        self
    }
    pub fn top_left(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::TopLeft, content)
    }
    pub fn top(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::Top, content)
    }
    pub fn top_right(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::TopRight, content)
    }
    pub fn left(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::Left, content)
    }
    pub fn center(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::Center, content)
    }
    pub fn right(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::Right, content)
    }
    pub fn bottom_left(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::BottomLeft, content)
    }
    pub fn bottom(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::Bottom, content)
    }
    pub fn bottom_right(self, content: impl Into<Node<Msg>>) -> Self {
        self.at(Region::BottomRight, content)
    }
}

impl<Msg> From<Screen<Msg>> for Node<Msg> {
    /// The whole screen as a root node: fill the viewport, pad nothing (add `.pad(..)` for a screen margin).
    fn from(screen: Screen<Msg>) -> Self {
        let cells = Region::ALL
            .into_iter()
            .zip(screen.contents)
            .map(|(region, content)| {
                let (vertical, horizontal) = region.place();
                let (row, column, columns) = region.cell();
                let mut style = Style::column().justify(vertical).align_items(horizontal);
                style.layout.grid_row = taffy::Line {
                    start: line(row),
                    end: span(1),
                };
                style.layout.grid_column = taffy::Line {
                    start: line(column),
                    end: span(columns),
                };
                stack(style, vec![content])
            })
            .collect();
        // `flex(1.0)` is `minmax(0, 1fr)`: with a plain `1fr` (min-content floor) wide bottom content pushed the right
        // column off screen below ~600 px. The side COLUMNS are `fit-content(30%)`, not `auto`: grid sizing grows auto
        // tracks to their max-content before a flexible track gets anything, so on a 430 px phone the corner toolbars
        // took all but 20 px and the centre's status lines wrapped one word per line down the screen. A side column is
        // now at most 30% of the width unless its content cannot break (then its min-content). Rows stay `auto`.
        let side = || fit_content(Percent(30.).into());
        let columns = vec![side(), flex(1.0), side()];
        let rows = vec![auto(), flex(1.0), auto()];
        stack(Style::grid(columns, rows), cells)
    }
}
