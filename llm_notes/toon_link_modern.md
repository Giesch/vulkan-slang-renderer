# toon_link_modern

Plan for a second Toon Link example, `examples/toon_link_modern`. It renders
the same converted assets as `examples/toon_link` with direct shader code
instead of a TEV interpreter. Status: planned, not implemented.

## Settled decisions

1. New crate `examples/toon_link_modern`. `examples/toon_link` keeps the TEV
   interpreter. One mechanical exception: the new `RasterState` field forces a
   one-line edit at `examples/toon_link/src/main.rs:703`.
2. The target is a perceptual match, not a pixel match. Textures load as sRGB
   views. Lighting math runs in linear space. The shader does not call
   `srgbDecode`.
3. The toon ramp is analytic by default: a smoothstep with tunable band center
   and softness. A runtime switch samples the `ZBtoonEX` LUT instead.
4. Eye-through-bangs compositing uses stencil, not destination alpha.
5. Light directions and colors stay hardcoded in the example.
6. `crates/gx` and `crates/convert-link` do not change. The example reads the
   existing manifest.

## Why this is tractable

The manifest's 24 material slots reduce to three shading archetypes plus one
group that becomes unnecessary:

- **LitToon** (12 slots: `ear`, `face`, `mouth`, `podA`, `sleeve`,
  `ear(2..8)`). The three TEV stages compute:
  `color = albedo.rgb * mix(shadowColor, litColor, ramp(N·L_main)) + eflightTint * ramp(N·L_eflight)`,
  `alpha = albedo.a`. Constants: `shadowColor` = rgb8(156,140,134) (the stage
  `Pale` actor C0), `litColor` = white, `eflightTint` = rgb8(160,90,0) at
  rest, rgb8(255,255,100) × 0.25 with eflight on. The band crosses at
  N·L ≈ 0.294 (ramp step 0.49 minus ambient 0.196). Cull Back, except
  `sleeve` = Cull None.
- **DecalMask** (`*damA`, 4 slots). Writes stencil where texture alpha > 0,
  with color writes off. The manifest's alpha compare on these materials is
  `Always`; coverage came from blending, so the shader needs an explicit
  `discard` on `tex.a <= 0`.
- **Composite** (`eyeL`, `eyeR`, `mayuL`, `mayuR`, 4 slots). Draws with
  stencil test Equal and SrcAlpha/InvSrcAlpha blend. Eyes multiply a second
  sample (`hitomi`) at `uv + (-0.05, 0)`; the offset comes from manifest
  `tex_matrices` slot 1.
- **Erase** (`*damB`, 4 slots). Dropped from the draw list. The render pass
  load-op clears stencil each frame, so the erase pass has no job. The
  classifier still emits material records for these slots, so slot indices
  stay manifest-aligned.

Each mask/composite pair samples the same texel of the same texture, so
SrcAlpha blending inside the stencil region reproduces the destination-alpha
composite math exactly. Stencil supplies only the occlusion that the mask's
z-test supplied.

## Step 1: renderer stencil support

Detailed plan: [toon_link_modern/phase_01_stencil.md](toon_link_modern/phase_01_stencil.md).

The renderer has no stencil path. `find_depth_format`
(`crates/renderer/src/renderer.rs:5070`) prefers `D32_SFLOAT`, which has no
stencil aspect. `create_graphics_pipeline` hardcodes
`.stencil_test_enable(false)` (renderer.rs:4136) and sets no
`stencil_attachment_format`. The main pass binds a depth attachment only
(renderer.rs:2093). The clear value already zeroes stencil, and the barriers
already handle stencil aspects via `has_stencil_component`.

Changes:

- `crates/mltrs/src/game/traits.rs`: add `fn needs_stencil() -> bool { false }`
  to `Game`, following the `max_msaa_samples` pattern. Pass the value into
  `Renderer::init`.
- `renderer.rs`: `Renderer::init` takes `needs_stencil`. When true,
  `find_depth_format` restricts candidates to
  `[D32_SFLOAT_S8_UINT, D24_UNORM_S8_UINT]`.
- `crates/renderer/src/renderer/pipeline.rs`: add
  `enum StencilMode { Disabled, Write { reference: u8 }, TestEqual { reference: u8 } }`
  and a `stencil: StencilMode` field on `RasterState`, `Disabled` in
  `Default`. `Write` = compare Always, pass op Replace. `TestEqual` = compare
  Equal, all ops Keep. Static reference, masks 0xFF, front == back.
- `create_graphics_pipeline`: map `StencilMode` into
  `PipelineDepthStencilStateCreateInfo`. When the depth format has a stencil
  component, also set `rendering_info.stencil_attachment_format`. Dynamic
  rendering requires the matching format on every pipeline that runs while a
  stencil attachment is bound; all main-pass pipelines flow through this
  function, so this one change covers them.
- Begin-rendering: add `.stencil_attachment` (same image view, CLEAR /
  DONT_CARE) when the depth format has a stencil component.
- `examples/toon_link/src/main.rs:703`: add `stencil: StencilMode::Disabled`.
  This call site lists every field on purpose.

The egui pass and the picking pipeline bind no depth attachment and do not
change. Hot reload re-runs `create_graphics_pipeline` with the stored
`raster_state`, so the stencil field carries through.

Verification: `cargo check --workspace --all-targets`, `just lint`,
`just sweep`. Stencil is opt-in and off for every existing example, so the
sweep output must not move.

## Step 2: crate scaffold and LitToon body

- `Cargo.toml` copied from toon_link's. The workspace glob picks the crate up.
  No per-example justfile; the example has no recipes of its own.
