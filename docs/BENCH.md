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
  virtualisation here, and neither does the kit: both build every row every frame.
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
