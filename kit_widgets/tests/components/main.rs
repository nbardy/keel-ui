//! Component tests. One module per component: `mod <name>;` here (the coordinator adds the line) and the file
//! `tests/components/<name>.rs`. Goldens live in `tests/golden/`. Shared driver: `support::Handle`.
mod checkbox;
mod collapsing;
mod color_picker;
mod combo_box;
mod drag_value;
mod listbox;
mod menu;
mod modal;
mod progress;
mod radio;
mod readme;
mod search_field;
mod segmented;
mod separator;
mod slider;
mod stepper;
mod support;
mod switch;
mod table;
mod tabs;
mod template;
mod text_field;
mod toast;
mod tooltip;
