# Phase 5: raster state + texture options

Detailed plan for P5 of [`../link_rendering.md`](../link_rendering.md) §6
(renderer §4.3 + §4.4). Estimated: 1–2 days. Verification follows
[`tests.md`](tests.md) §P5, with the same two re-weightings P4 adopted: the
snapshot criterion is restated per-file (Step 4 touches `multi_mesh`'s own
snapshots), and the all-examples sweep stays a documented shell loop, not a
justfile recipe. Renderer-only: independent of the converter track (P0–P3)
and needs no Link assets. Builds directly on P4
([`phase_04.md`](phase_04.md), `4621112`); line numbers below verified at
`4621112`.

**Goal**: after P5 every graphics pipeline carries an explicit `RasterState`
(blend / cull / depth compare / depth write / color write mask) and every
texture an explicit `TextureOptions` (filter / wrap / mipmaps / color space),
both defaulting to today's hardcoded behavior so no existing example changes;
and `examples/multi_mesh.rs` grows test objects that make each non-default
setting visually unmistakable. This is the last renderer prerequisite for the
Link example: P7 needs per-material cull and alpha blend, and §3's color-space
decision (sample **all** Link textures as UNORM, do TEV math on raw values,
apply the inverse-sRGB transfer at the end) is unimplementable without §4.4 —
P2's inventory found **all 44** Link textures want `Unorm` + `ClampToEdge` +
`mipmaps: false` + `Linear`, none of which the renderer can express today.

**Deliverables**

1. `src/renderer/pipeline.rs` — `RasterState` + `BlendMode` / `CullMode` /
   `DepthCompare` with a `Default` impl that reproduces today's pipeline;
   `RendererPipeline.raster_state` replacing `disable_depth_test`;
   `PipelineConfig::with_raster_state`
2. `src/renderer.rs` — `create_graphics_pipeline` consumes `&RasterState`;
   `TextureOptions` / `TextureWrap` / `TextureColorSpace`;
   `Renderer::create_texture_with_options`; plumbing through the non-mips
   texture path and the sampler
3. `shaders/source/multi_mesh.shader.slang` — `float2 uv0` on `Vertex`, one
   `Sampler2D` in the parameter block, fragment modulates by the sample
4. `examples/multi_mesh.rs` — raster-state test objects (cull-front,
   alpha-vs-opaque, depth-write-off) and texture-option test objects
   (wrap × filter grid, sRGB-vs-UNORM pair), with procedurally generated
   textures
5. Master-plan edits: §6 P5 row links here (**already landed** with this
   planning commit, along with the §4.3/§4.4 line-number refresh); mark the row
   ✅ + hash at close-out
6. No `Cargo.toml` change, no changes to existing examples, no askama template
   change, Recorded facts below filled in

## Renderer facts this phase relies on

All at `4621112`; re-verify line numbers before editing (renderer.rs is
actively developed).

### Raster state

- **`create_graphics_pipeline`** (src/renderer.rs:3394) takes
  `depth_test_enable: bool, blend_enable: bool` as its last two parameters.
  Everything else is baked in: cull `BACK` + `COUNTER_CLOCKWISE`
  (3446–3447), blend `SRC_ALPHA` / `ONE_MINUS_SRC_ALPHA` with `BlendOp::ADD`
  for *both* color and alpha (3457–3463), color write mask `RGBA` (3464),
  `depth_write_enable(true)` + `CompareOp::LESS` (3473–3475),
  `depth_bounds_test_enable(false)`, `stencil_test_enable(false)`. Topology,
  polygon mode, line width, depth bias, and the viewport/scissor dynamic state
  are **not** in P5's scope — leave them hardcoded.
- **Exactly three call sites**:
  - picking pipeline, renderer.rs:1036 — passes `false, false` (no depth
    attachment at all, and no blending because the target is a uint format)
  - `init_pipeline`, renderer.rs:1240 — passes
    `!config.disable_depth_test, true`
  - hot-reload recreate, renderer.rs:2613 — passes
    `!render_pipeline_mut.disable_depth_test, true`, reading the flag back off
    the stored `RendererPipeline`
