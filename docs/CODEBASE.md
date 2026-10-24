> Written while building the kit inside the Keel game (a private repository); paths outside this repo, and
> links to other notes, refer to that game.

# Keel UI: the design of the codebase

Date 2026-10-23 (crate map re-measured after the kit flip, the same day). Status: CURRENT. Index: [README.md](README.md).
API and laws: [KEEL_UI_DESIGN.md](KEEL_UI_DESIGN.md). "Audit" numbers come from
[the slimming audit](../KEEL_UI_KIT_SLIM_AUDIT_2026-10-23.md) (measured before that day's edits). "Flip" numbers are
measured the same way on the tree after the DOM UI was deleted: every line of `src/**/*.rs` (+ WGSL) minus inline
`#[cfg(test)]` items, `tests/` excluded, comments included.

**BUILT** = in the tree when read. **PLANNED** = designed, not in the tree.

## Images (2026-10-24)

`kit_view::image(Image, ImageStyle) -> Node<Msg>` is a leaf, measured intrinsically from its texel size by
`kit_layout`, then sized by taffy like text or a shape. `Fit::{Stretch, Contain}` computes the painted rect;
`kit_paint` emits `Draw::Image { image, rect, style }`. The style carries tint, nearest/linear filtering and
rounded corners. Layout bounds remain the whole node box when Contain letterboxes.

`ImagePixels::new(w, h, rgba)` validates straight-alpha sRGB RGBA8 and mints a process-unique key. Its clones
share bytes and identity. Each backend owns a registry: `register_pixels(&ImagePixels) -> Result<Image,
RenderError>` returns the same handle on CPU and GPU. `release(Image) -> Result<(), RenderError>` drops
resources and keeps a tombstone: released keys cannot be revived. Never-registered and released handles are
separate typed errors. The frame/render signatures are unchanged.

`WgpuUi::register_texture(&TextureView, ImageSize) -> Image` retains a game texture's bindings, while the game
may render new content before each UI pass. The caller supplies a filterable single-sample 2D view with
TEXTURE_BINDING usage and premultiplied samples in the target's blending space. External textures are a
distinct `ImageSource::Texture`, rejected by CPU with `ExternalTextureOnCpu`. View size is the selected mip's
size; normal wgpu validation applies to incompatible views/devices and feedback loops.

The GPU pixel registry converts straight RGBA once: encoded premultiplication for Unorm targets, linear
premultiplication stored as sRGB for Srgb targets. Textured-instance batches include image source/filter,
rounded coverage and tint; images count as drawing items for opacity grouping and use existing clip sections.
CPU patterns use premultiplied pixels and tiny-skia rounded paths. Shared image scenes pin a reviewed golden
and CPU/GPU parity; per-kind draw omission must break parity. GPU readback tests prove same-encoder external
render-pass updates and translucent pixels on both target encodings.

The tables and slimming notes below retain their dated measurements; their old proposed removal of images
is superseded by this implemented image path.

## 1. Crate map

All at this repository's root. No kit crate depends on the game's crates. Dependency direction: `kit_core` <- `kit_view` <-
`kit_input` / `kit_layout` / `kit_text` / `kit_state` <- `kit_paint` <- backends. Backends never depend on each other,
except the dev-only harness `kit_test` (for apps' and the kit's tests), which depends on `kit_raster` for goldens.
`kit_widgets` depends on `kit_test` only as a dev-dependency.

| Crate | Single job | Depends on | Audit | Flip | Target | Status |
|---|---|---|---|---|---|---|
| `kit_core` | Ids, `Identity`, `Time`, `Epoch`, `Viewport`, `Theme` tokens, re-export `kurbo`/`peniko` | kurbo, peniko | 576 | 552 | ~545 | BUILT; cut dead theme tokens, merge key hint tables |
| `kit_view` | `Node<Msg>`, builders, `Screen`, `Style`, `LaidOut`, semantics extraction, test helpers | kit_core | 2,278 | 2,685 | ~1,315 | BUILT; text/style mirror types -> cosmic/taffy; `LayoutEngine`/`Element` traits go |
| `kit_input` | `InputEvent`, `Router`, `UiMemory` (hover/press/drag/focus/scroll), gestures, hotkeys | kit_core, kit_view | 1,119 | 3,006 | ~965 | BUILT; own caret editor -> `TextValue`/cosmic `Editor`; gamepad events unused |
| `kit_state` | `Cx`, `state`/`derive`, `Ident`/`Life`, `Intent`/`dispatch`, `Store`, `Act`, `Disclosure`/`Presence`/`Easing` | kit_core (kit_view dev only) | new | 1,399 | ~700 | BUILT: state, derive, life, dispatch, presence. PLANNED: `read`, `track`, ledger, preview helpers |
| `kit_layout` | taffy layout of a `Node` tree into `Placed` rects; `place_anchored`; `check_cells` | kit_core, kit_view, taffy | 1,224 | 1,021 | ~710 | BUILT; `AnchorLayout` deleted (verify), `Style` -> `taffy::Style` PLANNED |
| `kit_text` | cosmic-text shaping, measurement, glyph rasterisation, font sources | kit_core, kit_view, cosmic-text, swash | 844 | 1,680 | ~400 | BUILT; unchanged so far. Cut `LayoutKey` u8 encoding, mirror `Cursor`/`GlyphRun`/`RasterKey`, dead `hit()` |
| `kit_paint` | `Node` + layout -> `DrawList` (`Fill`, `Stroke`, `RoundRect`, `Glyphs`, `Caret`, `Image`, `PushLayer`/`PopLayer`) | kit_core, kit_view, kit_input | 597 | 574 | ~375 | BUILT; image and caret draws are live; no Custom/nine-slice/renderer trait |
| `kit_wgpu` | Draw a `DrawList` into the host's encoder (SDF quads, lyon paths, glyph atlas) | kit_core, kit_view, kit_paint, lyon, etagere, wgpu | 3,216 + 244 WGSL | 2,365 | ~1,830 + 150 | BUILT; glyph atlas and image registries; no gradient/custom/stencil/blend paths |
| `kit_raster` | tiny-skia reference renderer, `golden` harness, parity scenes. **Dev only** | kit_core, kit_view, kit_paint, kit_input, tiny-skia | 2,111 | 1,514 | ~1,500 (~915 of it test infra) | BUILT |
| `kit_test` | Test driver for components and apps: `Handle` (real router, real layout and fonts, controls found by role and label, `tick`, CPU goldens). **Dev only**. Moved out of `kit_widgets/tests/components/support.rs` unchanged, plus `with_golden_dir` | kit_core, kit_view, kit_state, kit_input, kit_layout, kit_paint, kit_raster | new | n/a | n/a | BUILT |
| `kit_widgets` | The widgets the game uses: `Ui`, `button`, `card`, `caption`, `panel`, `well`, `space`, `gauge`, `Icon` | kit_core, kit_view, kit_layout, kit_paint, kit_input (dev: kit_test) | 1,963 | 3,953 | ~615 | BUILT; dead widgets (ability/cooldown, slots, tabs/list, tooltip, floats, nine-slice, team) already removed in tree |
| `kit_anim` | (was tweens/springs) | - | 225 | gone | 0 (~20 easing -> `kit_state`) | Directory removed; `Easing` now in `kit_state` |

Totals (audit): 14,153 -> ~8,275 including dev-only `kit_raster`; **shipped** 12,042 -> ~6,775; plus microlib ~700 =
**~7,500 shipped UI stack**. Totals (flip, measured): 18,749 including `kit_raster`; **shipped 17,235**. The kit grew
instead of shrinking: text editing (`kit_input` editor, IME, clipboard), scroll physics, focus, portals, timed
wake-ups and the component set (`kit_widgets`: text field, combo box, list box, table, tabs, modal, menu, stepper,
slider, colour picker, ...) landed to carry every screen; the slimming steps below were not resumed. Tests ~3.3k -> ~2k lines. The audit's conclusion on a ~5k target: not reachable with this
feature set without replacing our renderer with a library one (vello or egui's painter), which would lose in-encoder
drawing and the parity tests. We take 7.5k.

Consumers (the only real ones; `wc -l`, flip): `keel/games/game_client/src/hud.rs` (1,411), `screens/**` (3,501, every
page), `tray.rs` (211), `icons.rs` (395); `keel/hosts/host_web/src/kit_ui.rs` (536), `kit_mirror.rs` (341),
`text_bridge.rs` (511); `keel/tools/keel_preview/src/hud_ui.rs` (999). host_web has no other UI: its whole `src/` is
3,795 lines (7,355 before the flip).

## 2. What each seam is, after slimming

| Seam | Type | Owner crate | Note |
|---|---|---|---|
| View | `Node<Msg>` (plain data, rebuilt per frame, `Node::map`) | kit_view | `Msg = Intent<App>` for screens built on kit_state |
| Layout | `TaffyLayout::layout(Node, Viewport, text, projector) -> LaidOut` | kit_layout | Concrete, not a trait (PLANNED; the `LayoutEngine` generic still exists at `hud.rs` and `keel_preview/hud_ui.rs`) |
| Text | `CosmicText` (`buffer(id) -> &Buffer`, `rasterize`) | kit_text | Trait removal PLANNED; `NullText` goes, tests load the bundled Inter subset |
| Paint | `paint(&LaidOut, &UiMemory, &Theme, Time) -> DrawList` | kit_paint | Typed draw vocabulary includes images and carets |
| Render | `WgpuUi::begin_frame(&mut text, viewport) -> UiFrame`; `RasterUi` for CPU | kit_wgpu / kit_raster | `UiFrame` drop is the atlas fence (`frame_submitted()`) |
| Input | `Router::route(&LaidOut, &mut UiMemory, InputEvent, Time) -> Routing<Msg>` | kit_input | `Handled` / `Captured` / `Passed` |
| State | `Cx`, `Store`, `dispatch` | kit_state | Renderer-agnostic; never names `Node` |

Percent units are 0..=100 in the kit and 0..=1 in taffy; any unconverted builder call is silently 100x wrong. One
constructor converts; a test pins a percent-width child. Grid lines are `GridIndex` (1..=32767); `Placement::Cell`
outside a grid is `LayoutError::CellOutsideGrid`.

## 3. Frame data flow

```
                         INPUT                                                     OUTPUT
 host events (winit / DOM pointer+key)                                      game's wgpu encoder (shared submit)
        |                                                                          ^
        v                                                                          | WgpuUi::begin_frame / UiFrame
  InputEvent --> Router::route(&LaidOut from LAST frame, &mut UiMemory) --> Routing  |
        |                                  Handled(intents) | Captured | Passed     |
        |                                         |                                  |
        |                                         v                                  |
        |                       kit_state::dispatch(app, store, intent)              |
        |                         | Step: reducer(&mut S, key, ev, &app) -> Act     |
        |                         | Ask : app.ask(ask)  ----------------------------+--> commands (ClientMessage) --> SERVER
        |                         | then Store::collect(app): Over keys removed      |
        |                         v                                                  |
   SERVER --ServerMessage--> game_client::Client (replica, delivery, bships_netcode) |
        |   (fog redacted per team BEFORE publication)                               |
        v                                                                            |
  app model (Host) ---- view fns: fn(&Cx, props) -> Node<Intent>  (state/derive/read/track)
        |                                    |
        |                                    v
        |                         kit_layout (taffy) + kit_text (cosmic-text)  -->  LaidOut
        |                                    |
        |                                    v
        |                         kit_paint  -->  DrawList  --+--> kit_wgpu (GPU, in the game's encoder)
        |                                                     +--> kit_raster / tiny-skia (tests, goldens)
        +--> kit_view::semantics(LaidOut) --> host_web kit_mirror (pointer-transparent DOM; TEST surface only)
```

Per frame, in order: (1) route events against the previous frame's `LaidOut` (so a press hits what the player saw);
(2) `dispatch` each intent; the app forwards asks to the delivery queue; (3) build the `Node` tree from the app model;
(4) lay out at the frame's single viewport; (5) paint to a `DrawList`; (6) render into the host's encoder; (7) after the
host submits, `UiFrame` drops and `frame_submitted()` fences the atlas (an unsubmitted encoder must not see a recycled
page). Set `LaidOut::epoch` when a screen or turn is replaced so a drag begun under another epoch never fires.
Budget: HUD build+layout ~23 us (58 nodes, measured on the taffy path), 5 draw calls in 1 pass for the captain HUD.

Server round trip for one command, as it will look with the ledger (PLANNED parts marked):

1. Press -> `Intent::Step` -> reducer returns `Act::ask(Ask)`; `dispatch` hands it to `Host::ask`.
2. The client queues it in `Delivery` with a sequence. PLANNED: sequence = (boot, n) so a reload cannot reuse old numbers.
3. Server validates, applies or refuses (`OrderRefusal`), publishes. PLANNED P1: a `Mark` (processed-up-to) rides in the
   same body; a refusal is written before any mark covering it.
4. Client folds the body into the replica. PLANNED: the ledger marks the ask `Confirmed` / `Refused` / `Unknown` /
   `Withdrawn`; `cx.track` reads it; the preview derives from the ledger's in-doubt suffix.
5. If the owning key's life ends first: state removed; unsent ask withdrawn (PLANNED); sent ask orphaned until answered.

## 4. Where state lives: four layers, one writer each

| Layer | Lives in | Only writer | Read through |
|---|---|---|---|
| 1. Server | `game_room::Room`, `game_bships`/`game_bnav`/`game_stations`/`game_deck`, `keel_store` (SQLite) | validated commands; `economy::apply` | the wire; fog is redacted per team (`Audience::Team`) before publish |
| 2. Replica | `game_client::Client`: room view, sea snapshot with netcode interpolation, station, social, `draft_context`, `me`, `Delivery` | `Client::on_message`, delivery | `cx.app()`; PLANNED `cx.read`, `cx.track` |
| 3. Prefs (device) | mute, colour vision, UI scale, keybinds, last seat token; today scattered across `www/audio.js` and localStorage | one `PrefChange` path | derived theme/keymap in the app |
| 4. UI state | `kit_state::Store` (declared state, keyed, collected by life) for logical state; `kit_input::UiMemory` for presentational hover/press/drag ghost/scroll offset | `dispatch` / the router | `cx.state` handles; paint reads `UiMemory` |

Rules that keep it one writer: views get `&Cx` (shared references only); every write path is `dispatch` or the client's
message handler; machines hold only UI facts, never game facts (never gold; gold is layer 1 plus the preview); fog is
applied before layer 2, so no screen can show hidden enemy state. `UiMemory` is the pre-existing store that "never
forgets" widgets that leave the screen (audit finding 6); the long-term fix is that logical state moves to `Store`
(collected by life) and `UiMemory` keeps only per-frame presentational state.

## 5. What we own and what libraries own

| Concern | Owner | Why that boundary is lean |
|---|---|---|
| Vocabulary: points, rects, paths, colours, brushes | kurbo / peniko | Types used directly; zero conversion code |
| Flex/grid layout | taffy | The hard algorithm. We own only unit conversion, `place_anchored` (safe-area anchors), `check_cells` (taffy silently ignores `grid_row` on a flex child) |
| Shaping, bidi, line breaking, fallback, editing | cosmic-text | We own `resolve_segments` (our per-property span merge; cosmic replaces whole Attrs) and a bounded raster cache (SwashCache is unbounded) |
| Glyph rasterisation | swash via the text system | Renderers never load fonts; one source of glyph pixels, so CPU and GPU agree |
| Atlas packing | etagere | Replaces a hand-written shelf packer |
| Path tessellation | lyon | Internal to the GPU backend |
| GPU | wgpu; **ours**: the pipeline (SDF round-rect, fill, stroke, glyph quads), batching, draw into the host's encoder | Must share the host's encoder, depth/colour target and frame budget; egui-wgpu owns its own pass; vello's compute passes are heavier than a HUD needs on phones. Floor ~2k of the 7.5k |
| CPU reference | tiny-skia; **ours**: thin dispatcher, shadow blur, glyph tint (~915 lines are test infra) | The cross-backend parity test is the strongest test in the kit |
| View tree, router, hit-test, drag/drop, hotkeys | ours | Every framework's router is welded to its widget objects; ours routes over a laid-out data tree |
| State, life, intents, presence | ours (`kit_state`, ~0.7k) | The part no library models: server-authoritative lives and verdicts |
| Widgets | ours, only what `hud.rs` uses | Look and behaviour are the product; speculative widgets are deleted |
| Platform | winit / web-sys in hosts; `kit_winit` (event conversion, IME area, arboard clipboard, `starter` example) is the native one in the kit | Only `kit_winit` names winit; no other kit crate depends on a platform |
| Accessibility | nobody | Out of scope by owner decision. The semantics extraction and `kit_mirror` remain solely so tests and browser audits can find controls by label |

The `Draw` vocabulary is Fill, Stroke, RoundRect (with shadow and border), Glyphs, Caret, Image and
PushLayer/PopLayer (rect clipping and normal group opacity). Image nodes accept registered app pixels or GPU
texture views. Gradients and image brushes, custom paint, other blend modes, stencil, rounded layer clips and
nine-slice are unsupported. Rounded image corners are coverage of the image itself, not a layer clip.

## 6. Test strategy

Principle (from the repo's testing rules): every test names the bug it catches; prefer one real-fixture test to many
mocks; a pixel assertion is trusted only after a mutant proves it can fail.

Test layout: component tests are `kit_widgets/tests/components/<name>.rs`, one module per component, listed in
`main.rs`; goldens are in `kit_widgets/tests/golden/`. They drive the components through `kit_test::Handle` (the
harness is its own crate so an app can use the same driver; see its crate doc). `tests/common/` holds the helpers that
`behaviour.rs` uses, separate from `Handle`. App tests use `kit_test` as a dev-dependency and keep goldens in their
own `tests/golden/`.

| Layer | Test | Where | Bug it catches |
|---|---|---|---|
| HUD goldens | Pixel goldens of the **real** captain HUD (`Hud::build` -> taffy -> paint -> `kit_raster`, `BoxText` so geometry/colour/layering are pinned, not glyph shapes). Bless with `KIT_BLESS=1` | `keel/games/game_client/tests/hud_golden.rs`, PNGs in `tests/golden/` (26 at the flip: the in-match HUD, every page in `screens.rs`, social dock, station consoles) | A slimming or restyle that changes how any button/card/caption/panel/gauge draws. Before this, all kit_widgets goldens exercised dead widgets (audit trap). CPU only: a GPU twin would make `kit_wgpu` depend on `game_client` |
| Mutant proof | Break the thing the assertion guards (e.g. skip the overlay `record` call) and show the check fails. Recorded for the browser stage (`kit-hud`: with `record` skipped, ink counts went 12864 -> 0 while the mirror still listed every control). Recorded for `hud_golden.rs` (2026-10-23): raising the theme radii (`sm/md/lg/control` 3/6/12/6 -> 13/16/112/16) failed all three goldens (8,033-19,160 pixels off, worst channel delta 86-96); theme restored byte-identical | `agent_notes/2026-10-22_ui_kit_web.md` | Vacuous pixel checks (earlier friend/foe colour checks were vacuous until a mutant failed them) |
| CPU-vs-GPU parity | Same `DrawList` through `kit_raster` and `kit_wgpu`, per `Draw` kind, per-pixel with a documented tolerance, with a per-kind mutation check. Add a fixture when adding a `Draw` kind | `kit_wgpu/tests/parity.rs`, scenes in `kit_raster/src/scene.rs` | Found 3 real GPU bugs: translucent-layer overlap, sampler bleed across atlas seams, caret rounding. Keep whenever a backend changes |
| Router integration | Real `Router` over a real laid-out tree: drag token -> slot yields exactly the tray's `TrayEv::Drop { slot: 2, token: Left }`; tap vs drag-to-nowhere vs drag-back; old-turn drag inert; epoch/capture/cancel/disabled | `kit_input/tests/router.rs`, `game_client/tests/plan_editor.rs` | Gesture and capture regressions; stale-turn drags |
| State behaviour | Real `Store`, real `dispatch`, real `kit_view` trees: independent keys per declaration; one closure in a loop is one declaration, equal bodies are two; `Over` collects and late intents are counted; a step whose ask ends its own key collects in the same dispatch; `Leaving` is inert; zero-duration presence; two toggles in a frame; a todo component renders, dispatches, rerenders | `kit_state/tests/behaviour.rs` | The identity trick, collection, and the zero-length-tween regression (`float.rs:91`) |
| Layout | Generated/engine tests incl. percent-width child and `check_cells` | `kit_layout/tests/*` | 100x percent; silently ignored grid placement |
| Label-based test mirror | `kit_view::testing::control(laid, Role, label)` and the browser `kit_mirror` (`#kit-semantics`, pointer-transparent DOM with `data-kit-id`). Browser audits call `kitFind`/`kitClick` which click the mirror rect's centre with the real mouse, so the canvas hit-test is exercised, and check `document.elementFromPoint` is the canvas | `kit_view/src/testing.rs`, `host_web/src/kit_mirror.rs`, `keel/scripts/lib/kit.mjs`, stage `keel/scripts/test-kit-hud.mjs` | Controls found by number (positional ids shift); DOM CSS making the canvas inert |
| Browser stage | Real GPU pixels, input plumbing, reconnect. Run via `node keel/scripts/audit-url.mjs` (headless full Chrome); since the flip every maintained stage drives the kit through `lib/kit.mjs` | all `audit-url.mjs` stages | What a CPU raster cannot prove |
| Verification gate | `keel/scripts/check.sh` (fmt, clippy, native tests, WASM) | repo | Everything above stays green at each slimming step |

Not written on purpose: tests of cosmic-text kerning/ligatures or taffy grid columns (they test the library), and tests
restating parity-covered pixels. Cost accepted: with `NullText` gone, ~15 test files load the bundled Inter subset and
goldens move on cosmic-text upgrades; re-baseline in one commit.

## 7. Slimming plan and order

Each step keeps `check.sh` and the kit-hud/versus/inventory URL stages green. Status read from the tree on 2026-10-23.

| # | Step | Measured / target | Status |
|---|---|---|---|
| 1 | Real-HUD golden on the CPU backend (tile row + gauge) before deleting anything | audit: "no golden covers the live HUD path" | **Done** in tree: `hud_golden.rs` + 3 PNGs (GPU twin not done, by design) |
| 2 | Delete dead code with no API impact: `kit_anim`, dead widgets, gradient/image/custom/stencil/blend paths, gamepad events, dead theme tokens, `AnchorLayout` (move `place_anchored`) | ~4k lines, largest win, lowest risk | **Partly done**: `kit_anim` gone, `kit_widgets` 1,963 -> 1,014; `kit_wgpu` still has `gradient.rs` (104) and `images.rs` (98) |
| 3 | `etagere` for the atlas | `kit_wgpu/src/atlas.rs` 326 lines to shrink | Done 2026-10-23: `BucketedAtlasAllocator` replaces `Shelf`; fence, LRU, padding kept; new full-page test |
| 4 | `taffy::Style` on `Node`; drop `LayoutEngine` trait; one percent constructor + test | `kit_layout` 1,224 -> ~710; Style translation 197 -> ~45 | Not started |
| 5 | cosmic-text types through kit_view / kit_text / backends; delete `NullText` and the `TextSystem` seam; re-baseline goldens on the Inter subset | `kit_text` 844 -> ~400; `kit_view` text.rs/style.rs mirror types gone | Not started |
| 6 | `kit_state` read/track/ledger; `TextValue` on cosmic `Editor`; **port the plan tray** | microlib ~700 | `state`/`derive`/life/presence BUILT; port not started |
| 7 | Optional: fold `kit_paint` into `kit_view`; split `LaidNode` into controls and containers (cuts ~8 copies of an 11-arm `LaidKind` match); single `StableHasher` (kit_core and keel_math duplicate it); `kit_widgets` stops depending on `kit_raster` (done 2026-10-24: the harness is the dev-only `kit_test`) | | Done |

Traps that must be handled inside the step, not after:

- **Percent units** (step 4): one constructor, test with a percent-width child.
- **`place_anchored` + `span_origin`** survive `AnchorLayout`'s deletion (taffy has no safe-area anchor).
- **`check_cells`** stays; **`resolve_segments`** stays; **bounded raster cache** stays.
- **Zero-duration presence** (step 6): enter -> `Shown` at once, exit holds `Shown` until `Gone`; the regression test is
  already ported into `kit_state`.
- Keep a rect scissor (~30 lines) if scroll lists are coming; cut stencil clip either way.
- `AnchorLayout` treats auto-sized containers as fill-host: tile badges and hotkey overlays need taffy.

Public API changes consumers see (from the audit): `Draw` shrinks to the six kinds; `Node::Image`, `Node::TextField` (until
rebuilt on `TextValue`), `Node::Custom`, `ClipSpec::RoundedBounds` removed; `WgpuUi::{register_image, unregister_image,
register_custom}`, `CustomPaint`, `STENCIL_FORMAT`, `UiRenderer` removed; the `Ui<'t>` facade becomes free component
functions taking `cx`; `LayoutEngine` generics at `hud.rs` and `keel_preview` `hud_ui.rs` become concrete `TaffyLayout`.
Unchanged: the `host_web` `kit_mirror.rs`/`kit_ui.rs` semantics contract; `FontSource`/`SharedFonts`/`TextConfig`.

## 8. What is not here

- **No DOM game UI.** Deleted 2026-10-23 (`host_web` `ui.rs`, `cockpit.rs`, `social_ui.rs`, `plan_drag.rs`,
  `dom_state.rs`, `actions.rs`; the `?ui=kit` switch; most of `style.css`). BUILT: every screen on the kit. The DOM
  keeps, by design: the "WebGPU required" gate and the crash panel (`ui_runtime.js`; must work after the wasm module
  traps), the connection panel (`transport.rs`; must work while the socket is down), and the semantics mirror.
- **Server-authority helpers** (`cx.read`, `cx.track`, the ask ledger, previews): PLANNED. The plan tray derives its
  plan from the client (`Client::plan()`: the sent edit until the echo covers it, else the echo), not from a ledger.
- **Protocol work P1-P5** on the server side; see KEEL_UI_DESIGN.md §9 R2 and R3.
- **Accessibility** beyond the mirror: out of scope by owner decision.
