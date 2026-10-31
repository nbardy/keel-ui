//! Behaviour that a refactor could silently break: bindings, drop typing, announcements.
mod common;

use common::*;
use kit_core::Key;
use kit_core::kurbo::Vec2;
use kit_view::{
    Anchor, AnchorH, AnchorV, DragPayload, Dropped, Node, Placement, Role, SafeArea, SemanticState,
    Shortcut, Toggle, anchors, semantics,
};
use kit_widgets::*;

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Press,
    Drop(Dropped),
}

/// A malformed authored icon must fail loudly (at compile time when bound to a `const`), never be repaired.
#[test]
#[should_panic(expected = "draws before a Move")]
fn icon_that_draws_before_any_move_is_rejected() {
    let _ = Icon::new(&[PathEl::Line([1.0, 1.0])], Ink::Fill);
}

#[test]
fn malformed_gauge_input_is_a_typed_error() {
    assert_eq!(Gauge::new(1.0, 0.0), Err(GaugeError::BadMax));
    let max = std::num::NonZeroU32::new(120).unwrap();
    assert_eq!(Gauge::of_whole(30, max).fraction(), 0.25);
    assert_eq!(
        Gauge::of_whole(500, max).fraction(),
        1.0,
        "over-full is full"
    );
    assert_eq!(Gauge::of_whole(30, max), Gauge::new(30.0, 120.0).unwrap());
}

/// A tile named `label`, `x` px from the left, configured by the builder chain under test.
fn tile_at(
    label: &str,
    x: f64,
    configure: impl for<'a> FnOnce(Button<'a, Msg>) -> Button<'a, Msg>,
) -> Node<Msg> {
    let th = theme();
    let ui = Ui::new(&th);
    let tile: Node<Msg> = configure(ui.button(arrow(), label).size(ButtonSize::Tile)).into();
    let style = tile.style().clone().placed(Placement::Anchored(Anchor {
        h: AnchorH::Start,
        v: AnchorV::Start,
        offset: Vec2::new(x, 0.0),
        safe_area: SafeArea::Respect,
    }));
    tile.styled(style)
}

/// The HUD relies on this: an information tile (no `on_press`) eats the press, so it never becomes a move order for
/// the world behind it, while a disabled one is transparent to hits, exactly like the `keel_ui` regions they replace.
#[test]
fn inert_tile_swallows_presses_and_disabled_tile_lets_them_through() {
    let under = || tile_at("under", 0.0, |b| b.on_press(Msg::Press));
    let info = tile_at("over", 0.0, |b| b);
    let off = tile_at("over", 0.0, |b| b.on_press(Msg::Press).enabled(false));
    for (over, reaches_below) in [(info, false), (off, true)] {
        let mut s = scene(anchors(vec![under(), over]), 100.0, 60.0);
        let at = s.center_of_label("under");
        assert_eq!(
            s.click(at),
            if reaches_below {
                vec![Msg::Press]
            } else {
                vec![]
            }
        );
    }
}

#[test]
fn tile_drag_reaches_an_accepting_enabled_tile_and_a_disabled_tile_is_not_a_drop_target() {
    const TOKEN: u32 = 4;
    let drop = |enabled: bool| {
        let target = tile_at("slot", 80.0, |b| {
            b.on_press(Msg::Press)
                .drop_target(TOKEN, Msg::Drop)
                .enabled(enabled)
        });
        let source = tile_at("token", 0.0, |b| {
            b.on_press(Msg::Press).drag_source(DragPayload {
                kind: TOKEN,
                value: 9,
            })
        });
        let mut s = scene(anchors(vec![source, target]), 160.0, 60.0);
        let (from, to) = (s.center_of_label("token"), s.center_of_label("slot"));
        s.drag(from, to)
    };
    let landed = drop(true);
    assert!(
        matches!(landed.as_slice(), [Msg::Drop(d)] if d.payload.value == 9),
        "{landed:?}"
    );
    assert_eq!(drop(false), Vec::<Msg>::new());
}

fn semantic_of(root: Node<Msg>, role: Role, label: &str) -> kit_view::Semantic {
    let s = scene(root, 300.0, 120.0);
    semantics(&s.laid)
        .into_iter()
        .find(|semantic| semantic.role == role && semantic.label == label)
        .unwrap_or_else(|| panic!("no {role:?} named {label:?}"))
}

/// J-web-8: a gold counter was announced as a disabled button. Information is a label, a merely unavailable command
/// stays a disabled button, and neither takes presses.
#[test]
fn an_information_tile_is_a_label_and_an_unavailable_command_is_a_disabled_button() {
    let info = tile_at("Gold", 0.0, |b| b.info());
    let blocked = tile_at("Sell", 0.0, |b| b.on_press(Msg::Press).enabled(false));
    let gold = semantic_of(anchors(vec![info]), Role::Label, "Gold");
    assert_eq!(gold.state, SemanticState::Disabled);
    let sell = semantic_of(anchors(vec![blocked]), Role::Button, "Sell");
    assert_eq!(sell.state, SemanticState::Disabled);
}

/// J-web-1: "selected" must reach the accessibility tree whether on or off, and a control that never was a toggle
/// must not claim to be one (a screen reader would say "not pressed" about a plain command).
#[test]
fn selection_is_announced_as_a_toggle_state_only_where_the_builder_set_it() {
    let on = tile_at("Tool", 0.0, |b| b.on_press(Msg::Press).selected(true));
    let off = tile_at("Tool", 0.0, |b| b.on_press(Msg::Press).selected(false));
    let plain = tile_at("Tool", 0.0, |b| b.on_press(Msg::Press));
    let read = |node| semantic_of(anchors(vec![node]), Role::Button, "Tool").toggle;
    assert_eq!(
        [read(on), read(off), read(plain)],
        [Toggle::On, Toggle::Off, Toggle::NotToggle]
    );
}

/// J-web-2: a hotkey is a binding. An enabled button fires on its key; a disabled one does not; the key is exposed so
/// the host can mirror it as `aria-keyshortcuts`.
#[test]
fn a_hotkey_presses_the_button_unless_it_is_disabled() {
    let key = Shortcut::Key(Key::character('m'));
    let make = |enabled: bool| {
        let tile = tile_at("Mute", 0.0, |b| {
            b.on_press(Msg::Press).hotkey(key).enabled(enabled)
        });
        scene(anchors(vec![tile]), 100.0, 80.0)
    };
    let mut on = make(true);
    let press = on.key('M');
    assert_eq!(press, vec![Msg::Press], "Shift+M is the same key as m");
    let exposed = semantics(&on.laid)[0].shortcut;
    assert_eq!(exposed, key);
    assert_eq!(make(false).key('m'), Vec::<Msg>::new());
}

/// J16: a card names the item and its price both in what is drawn and in what is announced.
#[test]
fn a_card_announces_name_and_price_and_grows_with_its_words() {
    let th = theme();
    let ui = Ui::new(&th);
    let card: Node<Msg> = ui
        .card(arrow(), "Chainshot", "120g")
        .size(ButtonSize::Tile)
        .on_press(Msg::Press)
        .into();
    let s = scene(anchors(vec![card]), 200.0, 120.0);
    let found = semantics(&s.laid);
    assert_eq!(found[0].label, "Chainshot: 120g");
    let tile = ui.theme().controls.tile;
    assert!(
        found[0].rect.width() >= tile.0 && found[0].rect.height() >= tile.1,
        "a card is at least the tile role: {:?}",
        found[0].rect
    );
}
