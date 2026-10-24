//! Seam: widgets are plain functions returning `kit_view::Node<Msg>`: pure, taking a `Theme` and explicit values.
//!
//! Rules every widget here follows:
//! - No clock. Animation enters as a value; `Theme::motion` decides whether it moves.
//! - No hidden state. Hover/pressed/drag visuals come from `UiMemory` at paint time; a widget only declares bindings.
//! - Kinds are types: `ButtonState`, `Gauge`... each has one handler, matched once.
//! - Layout is flex columns/rows plus anchors (HUD style); no grid placement.

mod bar;
mod button;
mod checkbox;
mod combo_box;
mod common;
mod icon;
mod panel;
mod scroll;
pub mod template;
// Component ports: one public module each (see the re-exports at the end).
pub mod collapsing;
pub mod color_picker;
pub mod drag_value;
pub mod listbox;
pub mod menu;
pub mod modal;
pub mod progress;
pub mod radio;
pub mod search_field;
pub mod segmented;
pub mod separator;
pub mod slider;
pub mod stepper;
pub mod switch;
pub mod table;
pub mod tabs;
mod text_field;
pub mod toast;
pub mod tooltip;
mod ui;

pub use bar::{Gauge, GaugeError};
pub use button::{Button, ButtonSize};
pub use checkbox::{Check, Checkbox, checkbox};
pub use combo_box::{ComboBox, combo_box};
pub use common::{Press, pressable};
pub use icon::{Icon, Ink, PathEl};
pub use panel::{inset, panel};
pub use scroll::{OVERSCAN, reveal_row, virtual_list, virtual_rows, visible_rows};
pub use text_field::{Submitted, TextArea, TextField, text_area, text_field};
pub use ui::{Text, Ui};

// Component ports: the function and its props at the root; supporting types stay under the module path
// (`kit_widgets::table::Sort`), because names like `Selection` or `Action` would collide here.
pub use collapsing::{Collapsing, collapsing};
pub use color_picker::{ColorPicker, color_picker};
pub use drag_value::{DragValue, drag_value};
pub use listbox::{Listbox, listbox};
pub use menu::{Menu, MenuEntry, MenuItem, menu};
pub use modal::{Modal, modal};
pub use progress::{ProgressBar, Spinner, progress, spinner};
pub use radio::{Radio, RadioOption, radio};
pub use search_field::{SearchField, search_field};
pub use segmented::{Segmented, segmented};
pub use separator::{Divider, Separator, separator};
pub use slider::{Slider, slider};
pub use stepper::{Stepper, stepper};
pub use switch::{Switch, switch};
pub use table::{Column, Table, table};
pub use tabs::{Tabs, tabs};
pub use toast::{Toast, ToastStack, toast};
pub use tooltip::{Tooltip, tooltip};