- **`RendererPipeline.disable_depth_test`** (pipeline.rs:177) exists only so
  the hot-reload path can rebuild with the same state; it carries
  `#[cfg_attr(not(debug_assertions), expect(unused))]` because hot reload is
  debug-only. `RasterState` inherits both the role and the attribute.
- **`disable_depth_test` is generated code.**
  `templates/shader_atlas_entry.rs.askama:132` emits
  `disable_depth_test: false,` into every shader's `pipeline_config()`, and the
  field appears in ~20 committed snapshots plus every file under
  `src/generated/shader_atlas/`. Keeping the field (master plan §4.3) is the
  single decision that makes P5 snapshot-neutral outside `multi_mesh`.
- Only two examples set it: `examples/sprite_batch.rs:87` and
  `examples/space_invaders.rs:165`, both `= true` after
  `pipeline_config(resources)`.
- `PipelineConfig` is pipeline.rs:208, `PipelineConfigBuilder` pipeline.rs:242
  (its `build()` copies the field across), `with_shared_mesh` pipeline.rs:236 —
  the precedent for a consuming `with_*` builder method on `PipelineConfig`.

### Texture options

- **`TEXTURE_IMAGE_FORMAT`** (renderer.rs:3968) is a file-level
  `R8G8B8A8_SRGB` const, used by the non-mips path in two places: the image
  view (in `create_texture`, 3996) and the staging/transition code inside
  `create_texture_image` (4040).
- **`format_block_info`** (3984) already accepts both `R8G8B8A8_SRGB` and
  `R8G8B8A8_UNORM`, and both `Renderer::create_texture_with_mips` (520) and
  the free `create_texture_from_mips` (4134) already take an explicit
  `vk::Format`. **So UNORM support only has to be added to the non-mips
  path** — the pre-baked-mip path already has it.
- `create_texture_image` (4040) derives `mip_levels` from the extent
  (`image.width().max(image.height()).ilog2() + 1`, 4055) and unconditionally
  calls `generate_mipmaps` (4115). It converts via `image.to_rgba8()` and
  `debug_assert`s a 4-bytes-per-texel size, so both formats are byte-identical
  on the upload side; only the `vk::Format` differs.
- **`create_texture_sampler`** (4543) hardcodes `REPEAT` on u/v/w,
  `anisotropy_enable(true)` with the device max, `mipmap_mode(LINEAR)`,
  `min_lod(0.0)`, `max_lod(LOD_CLAMP_NONE)`,
  `border_color(INT_OPAQUE_BLACK)`. Only the filter is a parameter today.
- **Three callers** of `create_texture_sampler`: `create_texture` (3996),
  `create_texture_from_mips` (4134), and `storage_texture_as_sampled`
  (renderer.rs:613, passes `TextureFilter::Linear`). All three must keep
  compiling when the signature changes.
- `create_vk_image`'s `ImageOptions` struct is the in-repo precedent for
  passing image parameters as a struct instead of a positional list.
- Seven current `create_texture` call sites across `examples/` (depth_texture,
  koch_curve, serenity_crt, space_invaders, sprite_batch, viking_room; suzanne
  uses `create_texture_with_mips`). None may change.

## Step 1 — `RasterState`

In `src/renderer/pipeline.rs`, beside `PipelineConfig`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// today's behavior: SRC_ALPHA / ONE_MINUS_SRC_ALPHA, ADD
    Alpha,
    /// blending disabled; the fragment's alpha is ignored
    Opaque,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullMode { Back, Front, None }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthCompare { Less, LessEqual, Always, Disabled }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterState {
    pub blend: BlendMode,
    pub cull: CullMode,
    pub depth_test: DepthCompare,
    pub depth_write: bool,
    pub color_write: [bool; 4],
}