- Assets: reference toon_link's by relative path,
  `manifest_path!["..", "toon_link", "assets", "link", "converted"]`
  (`manifest_path!` is at `crates/mltrs/src/util.rs:5`). The assets are
  gitignored, machine-local converter output; do not duplicate them. Keep
  toon_link's missing-asset error message, pointing at
  `just toon_link extract-link && just toon_link convert-link`.
- No ktx2 / `textures` recipe. These PNGs are converter output, off the
  `docs/textures.md` path by design. Load albedo and pupil textures with
  `TextureColorSpace::Srgb`; load the ramp (`runtime_substitution ==
  "toonex"` in the manifest texture list) as `Unorm`.
- Shader `shaders/source/toon_link_modern.shader.slang`, one vert/frag pair.
  Keep toon_link's skeleton: `ParameterBlock` params, push-constant
  `ImmutableAddr<DrawSlot>` indexed by `SV_DrawIndex`, bindless
  `Sampler2D.Handle`, immutable material buffer. Use fresh type names
  (`ModernMaterial`, `ModernDrawSlot`).
  - `enum MaterialKind : uint { LitToon, DecalMask, Composite }`
  - `ModernMaterial { tex0, tex1, kind, hasPupil, pupilOffset }`
  - Params UBO: `mvp`, `lightDirMain`, `lightDirEflight`, `shadowColor`,
    `litColor`, `eflightTint` (all linear), `bandCenter` (default 0.294),
    `bandSoftness` (default ~0.01), `lutAmbient` (default 0.196), `rampMode`,
    `debugMode`.
  - Vertex shader: `mvp.project` and `rotateDirection` only. Lighting is
    per-pixel; normals interpolate.
  - `ModernDebugMode` enum: Final, WorldNormals, Uv0, NdotL, BandOnly,
    AlbedoOnly. The codegen turns it into egui radio buttons.
- Rust side: carry over from `examples/toon_link/src/main.rs` the manifest /
  vertex / index loading and validation, `texture_options`, `load_textures`,
  `resolve_texmap`, the indirect-draw machinery, and the camera / spin / FPS /
  params upload. Keep only azimuth and elevation from the light rig.
- Replace `tev_pack.rs` with a ~60-line classifier over manifest predicates,
  never names (same philosophy as `decal_role`):
  - `channels[0].lighting_enabled` ⇒ LitToon
  - translucent and `z_test` ⇒ DecalMask
  - translucent and blend source `Destination_Alpha` ⇒ Composite, with
    `pupil = texmaps[1].is_some()`
  - translucent and blend mode `None_` ⇒ Erase
  Keep the 4/4/4 group-count assertions.
- Convert authored rgb8 constants and egui color-picker values to linear with
  one `srgb_to_linear` helper before upload.
- This step draws only the opaque body batches with the analytic ramp.
  Verify the band position and eflight tint against toon_link.

## Step 3: decals and stencil

Draw order and raster states, 4 pipelines total:

| group | batches | raster state |
| --- | --- | --- |
| body | 10 opaque slots | Opaque, manifest cull, LessEqual, depth write, stencil Disabled |
| mask | `*damA` × 4 | color_write off, LessEqual, no depth write, stencil Write{1}, shader discard on `a <= 0` |
| face + hair | `face`, `ear(2)` | same as body, full color writes |
| composite | 4 slots | Alpha blend, depth Disabled, no depth write, stencil TestEqual{1}, discard on `a <= 0` |

Body draws first so the mask's z-test rejects stencil writes behind nearer
opaque geometry. `needs_stencil()` returns true. Add a `toon_link_modern)` arm
to `assets_missing()` in `scripts/headless-sweep.sh:104` that tests the
toon_link manifest path, then run `just sweep`.

## Step 4: pupil

Second sample at `uv + pupilOffset` for `eyeL`/`eyeR`. Verify the pupil
position against toon_link.

## Step 5: LUT ramp mode

`RampMode::Lut` samples the ramp at
`float2(saturate(lutAmbient + max(ndl_main, 0)), saturate(lutAmbient + max(ndl_efl, 0)))`.
One sample serves both lights: the LUT is separable, red varies with u only
and green with v only. The egui window switches between modes.

## Step 6: polish

Full `EditState`: fps label, debug mode, ramp mode checkbox, band center and
softness sliders, LUT ambient slider, eflight toggle / tint / falloff /
elevation, shadow and lit color pickers. Update the machine-local-asset
counts in `docs/testing.md`.

## Verification

- `just shaders toon_link_modern` after every `.slang` change. Commit
  `shaders/compiled/` and `src/generated/`.
- `cargo check --workspace --all-targets`, `just lint`, `cargo fmt`,
  `just test`, `just sweep`.
- Side-by-side comparison: run both examples windowed
  (`SDL_VIDEODRIVER=x11 cargo run -p <example>`), capture with
  `xwininfo -name <title>` and `import -window <id>`. Both models spin at
  20°/s from process start, so matched launch times give comparable poses.
- Perceptual checks: band crossing on the tunic, eyes and brows visible
  through the bangs, pupil offset, double-sided sleeve, no black geometry.

## Risks

1. Stencil format availability. `D32_SFLOAT_S8_UINT` or `D24_UNORM_S8_UINT`
   support is universal on desktop Vulkan, but the fallback chain must fail
   loudly if neither is present.
2. Stencil-edge binarization. The stencil region has a hard edge where the
   destination-alpha path had a sub-1/255 fringe. The blend inside the region
   is unchanged, so the difference is one pixel ring at most.
3. Linear-space interpolation shifts the band gradient slightly relative to
   the raw-value math in toon_link. The tunable `bandCenter` and
   `bandSoftness` absorb this.
