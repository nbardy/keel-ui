# Third-party code, fonts and dependencies

The kit's own code is released under the [WTFPL](LICENSE). The pieces below are other people's work and keep their own
licences; if you copy them, keep their notices.

## Code ported from egui

These files port behaviour and maths from [egui](https://github.com/emilk/egui) 0.36.2 (MIT OR Apache-2.0). Each says so
in its header, and those parts remain under egui's licence:

- `kit_input/src/scroll.rs`
- `kit_widgets/src/checkbox.rs`
- `kit_widgets/src/collapsing.rs`
- `kit_widgets/src/color_picker.rs`
- `kit_widgets/src/combo_box.rs`
- `kit_widgets/src/drag_value.rs`
- `kit_widgets/src/listbox.rs`
- `kit_widgets/src/menu.rs`
- `kit_widgets/src/modal.rs`
- `kit_widgets/src/progress.rs`
- `kit_widgets/src/radio.rs`
- `kit_widgets/src/search_field.rs`
- `kit_widgets/src/segmented.rs`
- `kit_widgets/src/separator.rs`
- `kit_widgets/src/slider.rs`
- `kit_widgets/src/stepper.rs`
- `kit_widgets/src/switch.rs`
- `kit_widgets/src/table.rs`
- `kit_widgets/src/tabs.rs`
- `kit_widgets/src/template.rs`
- `kit_widgets/src/text_field.rs`
- `kit_widgets/src/toast.rs`
- `kit_widgets/src/tooltip.rs`

egui's MIT licence:

```text
Copyright (c) 2018-2021 Emil Ernerfeldt <emil.ernerfeldt@gmail.com>

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the Software without restriction, including without limitation the
rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit
persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the
Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
```

## Font

`kit_text/fonts/` bundles [Inter](https://rsms.me/inter/) 4.1, (c) The Inter Project Authors, under the SIL Open Font
License 1.1 (`kit_text/fonts/OFL.txt`). The `subset/` files are a Latin subset of it (see `kit_text/fonts/README.md`).

## Direct dependencies

Every transitive dependency is under a permissive licence (MIT, Apache-2.0, BSD, Zlib, ISC, Unicode-3.0); run
`cargo metadata` for the full list.

| Crate | Version | Licence | Used by |
|---|---|---|---|
| cosmic-text | 0.19.0 | MIT OR Apache-2.0 | kit_text |
| etagere | 0.3.0 | MIT OR Apache-2.0 | kit_wgpu |
| kurbo | 0.13.1 | Apache-2.0 OR MIT | kit_core |
| lyon | 1.0.19 | MIT OR Apache-2.0 | kit_wgpu |
| peniko | 0.6.1 | Apache-2.0 OR MIT | kit_core |
| pollster | 0.4.0 | Apache-2.0 OR MIT | kit_wgpu (tests) |
| swash | 0.2.10 | Apache-2.0 OR MIT | kit_text |
| taffy | 0.14.0 | MIT | kit_view |
| tiny-skia | 0.12.0 | BSD-3-Clause | kit_raster (tests) |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | kit_state, kit_text |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | kit_wgpu |
