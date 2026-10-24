//! Separator goldens and a regression guard for cross-axis stretch in a centred row.
use crate::support::{Handle, TestApp, theme};
use kit_core::kurbo::Size;
use kit_core::{Theme, Viewport};
use kit_layout::TaffyLayout;
use kit_state::{Cx, Intent};
use kit_view::taffy::AlignItems;
use kit_view::testing::text_system;
use kit_view::{NoWorld, Node, TextStyle, column, row, text};
use kit_widgets::separator::{Divider, Separator, separator};

fn view(_: &Cx<'_, TestApp>, th: &Theme) -> Node<Intent<TestApp>> {
    let label = |s| text(s, TextStyle::new(th.type_scale.body as f32, th.colors.text));
    let divider = |kind| separator(Separator { theme: th, kind });
    column([
        label("Horizontal"),
        divider(Divider::Horizontal),
        row([
            label("Port"),
            divider(Divider::Vertical),
            label("Starboard"),
        ])
        .size(260.0, 52.0)
        .gap(th.spacing.md)
        .align_items(AlignItems::CENTER),
        divider(Divider::Labelled("Crew")),
        label("Ready to sail"),
    ])
    .pad(th.spacing.md)
    .gap(th.spacing.sm)
}

#[test]
fn golden_horizontal_vertical_and_labelled() {
    Handle::new(TestApp::new(), (300.0, 180.0), view).assert_golden("separator_variants");
}

#[test]
fn vertical_separator_fills_height_even_when_row_centres_other_children() {
    let th = theme();
    // The root is a column: the tested row must size itself from its tall sibling,
    // rather than accidentally inheriting the viewport's height.
    let label = text(
        "Tall sibling",
        TextStyle::new(th.type_scale.body as f32, th.colors.text),
    )
    .size(100.0, 64.0);
    let node: Node<()> = column([row([
        label,
        separator(Separator {
            theme: &th,
            kind: Divider::Vertical,
        }),
    ])
    .align_items(AlignItems::CENTER)]);
    let laid = TaffyLayout::new()
        .layout(
            node,
            Viewport::new(Size::new(240.0, 120.0), 1.0),
            &mut text_system(),
            &NoWorld,
        )
        .expect("separator row lays out");
    let row = &laid.root.children()[0];
    let divider = &row.children()[1];
    let stroke = &divider.children()[0];
    assert_eq!(row.rect.height(), 64.0, "the tall sibling sets row height");
    assert_eq!(divider.rect.y0, row.rect.y0, "separator starts at row top");
    assert_eq!(divider.rect.y1, row.rect.y1, "separator reaches row bottom");
    assert_eq!(
        stroke.rect.y0, row.rect.y0,
        "visible stroke starts at row top"
    );
    assert_eq!(
        stroke.rect.y1, row.rect.y1,
        "visible stroke reaches row bottom"
    );
}