impl Default for RasterState {
    fn default() -> Self {
        // exactly today's hardcoded pipeline
        Self {
            blend: BlendMode::Alpha,
            cull: CullMode::Back,
            depth_test: DepthCompare::Less,
            depth_write: true,
            color_write: [true; 4],
        }
    }
}
```

- **`BlendMode` is `Alpha | Opaque` only.** Link's MAT3 also uses one
  destination-alpha variant (master plan §3); it is deliberately deferred to
  P8, where the eye trick that needs it lives — see "Out of scope". Adding it
  here would ship an unverified variant.
- Front face stays `COUNTER_CLOCKWISE` for every mode. `CullMode::Front` is a
  *test* affordance (it makes a closed mesh render inside-out, which is loud);
  the Link winding question (master plan risk #3) is resolved in P6 by
  flipping triangle order in the converter, not by flipping `front_face`.
- `DepthCompare::Disabled` maps to `depth_test_enable(false)`; the other three
  map to `depth_test_enable(true)` + the matching `vk::CompareOp`. Note that
  Vulkan still honors `depth_write_enable` when the test is disabled, so
  `Disabled` + `depth_write: true` writes unconditionally — document that,
  since it is a plausible accident.
- `create_graphics_pipeline` (renderer.rs:3394) takes `raster_state:
  &RasterState` in place of the two bools and derives `cull_mode`,
  `front_face` (unchanged), the color blend attachment, the color write mask,
  and the depth-stencil state from it. Keep a small private helper per field
  (`fn vk_cull(CullMode) -> vk::CullModeFlags`, etc.) so the mapping is
  greppable and unit-testable.
- Picking (1036) passes an explicit literal — `RasterState { blend: Opaque,
  depth_test: Disabled, depth_write: false, ..Default::default() }` — with a
  comment that its render target is a uint format with no depth attachment.
  This is a behavior-preserving translation of `false, false`; confirm
  `depth_write: false` matches what a pipeline with no depth attachment does
  today (it does: with `depth_attachment_format` `UNDEFINED`, the
  depth-stencil state is ignored, so any value is fine — pick the honest one).
- `RendererPipeline.disable_depth_test` (pipeline.rs:177) becomes
  `raster_state: RasterState`, keeping the
  `#[cfg_attr(not(debug_assertions), expect(unused))]` attribute; the
  hot-reload recreate (2613) passes `&render_pipeline_mut.raster_state`.
- Gate: `cargo check --all` clean; `timeout 3 just dev basic_triangle`,
  `depth_texture`, `gpu_picking` behave identically with no validation output.

## Step 2 — builder surface

```rust
impl<'t, V: VertexDescription, D: DrawCall> PipelineConfig<'t, V, D> {
    pub fn with_raster_state(mut self, raster_state: RasterState) -> Self;
}
```

- `PipelineConfig` gains `raster_state: RasterState` (defaulted at
  construction), and `PipelineConfigBuilder` copies it in `build()` the way it
  copies `disable_depth_test`.
- **`disable_depth_test` stays** on both structs and in
  `templates/shader_atlas_entry.rs.askama` — do not touch the template. At
  `init_pipeline` (renderer.rs:1240) resolve the two into one state:

  ```
  effective = config.raster_state;
  if config.disable_depth_test { effective.depth_test = DepthCompare::Disabled; }
  ```

  i.e. **`disable_depth_test` wins when set**, because it is the older,
  coarser knob and the only one two existing examples use. Document that
  precedence on `with_raster_state`, and point callers who want both at
  setting `depth_test: Disabled` directly.
- Gate: `cargo check --all`; `just test` — **every** snapshot byte-identical at
  this point (nothing generated has changed yet); `timeout 3 just dev
  sprite_batch` and `space_invaders` still render correctly (they are the two
  `disable_depth_test` users, so they are the precedence test).

## Step 3 — `TextureOptions`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureWrap { Repeat, ClampToEdge, MirroredRepeat }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureColorSpace { Srgb, Unorm }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextureOptions {
    pub filter: TextureFilter,
    pub wrap_u: TextureWrap,
    pub wrap_v: TextureWrap,
    pub mipmaps: bool,
    pub color_space: TextureColorSpace,
}

impl Default for TextureOptions { /* Linear, Repeat, Repeat, true, Srgb */ }

