//! `Button`: the builder every control goes through (icon, labeled, card), and the one `ButtonState` dispatcher.
use crate::common::{anchors_box, ink, pad, pinned, surface, tabular};
use crate::icon::Icon;
use kit_core::kurbo::{Size, Vec2};
use kit_core::peniko::Color;
use kit_core::{Border, Identity, Shadow, Theme, WidgetId};
use kit_view::{
    A11y, AnchorH, AnchorV, Bindings, DragPayload, Dropped, Node, Role, Shortcut, Style, Toggle,
    Tooltip, Weight, panel as surface_node, px, taffy as tf, text,
};
use std::hash::Hash;

/// What a press does, and how the button looks. Hover/pressed overlays are painted from `UiMemory`, not from here.
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum ButtonState<Msg> {
    Enabled(Msg),
    /// Highlighted (the active tool/stance); still activates.
    Selected(Msg),
    /// Highlighted and waiting for a key: it has no activation of its own (so Enter and Space reach its key
    /// binding like any other key) and reads the next press through the `on_key` the builder carried in `gestures`.
    Listening,
    /// Visible but inert: no binding, transparent to hits.
    Disabled,
    /// Drawn like an enabled control but carrying no message: it swallows the press (a click on an information
    /// tile must not fall through to the world under it) and shows a pressed/hover-free surface.
    Inert,
    /// Information, not a control: dimmed, transparent to presses, and announced as a label rather than as a
    /// disabled button (a gold counter, an empty inventory slot).
    Display,
}

#[derive(Copy, Clone)]
struct Look {
    background: Color,
    ink: Color,
}

/// Everything a control carries besides its look: who it is, what it says, what else it takes part in.
struct Parts<Msg> {
    identity: Identity,
    label: String,
    tooltip: Tooltip,
    /// Whether the control is a toggle and which way it is set (`Selected` states are always `On`).
    toggle: Toggle,
    /// Drag/drop and the shortcut only (`activate` is filled in from the state). A `Disabled`, `Inert` or `Display`
    /// control drops these: a disabled slot is not a drop target and its key does nothing.
    gestures: Bindings<Msg>,
}

impl<Msg> ButtonState<Msg> {
    /// The one dispatcher: each variant picks its look and its wrapper, and nothing downstream asks again.
    fn present(
        self,
        theme: &Theme,
        control: Parts<Msg>,
        body: impl FnOnce(Look) -> Node<Msg>,
    ) -> Node<Msg> {
        let c = &theme.colors;
        let Parts {
            identity,
            label,
            tooltip,
            toggle,
            gestures,
        } = control;
        let a11y = A11y::Labeled {
            role: match self {
                ButtonState::Display => Role::Label,
                ButtonState::Enabled(_)
                | ButtonState::Selected(_)
                | ButtonState::Listening
                | ButtonState::Disabled
                | ButtonState::Inert => Role::Button,
            },
            label,
            toggle: match self {
                ButtonState::Selected(_) | ButtonState::Listening => Toggle::On,
                ButtonState::Enabled(_)
                | ButtonState::Disabled
                | ButtonState::Inert
                | ButtonState::Display => toggle,
            },
        };
        let wrap = |on: Bindings<Msg>, availability, child| Node::Interactive {
            identity,
            on,
            a11y,
            tooltip,
            availability,
            child: Box::new(child),
        };
        let raised = |ink| Look {
            background: c.surface_raised,
            ink,
        };
        match self {
            ButtonState::Enabled(msg) => wrap(
                Bindings {
                    activate: Some(msg),
                    ..gestures
                },
                kit_view::Availability::Enabled,
                body(raised(c.text)),
            ),
            ButtonState::Selected(msg) => wrap(
                Bindings {
                    activate: Some(msg),
                    ..gestures
                },
                kit_view::Availability::Enabled,
                body(Look {
                    background: c.accent,
                    ink: c.text,
                }),
            ),
            ButtonState::Listening => wrap(
                gestures,
                kit_view::Availability::Enabled,
                body(Look {
                    background: c.accent,
                    ink: c.text,
                }),
            ),
            ButtonState::Inert => wrap(
                Bindings::none(),
                kit_view::Availability::Enabled,
                body(raised(c.text)),
            ),
            ButtonState::Disabled | ButtonState::Display => wrap(
                Bindings::none(),
                kit_view::Availability::Disabled,
                body(raised(c.text_disabled)),
            ),
        }
    }
}

