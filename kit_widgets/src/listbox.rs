// Ported from egui 0.36.2 `crates/egui/src/widgets/button.rs` (MIT OR Apache-2.0, (c) Emil Ernerfeldt, Rerun Technologies).
// Changes: kit state, intents and theme tokens replace egui's Ui/Response/Memory/epaint.
//! Controlled keyed selection. The host keeps a [`Selection`] and applies what `choose` sends with
//! [`Selection::input`] against its CURRENT items. A click picks (Shift extends, Cmd/Ctrl toggles, both add a range);
//! while a row has focus the arrows, Home/End and typed letters move the cursor, focus follows it (a focus claim) and
//! Enter/Space pick the focused row. The viewport is the scroll core, revealing the cursor.
use crate::common::{ink, surface};
use kit_core::{Axis, Border, Key, Modifiers, NamedKey, Shadow, Theme, Time, WidgetId};
use kit_state::{Cx, Host, Ident, Intent};
use kit_view::{A11y, Style, Toggle, Weight, column, interactive, panel, px, scroll_area, text};
use kit_view::{Bindings, KeyAnswer, KeyPress, Node, RevealAlign, RevealTarget, Role, ScrollSpec};
use std::{collections::HashSet, hash::Hash, rc::Rc, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Single,
    Multiple,
}
/// Plain, Cmd/Ctrl, Shift and Cmd/Ctrl+Shift: read from the modifiers held (Ctrl counts as Cmd on every platform).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Replace,
    Toggle,
    Extend,
    AddRange,
}
impl Gesture {
    pub fn of(mods: Modifiers) -> Self {
        match (mods.ctrl || mods.meta, mods.shift) {
            (false, false) => Gesture::Replace,
            (true, false) => Gesture::Toggle,
            (false, true) => Gesture::Extend,
            (true, true) => Gesture::AddRange,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input<I> {
    Pick(I),
    Up,
    Down,
    Home,
    End,
    Text(String),
}

/// `anchor` and `cursor` are absent until something is chosen; `revision` counts moves (reveal and focus tickets).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection<I> {
    selected: Vec<I>,
    anchor: Option<I>,
    cursor: Option<I>,
    query: String,
    typed: Time,
    revision: u64,
}
impl<I: Clone + Eq> Default for Selection<I> {
    fn default() -> Self {
        let (selected, query, typed) = (Vec::new(), String::new(), Time::default());
        let (anchor, cursor, revision) = (None, None, 0);
        Self {
            selected,
            anchor,
            cursor,
            query,
            typed,
            revision,
        }
    }
}
// `Hash` is for the duplicate-key check (O(n), was O(n^2)); `listbox` already required it, so no usable caller changes.
impl<I: Clone + Eq + Hash> Selection<I> {
    pub fn selected(&self) -> &[I] {
        &self.selected
    }
    /// Navigation clamps at the ends and does nothing on an empty list; a removed anchor restarts at the target.
    /// Type-ahead extends its prefix for one second. Panics on duplicate item keys or a Pick of an absent key.
    pub fn input(
        &mut self,
        items: &[(I, &str)],
        mode: Mode,
        gesture: Gesture,
        input: Input<I>,
        now: Time,
    ) {
        unique(items);
        let position = |key: &I| items.iter().position(|(id, _)| id == key);
        self.selected.retain(|k| position(k).is_some());
        let current = self.cursor.as_ref().and_then(position);
        // `None`: the list is empty, there is nowhere to move.
        let last = items.len().checked_sub(1);
        match &input {
            Input::Text(_) => {}
            Input::Pick(_) | Input::Up | Input::Down | Input::Home | Input::End => {
                self.query.clear()
            }
        }
        let target = match input {
            Input::Pick(key) => Some(position(&key).expect("listbox: Pick key is absent")),
            Input::Up => last.map(|last| current.map_or(last, |i| i.saturating_sub(1))),
            Input::Down => last.map(|last| current.map_or(0, |i| (i + 1).min(last))),
            Input::Home => last.map(|_| 0),
            Input::End => last,
            Input::Text(text) => self.seek(items, current, &text.to_lowercase(), now),
        };
        let Some(target) = target else { return };
        let key = items[target].0.clone();
        let gesture = match mode {
            Mode::Single => Gesture::Replace,
            Mode::Multiple => gesture,
        };
        match gesture {
            Gesture::Replace => {
                self.selected = vec![key.clone()];
                self.anchor = Some(key.clone());
            }
            Gesture::Toggle => {
                match self.selected.iter().position(|id| id == &key) {
                    Some(i) => drop(self.selected.remove(i)),
                    None => self.selected.push(key.clone()),
                }
                self.anchor = Some(key.clone());
            }
            Gesture::Extend | Gesture::AddRange => {
                // A removed anchor restarts the range at the target.
                let anchor = self.anchor.as_ref().and_then(position).unwrap_or(target);
                self.anchor = Some(items[anchor].0.clone());
                if gesture == Gesture::Extend {
                    self.selected.clear();
                }
                for (id, _) in &items[anchor.min(target)..=anchor.max(target)] {
                    if !self.selected.contains(id) {
                        self.selected.push(id.clone());
                    }
                }
            }
        }
        self.cursor = Some(key);
        self.revision += 1;
    }
    /// Type-ahead: the next row whose name starts with the prefix typed so far (a repeated letter cycles).
    fn seek(
        &mut self,
        items: &[(I, &str)],
        current: Option<usize>,
        typed: &str,
        now: Time,
    ) -> Option<usize> {
        if typed.is_empty() {
            return None;
        }
        if now.since(self.typed) > Duration::from_secs(1) || self.query == typed {
            self.query.clear();
        }
        let continuing = !self.query.is_empty();
        self.query.push_str(typed);
        self.typed = now;
        let start = current.map_or(0, |i| if continuing { i } else { (i + 1) % items.len() });
        let found = (0..items.len()).map(|n| (start + n) % items.len());
        found
            .into_iter()
            .find(|&i| items[i].1.to_lowercase().starts_with(&self.query))
    }
}
/// One pass with a hash set. It ran every build as a pairwise scan: 10,000 rows cost 50 million comparisons a frame.
fn unique<I: Eq + Hash>(items: &[(I, &str)]) {
    let mut seen = HashSet::with_capacity(items.len());
    let ok = items.iter().all(|(key, _)| seen.insert(key));
    assert!(ok, "listbox: duplicate item keys");
}

pub struct Listbox<'a, Msg, K, I> {
    pub theme: &'a Theme,
    pub key: K,
    pub label: &'a str,
    pub items: &'a [(I, &'a str)],
    pub selection: &'a Selection<I>,
    /// Apply this input with this gesture to the caller's CURRENT ordered items, in its reducer.
    pub choose: Rc<dyn Fn(Input<I>, Gesture) -> Msg>,
    /// Panics unless finite and positive.
    pub height: f64,
}
pub fn listbox<A: Host, K: Ident<A>, I: Clone + Eq + Hash + 'static>(
    cx: &Cx<'_, A>,
    p: Listbox<'_, Intent<A>, K, I>,
) -> Node<Intent<A>> {
    assert!(
        p.height.is_finite() && p.height > 0.0,
        "listbox: height must be finite and positive"
    );
    unique(p.items);
    let (th, c, s) = (p.theme, &p.theme.colors, p.selection);
    let within = p.items.iter().any(|(id, _)| cx.focused(&(&p.key, id)));
    let rows = p.items.iter().map(|(id, label)| {
        let (key, on, choose) = ((&p.key, id), s.selected.contains(id), p.choose.clone());
        let fill = match (on, cx.hovered(&key)) {
            (true, _) => c.accent,
            (false, true) => c.surface_raised,
            (false, false) => c.field_fill,
        };
        let (style, name) = (Style::row().padded_xy(th.spacing.md, th.spacing.sm), *label);
        let name = text(name, ink(th.type_scale.label, c.text, Weight::NORMAL));
        let face = panel(
            style,
            surface(fill, th.radii.sm, Border::None, Shadow::None),
            vec![name],
        );
        let toggle = if on { Toggle::On } else { Toggle::Off };
        let (pick, keys) = (id.clone(), p.choose.clone());
        let pick = move |m| choose(Input::Pick(pick.clone()), Gesture::of(m));
        let bound = Bindings::activate_with(pick).on_key(move |k: KeyPress| {
            let input = match k.key {
                Key::Named(NamedKey::ArrowUp) => Input::Up,
                Key::Named(NamedKey::ArrowDown) => Input::Down,
                Key::Named(NamedKey::Home) => Input::Home,
                Key::Named(NamedKey::End) => Input::End,
                Key::Character(ch) => Input::Text(ch.to_string()),
                Key::Named(_) => return KeyAnswer::Pass,
            };
            KeyAnswer::Handled(keys(input, Gesture::of(k.mods)))
        });
        let a11y = A11y::toggled(Role::Option, *label, toggle);
        let row = interactive(WidgetId::of(&key), bound, a11y, face);
        // Keyboard focus follows the cursor, once per move, only while the list already has focus.
        match (within, s.cursor.as_ref() == Some(id)) {
            (true, true) => row.claim_focus(&s.revision),
            (false, _) | (_, false) => row,
        }
    });
    let spec = match &s.cursor {
        Some(id) if p.items.iter().any(|(key, _)| key == id) => {
            let ids: Vec<&I> = p.items.iter().map(|(id, _)| id).collect();
            let target = RevealTarget::Child(WidgetId::of(&(&p.key, id)));
            ScrollSpec::new(Axis::Vertical).reveal(target, RevealAlign::Nearest, &(s.revision, ids))
        }
        Some(_) | None => ScrollSpec::new(Axis::Vertical),
    };
    let mut viewport = Style::column();
    viewport.layout.size.height = px(p.height);
    let _ = cx.scroll(&p.key);
    let caption = text(
        p.label,
        ink(th.type_scale.label, c.text_muted, Weight::NORMAL),
    );
    let list = scroll_area(WidgetId::of(&p.key), spec, viewport, column(rows));
    column([caption, list]).gap(th.spacing.xs)
}
