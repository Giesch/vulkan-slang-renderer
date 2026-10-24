# Watercolor Example: Review Findings and Fix Plan

> **Status: PLANNED** (2026-08-08). Plan for addressing the issues found in a
> full review of `examples/watercolor` (Rust, all eleven slang shaders, the
> paper-texture generator, and the renderer pieces it leans on). Line anchors
> are current as of `origin/main` @ `5de79ab`.
>
> Context: the cross-queue display race documented in
> [watercolor_race_fixes.md](watercolor_race_fixes.md) and
> `render-graph/04_design.md` §8 is **already fixed** on main — pipelined
> compute was removed and the renderer now owns the compute→compute,
> compute→graphics, and cross-frame barriers (`crates/renderer/src/renderer.rs`
> `dispatch` at `:5526`, `record_command_buffer` at `:1687-1720`). This plan
> covers what the review found *beyond* that.

## Findings and decisions

| # | Finding | Decision |
|---|---------|----------|
| 1 | Pigment advection reads last frame's velocity (parity mis-wire) | **Fix** |
| 2 | Simulation samplers use `Repeat` wrap — pigment/bump taps wrap to the opposite canvas edge | **Fix** |
| 3 | Linear filtering on `R32_SFLOAT`/`R32G32B32A32_SFLOAT` is optional in Vulkan | **Accept** — desktop-only support; record the assumption |
| 4 | Granulation mapping inverted vs. its own comment (deposits on peaks, not valleys) | **Fix** |
| 5 | Capillary diffusion creates water; no evaporation, so paint never dries | **Fix** |
| 6 | `JACOBI_ITERATIONS = 2` (was 20 per `frame_inputs_api.md`) barely projects | **Follow-up** — try a higher count |
| 7 | Half-texel misregistration sampling the staggered u/v grids | **Fix** |
| 8 | Stroke points beyond 256/frame silently dropped | **Fix** |
| 9 | Stroke spacing computed in window px against a canvas-px brush radius | **Fix** |
| 10 | egui can swallow `MouseUp`, stranding `painting = true` | **Accept** — egui is a debug/dev tool |
| 11 | Brush accumulation unbounded (up to 256 stamps/frame of pressure + pigment) | **Fix** |
| 12 | Paper-texture generator dims (2048×2048) not linked to canvas dims (2048×1536) | **Fix** |
| 13 | `#[expect(unused)]` handle fields + misleading "kept alive for GPU" comment | **Fix** |

## Fixes

### 1. Advect uses this frame's velocity (`main.rs:670-671`)