/// Keyboard shortcut shown in the button corner.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub(crate) enum Hotkey<'a> {
    NoHint,
    Key(&'a str),
}

impl Hotkey<'_> {
    fn nodes<Msg>(self, theme: &Theme) -> Vec<Node<Msg>> {
        match self {
            Hotkey::NoHint => Vec::new(),
            Hotkey::Key(key) => vec![pinned(
                AnchorH::End,
                AnchorV::End,
                Vec2::new(-3.0, -1.0),
                text(
                    key,
                    ink(
                        theme.type_scale.caption,
                        theme.colors.text_muted,
                        Weight::BOLD,
                    ),
                ),
            )],
        }
    }
}

/// Icon fraction of the shorter side (`keel_ui`'s 12% padding).
const TILE_GLYPH: f64 = 0.76;

fn button_body<Msg>(
    theme: &Theme,
    face: (Size, f64),
    icon: &Icon,
    hotkey: Hotkey<'_>,
    look: Look,
    overlays: Vec<Node<Msg>>,
) -> Node<Msg> {
    let (size, glyph_fraction) = face;
    let glyph = size.width.min(size.height) * glyph_fraction;
    let icon = icon.node(
        Style::column()
            .sized(glyph, glyph)
            .anchored(AnchorH::Center, AnchorV::Center, Vec2::ZERO),
        look.ink,
    );
    let border = Border::Solid {
        width: 1.0,
        color: theme.colors.field_border,
    };
    let mut children = vec![icon];
    children.extend(overlays);
    children.extend(hotkey.nodes(theme));
    Node::Panel {
        style: anchors_box(size.width, size.height),
        surface: surface(look.background, theme.radii.control, border, Shadow::None),
        children,
    }
}

/// Count chip in a tile's corner (talent choices, gold).
#[derive(Clone, Eq, PartialEq, Debug)]
pub(crate) enum Badge {
    NoBadge,
    Text(String),
}
impl Badge {
    fn nodes<Msg>(self, theme: &Theme) -> Vec<Node<Msg>> {
        match self {
            Badge::NoBadge => Vec::new(),
            Badge::Text(content) => vec![pinned(
                AnchorH::End,
                AnchorV::End,
                Vec2::new(-2.0, -2.0),
                badge(theme, &content),
            )],
        }
    }
}

/// Small chip for counts, gold, levels.
fn badge<Msg>(theme: &Theme, content: &str) -> Node<Msg> {
    surface_node(
        pad(Style::row(), theme.spacing.sm, theme.spacing.xs),
        surface(
            theme.colors.surface_raised,
            theme.radii.sm,
            Border::None,
            Shadow::None,
        ),
        vec![text(
            content,
            tabular(theme.type_scale.caption, theme.colors.text, Weight::BOLD),
        )],
    )
}

/// What pressing a button sends. `Nothing` is a display control (a stat tile, an inventory slot).
enum Press<Msg> {
    Nothing,
    Send(Msg),
    /// No press message: the button reads keys instead ([`Button::on_keys`]).
    Keys,
}

/// Size roles, named by what the control is for; the pixels live in `Theme::controls`, so every screen's tiles agree.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum ButtonSize {
    /// A corner chip on a tile (drop cargo).
    Mini,
    /// Toolbar, status and talent tiles; the height of every text-bearing button.
    Compact,
    /// The standard tile: plan steps, inventory, shop cards, weapon pickers.
    Tile,
    /// The one action a screen is about.
    Hero,
}

impl ButtonSize {
    pub(crate) fn px(self, theme: &Theme) -> Size {
        let sizes = &theme.controls;
        let (width, height) = match self {
            ButtonSize::Mini => sizes.mini,
            ButtonSize::Compact => sizes.compact,
            ButtonSize::Tile => sizes.tile,
            ButtonSize::Hero => sizes.hero,
        };
        Size::new(width, height)
    }
}

/// What a button shows. The accessible name is always the whole story; the face decides how much of it is drawn.
enum Face {
    /// The icon alone: tiles, toolbars.
    Icon(Icon),
    /// Icon and label side by side.
    IconText(Icon),
    /// Icon over a name and a secondary line (a shop item: name, price).
    Card {
        icon: Icon,
        name: String,
        detail: String,
    },
}

/// A button of any face and proportion: `ui.button(icon, "Name").size(ButtonSize::Tile).on_press(msg).selected(on)`.
///
/// The builder collects independent facts; [`Button::state`] is the single place they become a [`ButtonState`].
pub struct Button<'a, Msg> {
    theme: &'a Theme,
    face: Face,
    size: ButtonSize,
    press: Press<Msg>,
    toggle: Toggle,
    enabled: bool,
    display: bool,
    shortcut: Shortcut,
    badge: Badge,
    control: Parts<Msg>,
}

