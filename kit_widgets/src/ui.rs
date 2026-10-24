//! `Ui<'t>`: the theme, held once per screen build, with the widget constructors hung off it.
//!
//! `let ui = Ui::new(&theme);` then `ui.button(icon, "Name").on_press(msg)`, `ui.caption("Hull")`, `ui.panel(..)`.
//! The theme is an explicit value the caller owns (no global, no thread-local); a `Ui` is `Copy` and cheap to pass.
use crate::bar::{Gauge, meter};
use crate::button::{Button, ButtonSize};
use crate::common::{ink, surface};
use crate::icon::Icon;
use crate::panel::{inset, panel};
use kit_core::peniko::Color;
use kit_core::{Border, Identity, Shadow, Theme};
use kit_view::RichText;
use kit_view::{
    A11y, Node, Percent, Role, Style, TextAlign, TextWrap, Toggle, Weight, disabled,
    panel as panel_node, rich_text,
};

#[derive(Copy, Clone)]
pub struct Ui<'t> {
    theme: &'t Theme,
}

impl<'t> Ui<'t> {
    pub fn new(theme: &'t Theme) -> Self {
        Self { theme }
    }
    pub fn theme(&self) -> &'t Theme {
        self.theme
    }
    /// An icon control; `label` is its accessible name (screen readers, tests), not drawn.
    pub fn button<Msg>(&self, icon: Icon, label: impl Into<String>) -> Button<'t, Msg> {
        Button::icon(self.theme, icon, label)
    }
    /// Icon and words side by side; `label` is drawn and is also the accessible name.
    pub fn labeled_button<Msg>(&self, icon: Icon, label: impl Into<String>) -> Button<'t, Msg> {
        Button::icon_text(self.theme, icon, label)
    }
    /// Icon over a name and a secondary line (`"Cannon"`, `"40g"`). The accessible name is `"name: detail"`.
    pub fn card<Msg>(
        &self,
        icon: Icon,
        name: impl Into<String>,
        detail: impl Into<String>,
    ) -> Button<'t, Msg> {
        Button::card(self.theme, icon, name, detail)
    }
    /// Small supporting text (strip captions, hotkeys).
    pub fn caption(&self, content: impl Into<String>) -> Text<'t> {
        Text::new(self.theme, content)
    }
    /// Raised surface around `content`, padded by the theme's medium spacing.
    pub fn panel<Msg>(&self, content: impl Into<Node<Msg>>) -> Node<Msg> {
        panel(
            self.theme,
            Style::column().padded(self.theme.spacing.md),
            vec![content.into()],
        )
    }
    /// Recessed well around `content`.
    pub fn well<Msg>(&self, content: impl Into<Node<Msg>>) -> Node<Msg> {
        inset(
            self.theme,
            Style::column().padded(self.theme.spacing.sm),
            vec![content.into()],
        )
    }
    /// A whole screen's floor: the darkest surface over the full viewport, with `content` (usually a `Screen`) filling it.
    /// The world behind is hidden, which is what a lobby or a result screen wants and an in-match overlay does not.
    pub fn backdrop<Msg>(&self, content: impl Into<Node<Msg>>) -> Node<Msg> {
        let mut style = Style::column();
        style.layout.size = kit_view::taffy::Size {
            width: Percent(100.0).into(),
            height: Percent(100.0).into(),
        };
        let floor = surface(
            self.theme.colors.surface_inset,
            0.0,
            Border::None,
            Shadow::None,
        );
        panel_node(style, floor, vec![content.into().grow(1.0)])
    }
    /// An empty box the size of a button role: keeps a row of controls aligned where one is absent.
    pub fn space<Msg>(&self, size: ButtonSize) -> Node<Msg> {
        let size = size.px(self.theme);
        kit_view::nothing().size(size.width, size.height)
    }
    /// Horizontal gauge `height` px tall that fills the width its parent gives it.
    pub fn gauge<Msg>(&self, gauge: Gauge, color: Color, height: f64) -> Node<Msg> {
        meter(self.theme, height, gauge, color)
    }
}