impl Renderer {
    pub fn create_texture_with_options(
        &mut self,
        source_file_name: impl Into<String>,
        image: &image::DynamicImage,
        options: TextureOptions,
    ) -> anyhow::Result<TextureHandle>;
}
```

- `Renderer::create_texture` (490) becomes a wrapper:
  `self.create_texture_with_options(name, image, TextureOptions { filter,
  ..Default::default() })`. All seven existing call sites are untouched.
- Free `create_texture` (3996) takes `TextureOptions` instead of
  `TextureFilter` and threads it down.
- `create_texture_image` (4040) takes `TextureOptions`: pick the `vk::Format`
  from `color_space` instead of the `TEXTURE_IMAGE_FORMAT` const, and when
  `!options.mipmaps` set `mip_levels = 1` and skip the `generate_mipmaps` call
  (4115) entirely. Keep `TEXTURE_IMAGE_FORMAT` only if something else still
  uses it; otherwise replace it with a
  `fn texture_format(TextureColorSpace) -> vk::Format` helper so there is one
  place the format is decided.
- `create_texture_sampler` (4543) takes `TextureOptions`: address modes u/v
  from `wrap_u`/`wrap_v` (w mirrors `wrap_u` — 2D textures never sample it,
  but leaving it `REPEAT` while u/v clamp is gratuitously inconsistent);
  `anisotropy_enable(options.mipmaps)` with `max_anisotropy` 1.0 when off; and
  **`max_lod(0.0)` when mipmaps are off** (with `LOD_CLAMP_NONE` and a
  1-mip image the sampler is still fine, but being explicit is what keeps
  risk #3 below from biting).
- `create_texture_from_mips` (4134) keeps its explicit-format signature; give
  it a `TextureOptions` parameter only for the sampler (it must not take
  `mipmaps: false` — bail if asked, since its whole job is uploading a mip
  chain). `storage_texture_as_sampled` (613) passes
  `TextureOptions::default()`.
- Gate: `cargo check --all`; `just test` still fully byte-identical; sweep the
  texture-using examples (depth_texture, koch_curve, serenity_crt,
  space_invaders, sprite_batch, suzanne, viking_room) and confirm they look
  unchanged — this is the "defaults reproduce today's behavior" check, and
  `serenity_crt` (Nearest) plus `suzanne` (pre-baked mips) are the two that
  exercise the non-obvious paths.

## Step 4 — shader + codegen

`shaders/source/multi_mesh.shader.slang` gains texturing so the wrap/filter
and color-space objects have something to sample:

- `Vertex { float3 position; float3 normal; float2 uv0; }`
- `MultiMeshParams` unchanged (`MVPMatrices mvp; float4 tint;`); the parameter
  block gains one `Sampler2D`
- vertex passes `uv0` through unmodified — **do not** clamp or wrap it in the
  shader, or the sampler's wrap mode becomes untestable
- fragment: `float3 c = params.tint.rgb * tex.Sample(uv).rgb * (0.35 + 0.65 *
  max(dot(n, L), 0));` — a white texture reproduces P4's exact output, so the
  existing shapes are unaffected by the change

Then `just shaders` → `just insta` → `INSTA_UPDATE=no cargo test`.

**Expected snapshot churn, and nothing else** (this is the restated tests.md
criterion):

- `…generated_files@src__generated__shader_atlas__multi_mesh.rs.snap` — new
  `uv0` vertex attribute + the sampler in `Resources`/`layout_bindings`
- `…generated_files@shaders__compiled__multi_mesh.json.snap` — reflection
- `…shader_branching_snapshots.snap` — the `multi_mesh.frag` line, if the
  branch count moves (P4 discovered this snapshot exists; treat a change here
  as expected, not a regression)
- **no** atlas-index change (the shader already exists — unlike P4)
- **every other per-shader snapshot byte-identical**

If `Resources` gaining a texture handle forces a signature change on
`pipeline_config`, that is the generated code doing its job; the example
adapts, no template edit.

## Step 5 — `examples/multi_mesh.rs` test objects

The example's invariant stays: `DRAWS` holds `(index_count, pipeline_index)`
with `first_index` derived as a running sum, and
`const _: () = assert!(draws_total(&DRAWS) == INDEX_COUNT)` plus the runtime
`assert_eq!(indices.len(), INDEX_COUNT as usize)` keep coverage honest. Bump
`INDEX_COUNT` as geometry is added. `PIPELINES` grows a `RasterState` and a
texture per entry, since state and textures are per-pipeline.

Two procedurally generated textures, built in the example with the `image`
crate (`src/bin/generate_paper_texture.rs` shows the idiom; no new asset
files, nothing committed):

- an 8×8 high-contrast checkerboard — small enough that `Nearest` vs `Linear`
  and `Repeat` vs `ClampToEdge` are unmissable
- a flat mid-gray (128,128,128) fill — the color-space probe
- a 1×1 white, for every pre-existing shape (reproduces P4's look exactly)

Test objects to add, each a quad or cube placed in a row behind the existing
cube/pyramid/disc so the original P4 scene stays readable:

| object | state / options | expected image |
|---|---|---|
| cube, `cull: Front` | `RasterState { cull: Front, ..default }` | renders inside-out: you see the interior of the far faces, silhouette inverted as it spins |
| quad pair, alpha over opaque | front quad `blend: Alpha` with tint alpha 0.5; back quad `blend: Opaque` | the front quad is translucent; the opaque control quad beside it is not, despite the same tint alpha |
| quad pair, depth-write off | front quad `depth_write: false`, drawn first | the quad behind it draws over it where they overlap — an artifact that only appears if `depth_write` reached the pipeline |
| 2×2 quad grid | checkerboard, UVs spanning `[-0.5, 1.5]`; `ClampToEdge`×`Linear`, `Repeat`×`Linear`, `ClampToEdge`×`Nearest`, `Repeat`×`Nearest` | clamp smears the border texels outward; repeat tiles 2×2; nearest is hard-edged, linear is smooth |
| gray quad pair | same mid-gray image, one `Srgb` one `Unorm` | the sRGB quad is visibly lighter — this is the check P8's TEV gamma path depends on |

- All new pipelines use `mipmaps: false` for the checkerboard (an 8×8 with a
  full mip chain averages to flat gray at distance and destroys the test) —
  which conveniently also exercises the `mipmaps: false` path Link needs.
- Comments should say *why* each object exists (which renderer knob it
  proves), in the style of the existing header comment about per-pipeline
  uniforms.
- Adding this many pipelines is fine — it also re-exercises P4's queue with a
  larger N and a mix of raster states in one render pass.

## Step 6 — verification + docs

Run the test plan below; do the perturbation check; fill Recorded facts; mark
the master plan §6 P5 row ✅ with the commit hash.

## Test plan

**Automated (`just test` / CI):**

- Insta: gate is *every per-shader snapshot except `multi_mesh`'s
  byte-identical*, `multi_mesh`'s three changed as described in Step 4, no
  atlas-index change, no new snapshot files.
- Unit test the `RasterState` → Vulkan mapping helpers (each enum variant →
  its `vk::` constant) and `RasterState::default()` against the literal values
  `create_graphics_pipeline` used before this phase — that assertion is what
  proves "defaults reproduce today's behavior exactly."
- `cargo check --all` + `cargo build --examples`; `just lint` clean.

**Validation sweep** — documented loop, not a recipe (as in P4):

```sh
for e in basic_triangle depth_texture dragon gpu_picking koch_curve multi_mesh \
         particles ray_marching sdf_2d serenity_crt space_invaders sprite_batch \
         suzanne viking_room watercolor; do
  timeout 3 just dev "$e" 2>&1 | grep -iE "validation|VUID" && { echo "FAIL: $e"; exit 1; }