impl<'a, Msg> Button<'a, Msg> {
    /// Compact until `.size(..)` says otherwise; enabled, not a toggle, pressing sends nothing.
    fn new(theme: &'a Theme, face: Face, label: impl Into<String>) -> Self {
        Self {
            theme,
            face,
            size: ButtonSize::Compact,
            press: Press::Nothing,
            toggle: Toggle::NotToggle,
            enabled: true,
            display: false,
            shortcut: Shortcut::Unbound,
            badge: Badge::NoBadge,
            control: Parts {
                identity: Identity::Positional,
                label: label.into(),
                tooltip: Tooltip::None,
                toggle: Toggle::NotToggle,
                gestures: Bindings::none(),
            },
        }
    }
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }
    pub fn on_press(mut self, msg: Msg) -> Self {
        self.press = Press::Send(msg);
        self
    }
    /// A button that waits for a key: highlighted, with no activation, so every key (Enter and Space too) reaches
    /// `on` while it has focus. Used to capture a key for rebinding; replaces `on_press`. It claims focus once per
    /// distinct `listening` stamp (a claim is edge-triggered: a second capture on the same row needs a new stamp).
    pub fn on_keys(
        mut self,
        listening: &impl std::hash::Hash,
        on: impl Fn(kit_view::KeyPress) -> kit_view::KeyAnswer<Msg> + 'static,
    ) -> Self {
        self.press = Press::Keys;
        self.control.gestures =
            std::mem::replace(&mut self.control.gestures, Bindings::none()).on_key(on);
        // Listening starts with focus on it: the very next key is the one wanted.
        self.control.gestures =
            std::mem::replace(&mut self.control.gestures, Bindings::none()).claim_focus(listening);
        self
    }
    /// A toggle: highlighted when `on`, and announced as pressed or not pressed either way (only meaningful with
    /// `on_press`).
    pub fn selected(mut self, on: bool) -> Self {
        self.toggle = if on { Toggle::On } else { Toggle::Off };
        self
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    /// Information, not a control: dimmed, transparent to presses, announced as a label. Use for counters and empty
    /// slots, never for a command that is merely unavailable (that is `.enabled(false)`).
    pub fn info(mut self) -> Self {
        self.display = true;
        self
    }
    /// A key that presses the button (while it is enabled), drawn as a hint in its corner. `Unbound` is no key.
    pub fn hotkey(mut self, shortcut: Shortcut) -> Self {
        self.shortcut = shortcut;
        self
    }
    pub fn badge(mut self, text: impl Into<String>) -> Self {
        self.badge = Badge::Text(text.into());
        self
    }
    /// Hover help (distinct from the accessible name, which is the short label).
    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.control.tooltip = Tooltip::Text(text.into());
        self
    }
    /// Hover help that also names what it is about, for hosts that preview the subject (a weapon's range).
    pub fn tooltip_about(mut self, text: impl Into<String>, subject: DragPayload) -> Self {
        self.control.tooltip = Tooltip::About {
            text: text.into(),
            subject,
        };
        self
    }
    /// Explicit identity for controls in a list that reorders or changes length; the default is the tree position.
    pub fn key(mut self, key: &impl Hash) -> Self {
        self.control.identity = Identity::Keyed(WidgetId::of(key));
        self
    }
    /// The control can be picked up and dragged, carrying `payload`.
    pub fn drag_source(mut self, payload: DragPayload) -> Self {
        self.control.gestures = self.control.gestures.with_drag(payload);
        self
    }
    /// The control accepts drops of payloads of kind `accepts`.
    pub fn drop_target(mut self, accepts: u32, on_drop: impl Fn(Dropped) -> Msg + 'static) -> Self {
        self.control.gestures = self.control.gestures.with_drop(accepts, on_drop);
        self
    }

    /// The one place the builder's facts become a [`ButtonState`].
    fn state(press: Press<Msg>, enabled: bool, display: bool, selected: bool) -> ButtonState<Msg> {
        match (display, enabled, press, selected) {
            (true, _, _, _) => ButtonState::Display,
            (false, false, _, _) => ButtonState::Disabled,
            (false, true, Press::Nothing, _) => ButtonState::Inert,
            (false, true, Press::Keys, _) => ButtonState::Listening,
            (false, true, Press::Send(msg), false) => ButtonState::Enabled(msg),
            (false, true, Press::Send(msg), true) => ButtonState::Selected(msg),
        }
    }
}

