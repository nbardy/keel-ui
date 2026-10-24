// Ported from egui 0.36.2 `crates/egui/src/widgets/button.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Caller-owned tab selection, fading indicators and optional close intents. Panics for an empty bar or an
//! out-of-range selection. Visible navigation buttons supply focused Left/Right wrap using the kit's shortcuts.
use crate::common::{ink, solid};
use kit_core::{Key, NamedKey, Theme, WidgetId};
use kit_state::{Cx, Host, Ident, Intent, Life};
use kit_view::{
    A11y, Bindings, Node, Role, Style, Toggle, Weight, column, interactive, layer, rect, row, text,
};

pub struct Tabs<'a, Msg, K> {
    pub theme: &'a Theme,
    pub key: K,
    pub labels: &'a [&'a str],
    pub selected: usize,
    pub choose: &'a dyn Fn(usize) -> Msg,
    /// `None` means fixed tabs; closing never selects a replacement or removes a tab itself.
    pub close: Option<&'a dyn Fn(usize) -> Msg>,
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
struct Item<K>(K, &'static str, usize);
impl<A, K: Ident<A>> Ident<A> for Item<K> {
    fn life(&self, app: &A) -> Life {
        self.0.life(app)
    }
}
pub fn tabs<A: Host, K: Ident<A>>(cx: &Cx<'_, A>, p: Tabs<'_, Intent<A>, K>) -> Node<Intent<A>> {
    assert!(p.selected < p.labels.len(), "tabs: invalid selection");
    let (th, n) = (p.theme, p.labels.len());
    let focused = [NamedKey::ArrowLeft, NamedKey::ArrowRight]
        .iter()
        .any(|part| cx.focused(&(&p.key, "navigation", part)))
        || (0..n).any(|i| {
            cx.focused(&Item(p.key.clone(), "tab", i)) || cx.focused(&(&p.key, i, "close"))
        });
    let control = |id, label: String, on, toggle, face| {
        let name = A11y::Labeled {
            role: Role::Button,
            label,
            toggle,
        };
        interactive(id, on, name, face)
    };
    let style = ink(th.type_scale.label, th.colors.text, Weight::NORMAL);
    let caption = |label: &str| text(label, style.clone()).pad(th.spacing.md);
    let navigation = |key, target| {
        let (label, glyph) = if key == NamedKey::ArrowLeft {
            ("Previous tab", "‹")
        } else {
            ("Next tab", "›")
        };
        let on = Bindings::activate((p.choose)(target));
        let on = if focused {
            on.with_shortcut(Key::Named(key))
        } else {
            on
        };
        control(
            WidgetId::of(&(&p.key, "navigation", &key)),
            label.to_owned(),
            on,
            Toggle::NotToggle,
            caption(glyph),
        )
    };
    let previous = (p.selected + n - 1) % n;
    let next = (p.selected + 1) % n;
    let mut children = vec![navigation(NamedKey::ArrowLeft, previous)];
    for (i, label) in p.labels.iter().enumerate() {
        let key = Item(p.key.clone(), "tab", i);
        let chosen = i == p.selected;
        let toggle = if chosen { Toggle::On } else { Toggle::Off };
        let pick = control(
            WidgetId::of(&key),
            (*label).to_owned(),
            Bindings::activate((p.choose)(i)),
            toggle,
            caption(label),
        );
        let close = match p.close {
            None => kit_view::nothing(),
            Some(close) => control(
                WidgetId::of(&(&p.key, i, "close")),
                format!("Close {label}"),
                Bindings::activate(close(i)),
                Toggle::NotToggle,
                caption("×"),
            ),
        };
        let mut line = Style::row();
        line.layout.size.height = kit_view::px(th.spacing.xs);
        let indicator = rect(line, solid(th.colors.accent)).fill_width();
        let opacity = cx.animate_bool(&key, chosen, th.motion.fast());
        children.push(column([row([pick, close]), layer(opacity, indicator)]));
    }
    children.push(navigation(NamedKey::ArrowRight, next));
    row(children).gap(th.spacing.xs)
}