done; echo "sweep clean"
```

(Adjust to `ls examples/` at run time.) Sampler/format changes are exactly the
kind of thing the validation layers catch, so this is the primary automated
signal for §4.4.

**Eyeball (results → Recorded facts):** one line per row of the Step 5 table,
recording what was actually seen — plus:

1. The original P4 scene (cube, pyramid, tricolor disc) is unchanged, proving
   the white-texture path and `RasterState::default()` are truly no-ops.
2. **Perturbation — test the test, then revert:** for each of `cull`,
   `blend`, `depth_write`, `wrap_u`, `filter`, `color_space`, set that one
   field back to its default on its test object and confirm the artifact
   disappears. Any object whose appearance does *not* change is a test object
   that proves nothing — that is the failure this step exists to catch.
3. **Hot reload:** edit the fragment body while multi_mesh runs; every
   pipeline recreates through P4's deduped-index path and each keeps its own
   `RasterState` (the cull-front cube must stay inside-out after the reload —
   this is risk #2's live check).
4. Clean exit with **no VMA leak report**: the new textures must be reaped.
   Requires a real window close (`WM_DELETE_WINDOW`); `timeout`'s SIGTERM
   skips `Drop`, as P4 recorded.

## Verification (exit checklist)

- [x] `just test` green; snapshot diff is exactly `multi_mesh`'s own files
- [x] `RasterState` default-equivalence + enum-mapping unit tests green
- [x] `just lint` clean; `cargo build --examples` clean
- [x] Validation sweep clean over all examples (loop above)
- [x] Original P4 scene visually unchanged in multi_mesh
- [x] Each Step 5 test object shows its expected artifact; results recorded
- [x] Perturbation performed and reverted for all six fields; results recorded
- [x] Hot reload preserves per-pipeline raster state
- [x] No VMA leak report on multi_mesh exit
- [x] No changes to existing examples, `Cargo.toml`, or
      `templates/shader_atlas_entry.rs.askama`; `git diff` on `src/generated/`
      is limited to `multi_mesh`
- [ ] Master plan §6 P5 row marked ✅ with the commit hash
- [x] Recorded facts filled in

## Recorded facts

```
commit:                   (pending — fill in hash when committed)