impl<'a, Msg> Button<'a, Msg> {
    pub(crate) fn icon(theme: &'a Theme, icon: Icon, label: impl Into<String>) -> Self {
        Self::new(theme, Face::Icon(icon), label)
    }
    pub(crate) fn icon_text(theme: &'a Theme, icon: Icon, label: impl Into<String>) -> Self {
        Self::new(theme, Face::IconText(icon), label)
    }
    /// Tile-sized by default: the card is taller than a one-line button.
    pub(crate) fn card(
        theme: &'a Theme,
        icon: Icon,
        name: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        let (name, detail) = (name.into(), detail.into());
        let label = format!("{name}: {detail}");
        let mut button = Self::new(theme, Face::Card { icon, name, detail }, label);
        button.size = ButtonSize::Tile;
        button
    }
}

impl<Msg> From<Button<'_, Msg>> for Node<Msg> {
    fn from(button: Button<'_, Msg>) -> Self {
        let Button {
            theme,
            face,
            size,
            press,
            toggle,
            enabled,
            display,
            shortcut,
            badge,
            mut control,
        } = button;
        control.toggle = toggle;
        control.gestures.shortcut = shortcut;
        let hint = match shortcut {
            Shortcut::Unbound => String::new(),
            Shortcut::Key(key) => key.hint(),
        };
        let hotkey = match shortcut {
            Shortcut::Unbound => Hotkey::NoHint,
            Shortcut::Key(_) => Hotkey::Key(&hint),
        };
        let size = size.px(theme);
        let state = Button::state(press, enabled, display, toggle == Toggle::On);
        let overlays = badge.nodes(theme);
        let label = control.label.clone();
        state.present(theme, control, |look| match face {
            Face::Icon(icon) => {
                button_body(theme, (size, TILE_GLYPH), &icon, hotkey, look, overlays)
            }
            Face::IconText(icon) => {
                let glyph = size.height * ROW_GLYPH;
                let content = vec![
                    icon.node(Style::column().sized(glyph, glyph), look.ink),
                    words(theme, &label, look.ink),
                ];
                shell(theme, size, Style::row(), content, hotkey, look, overlays)
            }
            Face::Card { icon, name, detail } => {
                let glyph = size.height * CARD_GLYPH;
                let content = vec![
                    icon.node(Style::column().sized(glyph, glyph), look.ink),
                    words(theme, &name, look.ink),
                    note(theme, &detail, look),
                ];
                shell(
                    theme,
                    size,
                    Style::column(),
                    content,
                    hotkey,
                    look,
                    overlays,
                )
            }
        })
    }
}

/// Icon fraction of a text row's height, and of a card's.
const ROW_GLYPH: f64 = 0.5;
const CARD_GLYPH: f64 = 0.42;

fn words<Msg>(theme: &Theme, content: &str, color: Color) -> Node<Msg> {
    text(content, ink(theme.type_scale.label, color, Weight::NORMAL))
}

/// A card's secondary line: quieter than the name, unless the whole card is dimmed already.
fn note<Msg>(theme: &Theme, content: &str, look: Look) -> Node<Msg> {
    let color = if look.ink == theme.colors.text {
        theme.colors.text_muted
    } else {
        look.ink
    };
    text(
        content,
        tabular(theme.type_scale.caption, color, Weight::NORMAL),
    )
}

/// A text-bearing button: content flowed in `direction`, centred, at least `size`, growing with its words.
fn shell<Msg>(
    theme: &Theme,
    size: Size,
    direction: Style,
    content: Vec<Node<Msg>>,
    hotkey: Hotkey<'_>,
    look: Look,
    overlays: Vec<Node<Msg>>,
) -> Node<Msg> {
    let mut style = direction
        .gap(theme.spacing.sm)
        .align_items(tf::AlignItems::CENTER)
        .justify(tf::JustifyContent::CENTER)
        .padded_xy(theme.spacing.md, theme.spacing.sm);
    style.layout.min_size = tf::Size {
        width: px(size.width),
        height: px(size.height),
    };
    let border = Border::Solid {
        width: 1.0,
        color: theme.colors.field_border,
    };
    let mut children = content;
    children.extend(overlays);
    children.extend(hotkey.nodes(theme));
    Node::Panel {
        style,
        surface: surface(look.background, theme.radii.control, border, Shadow::None),
        children,
    }
}
