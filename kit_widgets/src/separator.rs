// Ported from egui 0.36.2 `crates/egui/src/widgets/separator.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Centred dividers; vertical separators stretch to their row, labelled dividers are horizontal.
use crate::common::{ink, solid};
use kit_core::Theme;
use kit_view::taffy::{AlignItems, AlignSelf};
use kit_view::{Node, Style, Weight, column, px, rect, row, text};
pub enum Divider<'a> {
    Horizontal,
    Vertical,
    Labelled(&'a str),
}
pub struct Separator<'a> {
    pub theme: &'a Theme,
    pub kind: Divider<'a>,
}
pub fn separator<Msg>(p: Separator<'_>) -> Node<Msg> {
    let th = p.theme;
    // egui's inactive separator stroke is 1 logical px; spacing comes from the kit theme.
    let mut horizontal = Style::row();
    horizontal.layout.size.height = px(1.0);
    let line = || rect(horizontal.clone(), solid(th.colors.field_border));
    match p.kind {
        Divider::Horizontal => column([line()]).pad_xy(0.0, th.spacing.sm).fill_width(),
        Divider::Vertical => {
            let mut vertical = Style::row();
            vertical.layout.size.width = px(1.0);
            row([rect(vertical, solid(th.colors.field_border))])
                .pad_xy(th.spacing.sm, 0.0)
                .align_self(AlignSelf::STRETCH)
        }
        Divider::Labelled(label) => row([
            line().grow(1.0),
            text(
                label,
                ink(th.type_scale.label, th.colors.text_muted, Weight::NORMAL),
            ),
            line().grow(1.0),
        ])
        .gap(th.spacing.md)
        .pad_xy(0.0, th.spacing.sm)
        .align_items(AlignItems::CENTER)
        .fill_width(),
    }
}
