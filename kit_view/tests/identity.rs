//! Identity: interactive nodes get a `WidgetId` from tree position unless the app keys them. `UiMemory` (hover, press,
//! drag) is keyed by that id, so it must survive rebuilding the tree every frame and must not leak between items
//! when a list reorders.
use kit_core::kurbo::Rect;
use kit_core::{Identity, WidgetId};
use kit_view::testing::{control, laid_out};
use kit_view::{
    A11y, Bindings, LaidOut, LayoutError, Node, Role, Toggle, column, interactive, nothing,
};

fn item(identity: Identity, label: &str) -> Node<()> {
    interactive(
        identity,
        Bindings::none(),
        A11y::Labeled {
            role: Role::Button,
            label: label.to_owned(),
            toggle: Toggle::NotToggle,
        },
        nothing(),
    )
}

/// A list of `labels`, each keyed by its own name when `keyed`, else by position.
fn list(labels: &[&str], keyed: bool) -> Node<()> {
    column(labels.iter().map(|label| {
        let identity = if keyed {
            Identity::Keyed(WidgetId::key(label))
        } else {
            Identity::Positional
        };
        item(identity, label)
    }))
}

fn assemble(root: Node<()>) -> Result<LaidOut<()>, LayoutError> {
    let rects = vec![Rect::ZERO; root.count()];
    Ok(laid_out(root, &rects))
}

fn id_of(laid: &LaidOut<()>, label: &str) -> WidgetId {
    control(laid, Role::Button, label)
        .and_then(|node| node.id())
        .unwrap_or_else(|| panic!("no control {label:?}"))
}

#[test]
fn positional_identity_is_stable_across_rebuilds_and_distinct_per_position() {
    let labels = ["a", "b", "c"];
    let first = assemble(list(&labels, false)).unwrap();
    let second = assemble(list(&labels, false)).unwrap();
    let ids: Vec<WidgetId> = labels.iter().map(|l| id_of(&first, l)).collect();
    assert_eq!(
        ids,
        labels.iter().map(|l| id_of(&second, l)).collect::<Vec<_>>(),
        "hover/press memory must survive a rebuild of the same tree"
    );
    assert!(
        ids[0] != ids[1] && ids[1] != ids[2] && ids[0] != ids[2],
        "siblings must not share memory"
    );
}

/// Regression guard for the reason `.key(..)` exists: a reordered list moves positional ids with the slots, so a
/// press held on "a" would transfer to whichever item now sits first. A keyed item keeps its id wherever it goes.
#[test]
fn keyed_items_keep_their_identity_when_a_list_reorders_and_positional_ones_do_not() {
    let before = ["a", "b", "c"];
    let after = ["c", "a", "b"];

    let keyed_before = assemble(list(&before, true)).unwrap();
    let keyed_after = assemble(list(&after, true)).unwrap();
    for label in before {
        assert_eq!(id_of(&keyed_before, label), id_of(&keyed_after, label));
    }

    let positional_before = assemble(list(&before, false)).unwrap();
    let positional_after = assemble(list(&after, false)).unwrap();
    assert_ne!(
        id_of(&positional_before, "a"),
        id_of(&positional_after, "a"),
        "a positional id belongs to the slot, not the item"
    );
}

#[test]
fn a_repeated_key_is_rejected_instead_of_sharing_memory_between_two_controls() {
    let root = column(vec![
        item(Identity::Keyed(WidgetId::key("same")), "one"),
        item(Identity::Keyed(WidgetId::key("same")), "two"),
    ]);
    let rects = vec![Rect::ZERO; root.count()];
    let placed: Vec<_> = rects
        .iter()
        .map(|&rect| kit_view::Placed {
            rect,
            shown: kit_view::Shown::Visible,
        })
        .collect();
    let viewport = kit_core::Viewport::new(kit_core::kurbo::Size::new(10.0, 10.0), 1.0);
    let result = LaidOut::assemble(
        root,
        viewport,
        &placed,
        &mut kit_view::testing::text_system(),
    );
    assert!(matches!(
        result,
        Err(LayoutError::DuplicateIdentity { id }) if id == WidgetId::key("same")
    ));
}

#[test]
fn a_keyed_node_does_not_collide_with_a_positional_one() {
    // The macro form: `column![..]` takes anything `Into<Node>`.
    let root: Node<()> = column![
        item(Identity::Positional, "p"),
        item(Identity::Keyed(WidgetId::key("k")), "k"),
    ];
    let laid = assemble(root).unwrap();
    assert_ne!(id_of(&laid, "p"), id_of(&laid, "k"));
    assert_eq!(id_of(&laid, "k"), WidgetId::key("k"));
}
