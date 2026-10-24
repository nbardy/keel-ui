# Whole-frame benchmark: kit vs egui (CPU side)

Run on 2026-10-24 (13:4x local). One run, release build.

## Results

Kit: build, layout, paint and GPU record, as separate phases. egui: `run_ui` plus `tessellate`, CPU only.
Median and p95 are nearest-rank, in milliseconds, per frame.

| Rows | Phase | Frames | Median (ms) | p95 (ms) |
|---:|---|---:|---:|---:|
| 100 | kit view build | 200 | 0.107 | 0.118 |
| 100 | kit layout | 200 | 2.421 | 2.565 |
| 100 | kit paint | 200 | 0.016 | 0.022 |
| 100 | kit GPU record | 200 | 0.789 | 0.972 |
| 100 | kit build + layout + paint | 200 | 2.543 | 2.704 |
| 100 | kit total (incl. GPU record) | 200 | 3.332 | 3.668 |
| 100 | egui run + tessellate (CPU only) | 200 | 0.192 | 0.198 |
| 1000 | kit view build | 200 | 1.666 | 2.067 |
| 1000 | kit layout | 200 | 28.464 | 36.704 |
| 1000 | kit paint | 200 | 0.416 | 0.626 |
| 1000 | kit GPU record | 200 | 2.345 | 3.042 |
| 1000 | kit build + layout + paint | 200 | 30.623 | 39.073 |
| 1000 | kit total (incl. GPU record) | 200 | 33.085 | 42.375 |
| 1000 | egui run + tessellate (CPU only) | 200 | 1.710 | 2.579 |
| 10000 | kit view build | 50 | 20.039 | 21.394 |
| 10000 | kit layout | 50 | 326.036 | 350.231 |
| 10000 | kit paint | 50 | 9.155 | 10.129 |
| 10000 | kit GPU record | 50 | 18.245 | 19.107 |
| 10000 | kit build + layout + paint | 50 | 356.487 | 381.539 |
| 10000 | kit total (incl. GPU record) | 50 | 374.401 | 399.886 |
| 10000 | egui run + tessellate (CPU only) | 50 | 20.851 | 22.164 |

Frames: 20 warm-up frames, then 200 measured. At 10,000 rows the median warm-up frame was over 100 ms, so the
measured run was cut to 50 frames (the rule in the bench). egui used the same frame counts as kit at each N.

## Virtualised list (second run, same day)

The same rows in `kit_widgets::virtual_items`: a scroll area 800 px high, 32 px rows, idle at the top (about 25 rows
plus 3 of overscan built; the other rows are two spacers). The bench prints these as `kit virtual ...` lines next to
the plain `kit ...` lines of the same row count, in one process. The bench does not scroll, so
it measures the idle window only.

**This run was taken while the machine was badly overloaded** (load average 87 to 201 on the M4; the first table was
taken at 9 to 14). Every number from this run is inflated: the plain 10,000-row layout read 2,589 ms here against
326 ms in the first table. Read the virtual lines against the plain lines printed beside them in the same run, not
against the first table. Median ms per frame, 200 frames (the plain 1,000 and 10,000 cases were cut to 50):

| Rows | kit layout, plain | kit layout, virtual | kit build + layout + paint, plain | same, virtual | kit total, virtual (incl. GPU record) |
|---:|---:|---:|---:|---:|---:|
| 100 | 11.269 | 3.098 | 11.969 | 3.335 | 5.740 |
| 1000 | 257.303 | 3.322 | 272.002 | 3.536 | 6.675 |
| 10000 | 2588.571 | 2.567 | 2761.610 | 2.719 | 5.093 |

Layout of the virtual list does not grow with the row count (2.6 to 3.3 ms here for every N; the spread is load).
At 10,000 rows the plain list costs about 1,000 times the virtual one in the same run. Re-run on an idle machine
before quoting an absolute figure.

## Machine

- CPU: Apple M4
- Memory: 24 GiB
- Adapter: Apple M4 (Metal), headless wgpu device, no window
- Date: 2026-10-24

## Command

```
cargo run --release -p kit_bench --bin frame_bench
```

Run from `keel/crates/kit` with `CARGO_TARGET_DIR=keel/target/kit-bench` and `--locked`.

## What is measured

- Each row: a text label, a `kit_widgets` checkbox and a `kit_widgets` button, with its own keyed `cx.state`
  (the README shop pattern). The view is rebuilt every frame.
- The kit frame follows `kit_widgets/tests/components/support.rs`. Build = `store.collect`, `wake_due` and the view
  function. Layout = `TaffyLayout::layout` at 1280x800 logical, scale 1. Paint = `kit_paint::paint`.
  GPU record = `UiFrame::record` into an offscreen RGBA8 texture, encoder finish, submit, then `device.poll(wait)`.