final API line numbers:   src/renderer/pipeline.rs — RendererPipeline.raster_state 177,
                          BlendMode 182, CullMode 193, DepthCompare 201, RasterState 216,
                          Default impl 225, PipelineConfig 265, with_raster_state 309,
                          PipelineConfigBuilder 315, build()'s RasterState::default() 338.
                          src/renderer.rs — create_texture 492, create_texture_with_options 511,
                          picking raster literal 1060, init_pipeline 1263, TextureWrap 2893,
                          TextureColorSpace 2905, TextureOptions 2914 (Default 2924),
                          vk_cull_mode 3479, vk_depth_compare 3489, vk_color_write_mask 3498,
                          create_graphics_pipeline 3516, texture_format 4092,
                          create_texture_image 4169, vk_address_mode 4702,
                          create_texture_sampler 4710.

snapshot churn:           exactly two files, both multi_mesh's:
                          generated_files@src__generated__shader_atlas__multi_mesh.rs.snap
                          (uv0 attribute at location 2, `texture: &TextureHandle` in
                          Resources, the handle pushed into texture_handles) and
                          generated_files@shaders__compiled__multi_mesh.json.snap.
                          shader_branching_snapshots.snap did NOT move (predicted "possibly").
                          No atlas-index change; every other per-shader snapshot
                          byte-identical. After Steps 1-4 (before the shader edit) `just test`
                          was fully byte-identical, confirming the API work alone is
                          snapshot-neutral.

default-equivalence:      renderer.rs `mod tests` (existing) gained four unit tests:
                          cull_mode_mapping, depth_compare_mapping, color_write_mask_mapping,
                          and raster_state_default_matches_original_hardcoded_pipeline, which
                          asserts RasterState::default() maps to CullModeFlags::BACK,
                          BlendMode::Alpha (blend_enable true), ColorComponentFlags::RGBA,
                          (depth_test_enable true, CompareOp::LESS) and depth_write true —
                          i.e. literally the values create_graphics_pipeline hardcoded at
                          4621112. 5 tests total in that module, all green.

raster-state results:     cull Front cube — renders inside-out: the large face nearest the
                          camera is a dark interior back face under a bright top face, and
                          the corner points away from the viewer. Perturbing to Back flips
                          it to a bright outward-facing front face with the sides in shadow.
                          blend Alpha vs Opaque — the two panels carry the identical
                          HALF_YELLOW tint (alpha 0.5) over one slate backdrop: the Alpha
                          panel reads muted olive (tint blended with the backdrop), the
                          Opaque one bright yellow.
                          depth_write off — the cyan panel is nearer and queued first but
                          writes no depth, so the farther magenta panel drawn after it covers
                          the overlap. With depth_write on, cyan occludes magenta instead.
                          color_write: [bool; 4] is plumbed and unit-tested but has no test
                          object; it is exercised by the eye trick in P9 (§4.5).