During a frame at parity `sim`, `update_velocity` writes `storage[!sim]` and
`project_velocity` projects it in place; the divergence (`:535`, "after vel
flip") and project (`:580`) pipelines already read the `!sim` slot. The advect
pipelines bind `u_in: velocity_u.read_sampled(sim)` — the *pre-update* buffer,
one frame stale.

Change both `u_in` and `v_in` in the advect pipeline creation loop to
`read_sampled(!sim)`. Everything else in that Resources struct stays at `sim`:
`wet_mask` and the pigment ping-pongs are only flipped later in the frame (by
capillary flow) or by this dispatch itself.

### 2. Clamp-to-edge samplers for storage-texture aliases (`renderer.rs:826-831`)

`storage_texture_as_sampled` builds its sampler from
`SamplerOptions::default()` → `TextureWrap::Repeat` (`renderer.rs:3051`).
Consequences in watercolor: the advect backtrace
(`pigmentIn.SampleLevel(backtrace * invGridSize)`) wraps out-of-range
coordinates, pulling pigment in from the opposite canvas edge when painting
near a border; the display shader's bump-map taps at `uv ± texelSize` wrap at
the borders; and even in-range bilinear taps at the outermost half-texel blend
with the far edge.

Watercolor is the **only** caller of `storage_texture_as_sampled` (verified by
grep), so change the method itself to create its sampler with
`ClampToEdge` on both axes (filter stays `Linear`). No new options parameter
until a second caller wants something different.

### 3. (accepted) Linear filtering on 32-bit float formats

`SAMPLED_IMAGE_FILTER_LINEAR_BIT` is not mandatory for `R32_SFLOAT` /
`R32G32B32A32_SFLOAT`; on hardware without it these samplers trip
VUID-vkCmdDraw-magFilter-04553. Decision: **desktop GPUs only** — universally
supported there. Action: a short comment on the sampler creation in
`storage_texture_as_sampled` recording the assumption, so the next reader
doesn't rediscover it the hard way. No runtime gate, no fallback.

### 4. Granulation: deposit in valleys, scale paper influence by γ (`wc_advect_and_transfer_pigment.compute.slang`, `transferChannel`)

Current: `effectiveHeight = lerp(paperHeight, 1.0 - paperHeight, granulation)`
with `adsorb = 1 - effectiveHeight * density`. Deposition is maximal where
`effectiveHeight` is *low*, so with granulation = 1 pigment deposits most on
**peaks** — the comment ("granulating pigments prefer valleys") promises the
opposite. With granulation = 0 the paper still fully modulates deposition,
which is also backwards: γ is supposed to control *how much* the paper texture
matters.

Fix: `effectiveHeight = lerp(0.5, paperHeight, granulation)`.
- γ = 0 → constant `0.5`: paper-independent transfer for non-granulating
  pigments.
- γ = 1 → full paper effect, deposition max in valleys (low `paperHeight` →
  low `effectiveHeight` → high `adsorb`), matching the comment and Curtis
  et al.'s intent. `desorb` moves consistently (pigment sticks in valleys).

Verify visually: French Ultramarine (γ = 0.91) should show clear paper
texture; Hansa Yellow (γ = 0.08) should stay smooth. The effect is small
(densities ≤ 0.09) — a side-by-side stroke pair is the test.

### 5. Conservative capillary diffusion + evaporation (`wc_capillary_flow.compute.slang`, `main.rs` consts)

Two defects, one pass:

**(a) Diffusion creates water.** The gather sums only inflow
(`max(sat_n - sat, 0.0) * step(0.001, wet_n)`) and nothing ever subtracts the
matching outflow from the neighbor, so total saturation grows from nothing.
Replace with a conservative signed pairwise flux, still in gather form:

```
flux(i, j) = diffuseRate * (sat_j - sat_i) * gate(i, j)
sat_i'     = sat_i + Σ_j flux(i, j)
```

where `gate(i, j)` is the wet-gate of the **higher-saturation** cell of the
pair — symmetric in (i, j), so cell j computes exactly `-flux(i, j)` for the
same pair and the global sum is invariant. Water still wicks into dry cells
(the wet source gates the pair open) but the source now pays for it.
Stability: 4-neighbor explicit diffusion needs `diffuseRate ≤ 0.25`; current
0.03 is fine. With conservation, values stay inside the existing range, so the
`min(…, capacity)` clamp becomes a no-op safety net — keep it.

**(b) Nothing dries.** The only saturation sink today is `flow_outward`'s edge
term `eta * (1 - blurred) * wetMask`, which is ≈ 0 in the interior, so
`dry_threshold` is unreachable there and painted regions stay wet (and keep
advecting/transferring pigment) forever, while the wet front creeps outward
indefinitely. Add a per-frame evaporation term: new const
`EVAPORATION_RATE: f32 = 0.002` in `main.rs`, plumbed through the capillary
`Params`; in the shader, after diffusion:
`saturation = max(saturation - evaporationRate, 0.0)`. At 60 fps a saturated
stroke (1.0 → 0.05) dries in ~8 s. The existing threshold logic then works as
designed: saturation crosses `dryThreshold` → wet mask drops → `update_velocity`
zeroes velocity in dry cells → transfer stops, deposit is fixed. Tune the
constant by eye against "stroke dries in 5–15 s".

Requires `just shaders watercolor` (Params layout change).

### 7. Staggered-grid sampling offsets (`watercolor_common.slang`, `wc_update_velocity.compute.slang`)

`bilinearSampleR` does `p = position - 0.5` on both axes — correct only for
cell-centered data. u lives at (i+0.5, j) and v at (i, j+0.5), so the
semi-Lagrangian backtrace samples each velocity component half a texel off in
one axis. Add an offset parameter (the in-cell position of the stored
samples):

```slang
public float bilinearSampleR(Texture2D<float> t, float2 position, float2 gridSize, float2 sampleOffset)
// p = position - sampleOffset
```

Call sites in `wc_update_velocity`: u-field with offset `(0.5, 0.0)`, v-field
with `(0.0, 0.5)`. `bilinearSampleR` has no other callers (advect uses the
hardware sampler), so update the signature directly rather than adding a
wrapper.

### 8 + 9. Stroke pipeline rework: canvas-space stamping with carry-over (`main.rs` input/draw)

These two are one design problem. Today `input()` interpolates in **window**
pixels against `spacing = brush_radius * 0.3` where `brush_radius` is in
**canvas** pixels (2× denser at the default 1024-window/2048-canvas setup),
and `draw()` truncates to `MAX_STROKE_POINTS_PER_FRAME = 256`, silently
dropping the rest of a fast flick.

Restructure:

- `input()` only records raw window-space positions:
  `pending_raw_points: Vec<Vec2>` (MouseDown pushes + sets `painting`,
  MouseMotion pushes while painting, MouseUp clears `painting`; a MouseUp also
  resets the interpolation anchor below so strokes don't connect).
- `draw()` — which has `window_resolution()`, the reason mapping must happen
  here — maps each raw point through `window_to_canvas`, interpolates from
  `last_stamp: Option<Vec2>` (canvas space) at
  `spacing = brush_radius * 0.3` **canvas** px, and appends the resulting
  stamps to a persistent `stamp_queue: VecDeque<Vec2>`.
- Each frame uploads up to `MAX_STROKE_POINTS_PER_FRAME` stamps drained from
  the queue; the remainder carries over to the next frame instead of being
  dropped.

State changes: `stroke_points`/`prev_mouse_pos` → `pending_raw_points`,
`stamp_queue`, `last_stamp`. The old `dist > 1.0` micro-move filter becomes a
canvas-space check (or is subsumed by spacing-based emission).

### 11. Bound per-frame brush accumulation (`paint_brush.compute.slang`)

The stamp loop does read-modify-write of pressure and pigment **per stroke
point** — up to 256 additive applications per pixel per frame, so a slow
wiggly stroke spikes local pressure toward ~512 and dumps pigment to match.
Restructure: the loop only accumulates coverage,

```slang
totalAlpha += alpha;            // per in-radius stamp
...
totalAlpha = min(totalAlpha, 1.0);
```

then apply once after the loop: `wetMask = 1`, `saturation = 1`,
`pressure += brushPressure * totalAlpha`,
`pigment += pigmentColor * totalAlpha`. Per-frame deposition is now bounded
regardless of stamp density (cross-frame buildup — the watercolor-like
behavior — still accumulates), and the shader does one RMW instead of up to
256. Brush feel changes slightly; retune `brush_opacity` if strokes read too
light.

### 12. Link paper-texture dims to the canvas (`generate_paper_texture.rs`, `main.rs`)

The generator hardcodes 2048×2048; the canvas is 2048×1536. The committed PNG
is silently cropped on load, and growing `CANVAS_WIDTH/HEIGHT` past 2048 would
panic inside `image::get_pixel` with an index error instead of the helpful
"run `just watercolor paper-texture`" message (which only covers the
missing-file case).

- Add `examples/watercolor/src/canvas.rs` holding `CANVAS_WIDTH` /
  `CANVAS_HEIGHT`; `main.rs` takes it as `mod canvas;`, the generator bin as
  `#[path = "../canvas.rs"] mod canvas;` (two bins, no lib target needed).
- Generator emits `CANVAS_WIDTH × CANVAS_HEIGHT`; regenerate and commit the
  PNG (`just watercolor paper-texture`).
- `load_paper_height_map` validates the decoded dimensions **exactly match**
  the canvas and fails with a message naming the recipe — covering the
  wrong-size case, not just the missing-file case.

### 13. Delete the "kept alive" fields (`main.rs:117-146`)

`TextureHandle` / `StorageTextureHandle` are plain indices with no `Drop`
(`renderer/texture.rs`, `renderer/storage_texture.rs`); the resources live in
the renderer's storage regardless, and several aliases (and `paper_height`
entirely) are already dropped at the end of `setup` with no ill effect. The
fourteen `#[expect(unused)]` fields and the "kept alive for GPU" comment
imply a lifetime contract the API doesn't have. Delete them all; `PingPong`
becomes a setup-local wiring helper, and the struct shrinks to what `draw`
actually reads (pipelines, buffers, parity, input state).

While in the file: the `pressure_parity` comment still says "flips 20x per
frame" — it flips `JACOBI_ITERATIONS` (currently 2) times. Reword to reference
the const instead of a number.

## Follow-up (separate change)

- **6 — raise `JACOBI_ITERATIONS`.** `frame_inputs_api.md` refers to this loop
  at 20 iterations; it is 2 on main, which barely enforces incompressibility.
  Try 20 (must stay even — the const assert enforces the pressure-slot
  convention), watch the FPS label on real hardware, and check the lavapipe
  sweep margin (`tech_debt.md` §6 — watercolor's first frame already brushes
  `SWEEP_TIMEOUT` on slow machines; the extra per-frame dispatches also slow
  the frames the sweep waits for). If 20 doesn't hold 60 fps, bisect down.

## Explicitly not addressed

- **3** — no runtime gate or fallback for linear filtering of 32-bit float
  formats; desktop-only support is accepted and recorded in a comment.
- **10** — egui swallowing `MouseUp` mid-stroke (all mouse events are gated on
  `!egui_wants_pointer`, `crates/mltrs/src/app.rs:163-196`) can strand
  `painting = true`. The egui panel is a debug/dev tool; accepted.

## Suggested commit order

1. Renderer: clamp sampler + desktop-only comment (fixes 2, 3). Watercolor is
   the only consumer; run `just sweep` anyway since the renderer changed.
2. Shaders: fixes 4, 5, 7, 11 + `just shaders watercolor` (commit regenerated
   `shaders/compiled/` and `src/generated/`).
3. Rust: fixes 1 (pipeline wiring), 8 + 9 (stroke rework), 12 (canvas consts +
   regenerated PNG), 13 (struct cleanup + comments).

Each step leaves the example runnable.

## Verification

- `cargo check --workspace --all-targets`, `just lint`, `cargo fmt`,
  `just test`.
- `just shaders watercolor` after the `.slang` changes; `just watercolor
  paper-texture` after the generator change; commit the regenerated outputs.
- `just sweep` — renderer sampler change + shader changes.
- Manual (`just dev watercolor`):
  - paint along all four canvas edges — no pigment appearing at the opposite
    edge (fix 2);
  - fast flick across the window — continuous stroke, no gaps (fix 8);
  - slow wiggle in place — no pressure blow-out or pigment dump (fix 11);
  - watch a stroke: spreads while wet, then dries and stops moving within
    ~5–15 s; wet-mask debug view shows the front halting (fix 5);
  - French Ultramarine vs. Hansa Yellow side by side — granulation texture on
    the former only (fix 4).