/// Type size of a [`Text`]: the theme's scale, by name.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Scale {
    Caption,
    Body,
    Title,
}
/// What a [`Text`] says about its content: ink colour from the theme's palette, never an RGB at the call site.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Tone {
    Normal,
    Muted,
    Alert,
    Friend,
    Foe,
}

/// Whether a screen reader (and the test mirror) can read a [`Text`].
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
enum Reach {
    /// Drawn only: decoration and long prose.
    Drawn,
    /// Also announced as a label under its own words: a status line, a refusal, a roster entry.
    Announced,
}

/// Themed text: `ui.caption("Hull").align(TextAlign::Center)`, `ui.caption("Teams").title()`.
pub struct Text<'t> {
    theme: &'t Theme,
    content: String,
    /// cosmic-text's alignment: `None` is the start of the paragraph's direction.
    align: Option<TextAlign>,
    scale: Scale,
    tone: Tone,
    reach: Reach,
}

impl<'t> Text<'t> {
    fn new(theme: &'t Theme, content: impl Into<String>) -> Self {
        Self {
            theme,
            content: content.into(),
            align: None,
            scale: Scale::Caption,
            tone: Tone::Normal,
            reach: Reach::Drawn,
        }
    }
    /// Make the words readable by assistive technology and by tests (`Role::Label`, inert to presses). A status,
    /// a refusal or a roster entry is information the player needs; a bare text node is invisible to both.
    pub fn announced(mut self) -> Self {
        self.reach = Reach::Announced;
        self
    }
    /// A screen or section heading (the theme's title size, bold).
    pub fn title(mut self) -> Self {
        self.scale = Scale::Title;
        self
    }
    /// Reading size (the theme's body size).
    pub fn body(mut self) -> Self {
        self.scale = Scale::Body;
        self
    }
    /// Secondary information.
    pub fn muted(mut self) -> Self {
        self.tone = Tone::Muted;
        self
    }
    /// A refusal or a fault the player must see.
    pub fn alert(mut self) -> Self {
        self.tone = Tone::Alert;
        self
    }
    /// The viewer's own side (`Theme::colors.friend`).
    pub fn friend(mut self) -> Self {
        self.tone = Tone::Friend;
        self
    }
    /// The opposing side.
    pub fn foe(mut self) -> Self {
        self.tone = Tone::Foe;
        self
    }
    /// Alignment within the box the layout gives the text: centre a caption under a tile by putting it in a column
    /// that stretches to the tile's width, not by wrapping it in anchors.
    pub fn align(mut self, align: TextAlign) -> Self {
        self.align = Some(align);
        self
    }
}

impl<Msg> From<Text<'_>> for Node<Msg> {
    fn from(text: Text<'_>) -> Self {
        let Text {
            theme,
            content,
            align,
            scale,
            tone,
            reach,
        } = text;
        let (size, weight) = match scale {
            Scale::Caption => (theme.type_scale.caption, Weight::NORMAL),
            Scale::Body => (theme.type_scale.body, Weight::NORMAL),
            Scale::Title => (theme.type_scale.title, Weight::BOLD),
        };
        let color = match tone {
            Tone::Normal => theme.colors.text,
            Tone::Muted => theme.colors.text_muted,
            Tone::Alert => theme.colors.danger,
            Tone::Friend => theme.colors.friend,
            Tone::Foe => theme.colors.foe,
        };
        let mut style = ink(size, color, weight);
        style.align = align;
        // Alignment only has meaning inside a box wider than the text, so the text is laid out to its box's width.
        style.wrap = match align {
            None => TextWrap::None,
            Some(
                TextAlign::Left
                | TextAlign::Right
                | TextAlign::Center
                | TextAlign::Justified
                | TextAlign::End,
            ) => TextWrap::WordOrGlyph,
        };
        let node = rich_text(RichText::plain(content.clone()), style);
        match reach {
            Reach::Drawn => node,
            Reach::Announced => disabled(
                Identity::Positional,
                A11y::Labeled {
                    role: Role::Label,
                    label: content,
                    toggle: Toggle::NotToggle,
                },
                node,
            ),
        }
    }
}