texture-option results:   wrap x filter row, four panels, one 8x8 checkerboard image, UVs
                          spanning [-0.5, 1.5] in both axes, all mipmaps:false:
                            ClampToEdge x Linear  — 2x2 blocks, border texels smeared over
                                                    the outer quarter, soft internal edges
                            Repeat      x Linear  — tiles to a 4x4 checker, soft edges
                            ClampToEdge x Nearest — same 2x2 blocks, hard edges
                            Repeat      x Nearest — 4x4 checker, hard edges
                          All four visibly distinct along both axes.
                          srgb vs unorm gray pair, one 8x8 (128,128,128) image: measured
                          from a screenshot, Srgb panel = 117, Unorm panel = 172 (8-bit,
                          B8G8R8A8_SRGB swapchain). Decoding both back to linear gives
                          0.178 and 0.413, a ratio of 2.32 — exactly 0.502/0.2158, the ratio
                          of the raw value to its sRGB-decoded value — and both imply the
                          same lambert 0.82. Numerically exact, not just "looks different".

perturbation results:     all six fields set back to their default, rebuilt, captured, then
                          reverted. The six artifacts are spatially disjoint so one run
                          covered them all.
                            cull        Front -> Back: cube flips to a normal outward cube
                            blend       Alpha -> Opaque: the two yellow panels become
                                        identical, the muted olive disappears
                            depth_write false -> true: the occlusion reverses, cyan now
                                        covers magenta
                            wrap_u      ClampToEdge -> Repeat on grid panel 1: it becomes
                                        the 4x4 tiled checker, identical to panel 2
                            filter      Nearest -> Linear on grid panel 3: its edges go soft,
                                        matching panel 1
                            color_space Unorm -> Srgb: both gray panels measure 117/117
                          Every object changed; none proved nothing.

hot-reload:               edited the fragment body live (ambient term 0.35+0.65 -> 0.15+0.85)
                          while multi_mesh ran with 17 graphics pipelines. All pipelines
                          recreated through P4's deduped-index path; lighting visibly changed,
                          and the cull-front cube stayed inside-out while the blend and
                          depth-write artifacts stayed intact — risk #2's live check passes,
                          so the recreate really does read raster_state off RendererPipeline.

