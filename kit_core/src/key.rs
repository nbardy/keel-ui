//! Logical keys: the vocabulary shared by input events (`kit_input`) and by key bindings on controls (`kit_view`).

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum NamedKey {
    Escape,
    Enter,
    Tab,
    Space,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
}

/// Modifier keys held with a key press or a pointer press (shift-click, cmd-click).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum Key {
    /// A printable key, case-folded to lower case so a binding does not care whether Shift or Caps Lock is down.
    Character(char),
    Named(NamedKey),
}

impl Key {
    /// The key a printable character is bound to (`'M'` and `'m'` are one key).
    pub fn character(c: char) -> Self {
        Key::Character(c.to_lowercase().next().unwrap_or(c))
    }

    /// Short text for a keycap hint drawn on a control.
    pub fn hint(self) -> String {
        match self {
            Key::Character(c) => c.to_uppercase().collect(),
            Key::Named(named) => match named {
                NamedKey::Escape => "Esc",
                NamedKey::Enter => "Enter",
                NamedKey::Tab => "Tab",
                NamedKey::Space => "Space",
                NamedKey::Backspace => "Bksp",
                NamedKey::Delete => "Del",
                NamedKey::ArrowLeft => "Left",
                NamedKey::ArrowRight => "Right",
                NamedKey::ArrowUp => "Up",
                NamedKey::ArrowDown => "Down",
                NamedKey::Home => "Home",
                NamedKey::End => "End",
            }
            .to_owned(),
        }
    }

    /// The name WAI-ARIA `aria-keyshortcuts` uses for this key.
    pub fn aria(self) -> String {
        match self {
            Key::Character(c) => c.to_uppercase().collect(),
            Key::Named(named) => match named {
                NamedKey::Escape => "Escape",
                NamedKey::Enter => "Enter",
                NamedKey::Tab => "Tab",
                NamedKey::Space => "Space",
                NamedKey::Backspace => "Backspace",
                NamedKey::Delete => "Delete",
                NamedKey::ArrowLeft => "ArrowLeft",
                NamedKey::ArrowRight => "ArrowRight",
                NamedKey::ArrowUp => "ArrowUp",
                NamedKey::ArrowDown => "ArrowDown",
                NamedKey::Home => "Home",
                NamedKey::End => "End",
            }
            .to_owned(),
        }
    }
}