- egui 0.36.2 (`=0.36.2`): the same rows (label, checkbox, button) inside `Context::run_ui`, then `Context::tessellate`.

## Caveats

- **Release build.** Measured with `--release`.
- **GPU timing includes submit and wait.** The GPU record phase ends after `device.poll(wait)`, so it includes
  driver submit and GPU completion, not only encoding.
- **Untimed bookkeeping.** `begin_frame` (before layout), `UiFrame` drop (atlas fence, text cache eviction),
  `Router::frame` and `UiMemory::end_frame` are outside all four phases, so they are not in the totals.
- **egui is CPU only.** egui-wgpu does not match this workspace's wgpu version, so there is no egui GPU number.
- **Not like for like.** egui's button has no icon and the kit's does (the kit uses `labeled_button`). egui has no
  virtualisation here, and neither does the plain-list kit case: both build every row every frame. The `kit virtual`
  cases below do.
- **Shared machine.** Other jobs were running. The load average was about 9 to 14 when
  checked after the run. Treat the numbers as indicative, and expect noise between runs.
- **Single run.** One run per configuration, no repeats. The p95 is over 200 frames (50 at 10k).
- **Post-run edit.** After the run, one line in the bench was changed (a pre-allocated `Vec` for the sample
  buffers, done to satisfy clippy's `repeat_vec_with_capacity`). It runs outside every timed region, so the
  measured binary and the current source should time the same work.
- **Layout dominates at 10k.** Kit layout is about 91% of kit's CPU time at 10,000 rows (326 of 356 ms median).
  The egui side here is only run plus tessellate, so it is not a like-for-like layout comparison.

## Layout fix, 2026-10-24 (list layout was asking the text system ~46 times per row)

Cause, measured with a call counter on `CosmicText::layout` (100-row bench, three text leaves per row): 46 calls per
row per frame, and only 102 real shapings in 220 frames, so every call was a cache hit. A hit still builds the full
cache key (clones the string, builds the span list and attrs) and hashes it. Taffy asks a leaf for its size many times
per solve (flex sizing passes, and every ancestor row/column re-asks), so list layout was dominated by key building.
The HUD does not show it because it has few repeated leaves under nested flex. `TaffyLayout` now remembers each
leaf's answers for the frame, keyed by the room offered after erasing distinctions the measure ignores. Calls drop
from 46 to 2 per row per frame (one measure plus the final `assemble` shaping).

Before and after were run back to back on the same machine, release build, 1,000 rows only (binaries differ only by
the memo; the "before" binary is the fix with the memo lookup disabled). The machine was heavily loaded by other
jobs (load average 46 to 137 during the runs, 94 to 250 earlier), so wall times are noisy; take the ratio, not the
milliseconds.

| Run | Kit layout median (ms) | p95 (ms) | Process user CPU (s), whole run |
|---|---:|---:|---:|
| before, run 1 | 32.9 | 84.1 | 9.47 |
| after, run 1 | 19.8 | 21.6 | 5.65 |
| before, run 2 | 42.0 | 142.8 | 10.77 |
| after, run 2 | 24.6 | 31.5 | 7.11 |

Layout is about 1.7x faster at 1,000 rows. The 100-row and 10,000-row wall times in an earlier same-load pair were
not separable from noise (100 rows: 7.1 ms after vs 8.2 and 15.4 ms before). Not fixed, and still the cost of a list:
a `sample` profile of the fixed binary at 1,000 rows puts layout at roughly half in taffy's flexbox solver, a third
in building the taffy tree (`Frame::build`: style clones, allocation), and the rest in `assemble` and `check_cells`.
Also found: `TextConfig::max_layouts` (4096) is below the number of distinct labels once a list passes ~1,300
three-label rows, so the hard bound evicts layouts that are used every frame and they are re-shaped. Raising it to
100,000 at 10,000 rows cut process user CPU about 10% (67 and 66 s to 59 and 61 s), within noise of this machine;
left as is because the existing test pins the bound.
Guard: `kit_layout/tests/engines.rs::text_system_is_asked_a_bounded_number_of_times_per_text_leaf`.

## Layout keeps its taffy tree, 2026-10-25 (and the HUD-versus-list question)

Cause, from a per-phase timer and `sample`: after the text-call fix a 100-row list frame was about 75% taffy's solver,
10-15% building the taffy tree (a `Style` is hundreds of bytes and was copied six or seven times per node), 10%
`assemble`, 5% `check_cells` and clearing. The `Node` tree is new every frame but the taffy tree it asks for is almost
always the same one as last frame's, so `TaffyLayout` now keeps it. Nodes are streamed in pre-order into the kept tree:
a node at the same place, with an equal style and an equal measure input (text, text style, field, image size), costs a
comparison; a changed style is replaced and a changed leaf is marked dirty (taffy re-solves only the nodes above it);
the first node whose place differs cuts the kept tree off there and the rest is created. A frame that changes nothing
taffy can see never calls the solver (taffy's own node caches answer). The measure input is kept as owned copies of the
text and style; sizes are only reused with the same `CosmicText` (`CosmicText::instance`).

Measured with `cargo run --release -p kit_bench --bin layout_cost` (new): the game-HUD tree (459 laid nodes: 128 text,
114 stack/panel, 217 other) and the bench list (11 laid nodes per row: 3 text, 5 stack/panel, 3 other) in one process,
workloads alternating, trees built outside the timer, one warm text system. Three modes per workload: `rebuilt` (a fresh
`TaffyLayout` every frame: every node created and solved; this is also what every frame cost before this change),
`steady` (one `TaffyLayout`, same tree each frame) and `one edit` (one label's text changes every frame). "Before" is the
tree at `a36a340` with the same bench file; "after" is this branch. Five runs of before then after, back to back, Apple M4,
`uptime` load average (1 minute) at the start of each run 7.5, 6.1, 5.3, 4.8, 4.5 (before) and 6.5, 5.6,
4.6, 4.9, 4.4 (after): the machine was quiet this time, so each number is the median of the five runs' medians, and the
five minimums agree with them to within 10%. Microseconds per frame, layout only:

| Workload | Before (any mode) | After, rebuilt | After, steady | After, one edit |
|---|---:|---:|---:|---:|
| HUD, 459 nodes | 420 | 416 | 134 | 141 |
| list, 100 rows (1,101 nodes) | 2,180 | 2,276 | 355 | 384 |
| list, 1,000 rows (11,001 nodes) | 26,350 | 28,716 | 4,390 | 4,887 |

An unchanged or one-label-edited frame is 3 to 6 times cheaper (list 1,000: 26 ms to 4.4 ms; 100 rows: 6.1x). What is
left in it is walking the `Node` tree (build, compare), `assemble`, which shapes each text at its final width (one
`CosmicText::layout` per text leaf, a key build and a hash each) and rebuilds the `LaidNode` tree, and the style
comparison; none of that is taffy. A frame whose tree changed shape costs what it did, plus 0 to 9% (the owned copies of
each leaf for next frame's comparison); a list that only grows or shrinks at the end keeps every earlier row.

**HUD versus list per node.** Measured like for like (same process, same warm text system, same counting: every laid
node counts once, `Interactive`/`Layer` wrappers included), on the same cold-tree path (`before`, any mode):

| Workload | us per node | us per text leaf | us per stack/panel container |
|---|---:|---:|---:|
| HUD | 0.8 | 2.7-3.0 | 3.1-3.4 |
| list, 100 rows | 1.8 | 6.6 | 3.9-4.0 |
| list, 1,000 rows | 1.9-2.1 | 7.1-7.7 | 4.3-4.6 |

The earlier "HUD 0.4 us per node, list 70 us per row" was not a like-for-like pair and was inflated by machine load
(load average 46 to 250 then): a list row is 11 laid nodes, so at this load it is 20 us (1.8 us per node), not 70. The
real gap is about 2.2x per node, and it is node kind, not load: 45% of a list row's nodes are stack/panel containers
(a row holds a row holds a row for the checkbox and the button), against 25% in the HUD, and the HUD's biggest part is
leaf blocks and labels in flat clusters. Per container the two are within 25% of each other (3.1-3.4 us against
3.9-4.6 us, the list a little worse from deeper nesting and less cache locality), so cost follows nested flex
containers, which is where taffy's solver does its repeated sizing passes. In this session's busy-machine runs (load average 17 to 35) the same before binary read 1.0-1.4 us per node for the HUD
and 2.2-3.8 us for the list: the ratio (2.2 to 2.7x) survives load, only the absolute numbers move.

Guards: `kit_layout/tests/engines.rs::an_unchanged_frame_costs_taffy_nothing_and_an_edited_label_costs_one_leaf` (the
counted `Reuse` of an unchanged, edited, longer, shorter list and a swapped text system, and that an unchanged frame asks
the text system once per leaf) and `kit_layout/tests/generated.rs::a_kept_layout_agrees_with_a_fresh_one_after_every_kind_of_change`
(300 random trees, five frames each, kept layout against a fresh one, rect for rect).
Not done: retaining the shaped text between frames so `assemble` stops hashing a key per label (needs a way to keep a
`CosmicText` layout alive without asking for it), and `TextConfig::max_layouts` (4,096) still evicts above ~1,300 rows.