sweep:                    15/15 examples clean (basic_triangle, depth_texture, dragon,
                          gpu_picking, koch_curve, multi_mesh, particles, ray_marching, sdf_2d,
                          serenity_crt, space_invaders, sprite_batch, suzanne, viking_room,
                          watercolor), no validation/VUID output. Run twice: once after
                          Steps 1-4 with the examples untouched (the "defaults reproduce
                          today's behavior" check) and once at the end.

VMA leak:                 clean exit, status 0, zero output. Verified the check is not
                          vacuous by injecting a leak (skipping one texture in the teardown
                          loop): VMA aborts with "Some allocations were not freed before
                          destruction of this memory block!" and dumps core. Reverted.
                          Getting a Drop-running exit needed a temporary frame-limit escape
                          in app.rs — SDL3 posts no Quit event on SIGINT/SIGTERM here, so
                          P4's note that `timeout` skips Drop understates it; there is no
                          signal that works. Both temporary edits reverted.

deviations discovered:    1. PipelineConfigBuilder gained NO raster_state field, contrary to
                             Step 2. templates/shader_atlas_entry.rs.askama:126 emits a
                             complete struct literal, so a required field would have broken
                             every generated file and forced the template edit risk #1 exists
                             to avoid. build() sets raster_state: RasterState::default()
                             instead, and with_raster_state overrides it. Template untouched.
                          2. Step 5's sRGB expectation was inverted. The swapchain is
                             B8G8R8A8_SRGB, so a mid-gray sampled as Srgb decodes to linear
                             0.216 and re-encodes back to ~128 on screen, while Unorm hands
                             0.502 to the shader and encodes to ~188. The UNORM panel is the
                             lighter one. Measured 117 vs 172 (lambert-scaled).
                          3. Skipping generate_mipmaps also skips its final layout
                             transition — that function leaves every level in
                             SHADER_READ_ONLY_OPTIMAL. The mipmaps:false path needed an
                             explicit transition_image_layout from TRANSFER_DST_OPTIMAL, or
                             the image stays in the copy's layout. Not anticipated by Step 3;
                             a black-texture/validation bug if missed.
                          4. Test objects are placed in VIEW space (a `Panel` with a screen
                             position in viewport-half-height units, a depth, and half
                             extents), not as a world-space row behind the scene. Two earlier
                             world-space layouts failed: a static row is backface-culled for
                             half of every orbit, and a camera-locked row still gets occluded
                             by the orbiting shapes and mis-frames badly under the camera's
                             downward tilt and a tiled window's aspect ratio. View-space
                             placement in front of the shapes makes every artifact readable
                             at every moment, at any aspect ratio.
                          5. The wrap x filter objects are a row of four, not a 2x2 grid:
                             the camera's tilt leaves only a shallow band of screen clear of
                             the shapes, too short to stack two rows.
                          6. Geometry counts landed at INDEX_COUNT 210 / 18 draws /
                             17 pipelines / 7 texture handles. Every panel shares one unit
                             quad shape (its on-screen size lives in the model matrix), and
                             the blend group needed a backdrop panel — translucency over
                             nothing proves nothing — which the Step 5 table omitted.
                          7. The fragment shader now returns params.tint.a rather than 1.0,
                             without which BlendMode::Alpha has nothing to blend. Step 4's
                             snippet omitted it. Existing tints are alpha 1.0, so the
                             original scene is unaffected.
```

## Out of scope for P5

- **`BlendMode::DstAlpha`** — Link's one destination-alpha material (master
  plan §3) lands with the eye trick in **P9**/§4.5, not here. P8 must not
  assume the variant exists.
- Stencil, depth bias, polygon mode, primitive topology, per-attachment blend
  (there is one color attachment), independent color/alpha blend factors,
  logic ops
- Configurable `front_face` — the Link winding decision is P6's, made in the
  converter (master plan risk #3)
- `TextureWrap::ClampToBorder` and border color selection; 3D/array/cube
  textures; anisotropy as an explicit knob (it is derived from `mipmaps`)
- Block-compressed formats — `format_block_info` (3984) is where they would be
  whitelisted; P2 already decodes everything to RGBA8, so Link doesn't need
  them
- Picking + multi-draw (still deferred from P4, master plan §4.5) and the
  FrameInputs migration ([`../frame_inputs_api.md`](../frame_inputs_api.md))
- The render-graph design ([`../render-graph/04_design.md`](../render-graph/04_design.md))
  — see risk #6

## Risks / open questions

1. **Snapshot churn from the template.** The whole
   "`disable_depth_test` stays" decision exists so
   `templates/shader_atlas_entry.rs.askama` is never touched. Editing it
   rewrites every file in `src/generated/shader_atlas/` and ~20 snapshots, at
   which point "did anything unexpected change?" stops being answerable by
   inspection. If a future phase does remove the field, do it as its own
   commit with no other changes.
2. **Hot-reload recreate must read `raster_state`** off `RendererPipeline`
   (2613). If it falls back to `RasterState::default()`, a reloaded shader
   silently reverts to alpha-blend/back-cull/depth-less — invisible until a
   material looks wrong, which is exactly the class of bug P8 will be
   debugging. The Step 6 hot-reload check is the guard.
3. **Sampler LOD with `mipmaps: false`.** A 1-mip image with
   `max_lod(LOD_CLAMP_NONE)` is legal, but if `mip_levels` and the sampler
   ever disagree the result is a black or garbage texture rather than an
   obviously wrong one. Set `max_lod` from the same value that sizes the
   image, in one place.
4. **UNORM assumptions elsewhere.** `format_block_info` whitelists UNORM and
   the upload path is format-agnostic, but confirm nothing downstream assumes
   sRGB — in particular the MSAA resolve and the final blit, which are keyed
   to the *swapchain* format and should be unaffected. If the gray-quad
   brightness test shows no difference, that assumption is wrong.
5. **Draw order now matters.** With mixed blend and depth-write state in one
   render pass, the order of `DRAWS` is semantically load-bearing for the
   translucent objects. That is a real property, not a bug — it is the same
   two-pass rule the Link example follows (opaque batches before translucent,
   master plan §2.3). Say so in the example's comments so the next reader
   doesn't "fix" the ordering.
6. **Render-graph drift.** `../render-graph/04_design.md` (landed `73fb65f`)
   is an independent design that also touches pipeline creation; it is gated
   on `../bda_footguns/03_pipelined_current_read_plan.md` and does not block
   P5. Keep P5 additive — new fields on `PipelineConfig` and a new `with_*`
   method — and avoid restructuring the config type, so the graph work
   doesn't have to undo it.
