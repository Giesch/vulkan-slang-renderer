# Rendering Toon Link: implementation plan

Goal: a new example (`examples/toon_link.rs`) that renders a **static, cel-shaded
Toon Link** from *The Legend of Zelda: The Wind Waker*, using assets extracted from
the decompilation project at `../tww`. Companion to
[`../tww/docs/link-rendering.md`](../../tww/docs/link-rendering.md), which maps the
original game's code and asset paths; this document plans the port into this
renderer.

Companion documents: [`link_rendering/risks.md`](link_rendering/risks.md)
(expanded risk walkthrough) and
[`link_rendering/tests.md`](link_rendering/tests.md) (per-phase correctness
testing: external oracles + our own tests).

## Settled decisions

1. **TEV translation — hybrid "subset compiler."** Dump `cl.bdl`'s MAT3 chunk,
   then mechanically implement exactly the TEV feature subset Link's ~20 materials
   use (correct stage equations, texgens, register semantics), structured for later
   extension. Not a full generic GX→shader translator (noclip.website-scale
   project), not hand-eyeballed shaders (subtly wrong colors, Link-only).
2. **Asset storage — gitignored + extraction script.** Assets are pulled from the
   user's disc image in `../tww` via `dtk` and converted locally. Nothing
   Nintendo-copyrighted is ever committed; anyone with the disc image can
   reproduce.
3. **Renderer extension — queue-based multi-draw + index-range draws.** The
   renderer currently makes exactly one draw call per frame. Extend
   `FrameRenderer` with a pending-draw queue (mirroring the existing
   `pending_compute` pattern) and let multiple pipelines share one vertex/index
   buffer, each drawing an index sub-range.
4. **Pose/skinning — baked pose, skeleton preserved.** The converter evaluates the
   skeleton once (bind pose first; optionally a BCK-sampled pose later) and bakes
   skinned vertices. The intermediate format still carries joints/weights for
   future animation work. No runtime skinning in v1.

## Verified facts

**tww side** (all verified against the working tree):

- Disc image: `../tww/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA,
  Canada).ciso` (1.1 GiB). Only the USA region is populated.
- `../tww/build/tools/dtk` is a working built binary; `dtk vfs ls/cp` descends
  CISO→Yaz0→RARC with `:`-separated nested paths. Confirmed locations:
  - `Link.arc` at `/files/res/Object/Link.arc` — contains `bdl/cl.bdl` (356 KiB,
    the skinned body) and `tex/linktexbci4.bti` (7.5 KiB, casual-clothes body
    texture).
  - `System.arc` at `/files/res/Object/System.arc` — contains `dat/toon.bti`
    (1 KiB) and `dat/toonex.bti` (32 KiB), the shared cel-shading ramps.
- BDL files contain the **full data-driven MAT3 material chunk** even though they
  also carry MDL3 precompiled display lists; `J3DModelLoader::loadBinaryDisplayList`
  reads MAT3 regardless. **MDL3 can be ignored entirely** — parse MAT3 and
  synthesize our own state/shaders from it.
- Chunks in `cl.bdl`: INF1 (scene graph/draw order), VTX1 (vertex attribute
  arrays), EVP1 (multi-weight skinning envelopes + inverse bind matrices), DRW1
  (rigid-vs-skinned matrix slots), JNT1 (42 joints, `LINK_ROOT`..`CL_BACK`), SHP1
  (shapes; GX triangle strips/fans + per-packet matrix tables), MAT3 (TEV stages,
  texgens, konst/register colors, blend/Z/cull, texture indices), TEX1 (embedded
  textures in GX formats).
- Toon shading: materials sample the toon ramps via a texgen derived from
  normal·light; the ramps are injected **by texture name** at runtime
  (`setToonTex` in `../tww/src/d/d_resorce.cpp`). Light/ambient colors flow into
  TEV registers C0/K0/K1 per frame from `dKy_tevstr_c`
  (`../tww/include/d/d_kankyo.h`). For a static render we hardcode a daytime
  context.
- Useful references: `../tww/src/JSystem/J3DGraphLoader/J3DModelLoader.cpp` /
  `J3DShapeFactory.cpp` / `J3DMaterialFactory.cpp` (chunk offsets/semantics),
  `../tww/tools/converters/matDL_dis.py` (GX register meanings),
  noclip.website's J3D renderer (TypeScript; the proven J3D→modern-GPU port).

**Renderer side** (line numbers as of `bef946e`):

- One pipeline + one draw call per frame: `FrameRenderer` (src/renderer.rs:5042)
  consumes itself in `draw_indexed`/`draw_vertex_count`; the bind+draw block in
  `record_command_buffer` is src/renderer.rs:1546–1617. A queue pattern already
  exists for compute (`pending_compute: Vec<PendingComputeCommand>`).
- Hardcoded pipeline state in `create_graphics_pipeline` (src/renderer.rs:3305):
  blend always SRC_ALPHA/ONE_MINUS_SRC_ALPHA ADD (3366), color write mask RGBA
  (3374), cull BACK + CCW (3354–3357), depth LESS + write always on (3384),
  stencil off. `PipelineConfig` (src/renderer/pipeline.rs:139) exposes only
  `disable_depth_test`.
- Textures: always `R8G8B8A8_SRGB`, full mip chain, REPEAT wrap, anisotropy on
  (src/renderer.rs:3966/4022/4347). Only filter (Linear/Nearest) is selectable.
- Index buffers are u32-only; vertex/index buffers are owned per-pipeline.
- Per-pipeline descriptor sets and uniform buffers are allocated at
  `create_pipeline` (2 frames in flight); `Gpu::write_uniform` writes mapped
  memory per frame. So N materials = N pipelines, each with its own uniforms and
  textures — no dynamic offsets needed.
- Shader flow: `shaders/source/<name>.shader.slang` (exactly 1 vert + 1 frag
  entry) → `just shaders` → SPIR-V + reflection JSON + generated Rust bindings in
  `src/generated/shader_atlas/`. Multiple `Sampler2D`s per parameter block proven
  (paint_display uses 5). `StructuredBuffer` readable from vertex shaders
  (sprite_batch). Codegen covered by insta snapshot tests (`just test`).

## 1. Architecture overview

```
just extract-link                 just convert-link                just shaders (unchanged)
──────────────────►  assets/link/raw/  ──────────────►  assets/link/converted/
  scripts/extract_link.sh              src/bin/convert_link/         link.manifest.json
  (dtk vfs cp from ../tww ciso)        (BDL parse, GX tex decode,    tex/*.png
                                        pose bake, TEV subset gate)  link.{vtx,idx,skin}.bin
                                                                     mat3_dump.txt
                                                       │
                                                       ▼
                    examples/toon_link.rs  ←──  shaders/source/toon_link.shader.slang
                    (one pipeline per material,      + shaders/source/tev.slang
                     shared mesh, queued draws)      (hand-written TEV interpreter)
```

Key property: **all committed code is hand-written**. Every byte of
Nintendo-derived data (geometry, textures, TEV stage configs, colors) lives in
the gitignored `assets/link/` tree and flows into the shader via uniforms at
runtime. This also avoids any chicken-and-egg between converter output and the
committed `src/generated/` shader atlas — `just shaders` and its snapshots are
untouched by asset conversion.

## 2. Asset extraction & conversion

### 2.1 Extraction — `scripts/extract_link.sh`, `just extract-link`

```sh
DISC="../tww/orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA, Canada).ciso"
DTK=../tww/build/tools/dtk
mkdir -p assets/link/raw
"$DTK" vfs cp "$DISC:/files/res/Object/Link.arc:bdl/cl.bdl"          assets/link/raw/cl.bdl
"$DTK" vfs cp "$DISC:/files/res/Object/System.arc:dat/toon.bti"      assets/link/raw/toon.bti
"$DTK" vfs cp "$DISC:/files/res/Object/System.arc:dat/toonex.bti"    assets/link/raw/toonex.bti
"$DTK" vfs cp "$DISC:/files/res/Object/Link.arc:tex/linktexbci4.bti" assets/link/raw/linktexbci4.bti
```

- Add `/assets/` to `.gitignore`.
- Verify by size: `cl.bdl` ≈ 356 KiB, `toon.bti` ≈ 1 KiB, `toonex.bti` ≈ 32 KiB.

### 2.2 Converter — `src/bin/convert_link/`

A directory binary in the existing crate (precedent:
`src/bin/generate_paper_texture.rs`), invoked as
`just convert-link` → `cargo run --bin convert_link -- assets/link/raw assets/link/converted [--dump-mat3] [--obj out.obj] [--casual]`.

Serde types for the manifest go in a new lib module **`src/model_manifest.rs`**
(registered in `src/lib.rs`) so the converter and the example share them. Parser
modules stay inside the bin:

```
src/bin/convert_link/
  main.rs        CLI + orchestration
  be.rs          hand-rolled big-endian reader (~60 lines: u8/u16/i16/u32/f32/str/seek); no new deps
  bmd/mod.rs     header walk ("J3D2bdl4" magic, chunk table), dispatch; skip MDL3
  bmd/inf1.rs    scene graph tag stream → joint hierarchy + material/shape draw order
  bmd/vtx1.rs    attribute format table (comp count/type, fixed-point frac shift) + arrays
  bmd/evp1.rs    envelopes: joint indices, weights, inverse bind matrices (4x3 f32)
  bmd/drw1.rs    matrix slots: isWeighted flags + joint-or-envelope indices
  bmd/jnt1.rs    joints: scale f32x3, rotation s16x3 (0x8000 = π), translation f32x3, names
  bmd/shp1.rs    shapes: attr sets, per-packet matrix tables, GX display-list primitive decode
  bmd/mat3.rs    materials: TEV/blend/zmode/channels/texgens/konst via MAT3's index tables
  bmd/tex1.rs    embedded BTI headers + image data
  bti.rs         standalone .bti (same header layout as TEX1 entries)
  gx/types.rs    typed enums for every GX field we read (u8 → enum, validated)
  gx/texture.rs  decoders: CMPR, I4, I8, IA4, IA8, RGB565, RGB5A3, RGBA8, C4/C8+palette
  pose.rs        JNT1 world matrices, EVP1/DRW1 CPU skinning, strip/fan → triangle list
  tev_ir.rs      MAT3 → validated TevMaterialDesc (the subset gate)
  output.rs      manifest JSON + PNGs + flat binaries + mat3_dump.txt + --obj debug export
```

**GX texture decode.** GX stores textures in tiles (CMPR: 8×8 tiles of four 4×4
DXT1-like sub-blocks with big-endian u16 colors; I4/I8/etc. have their own tile
dims) — the tiling is the classic bug source. Decode everything to RGBA8
`image::RgbaImage` → PNG. Likely set for Link: CMPR (clothes/body), C8/RGB5A3
(face/eyes), I4/I8 (ramps), IA8; implement the full list anyway, each is small.

**Toon-ramp injection** happens in the converter: TEX1 entries whose *name*
matches the runtime-injected ramps (expected `ZAtoon`/`ZBtoonEX` — confirm exact
strings from the dump) get their image replaced by decoded
`toon.bti`/`toonex.bti`. `--casual` swaps the body texture for `linktexbci4` the
same way.

**Pose baking.** `world(j) = world(parent(j)) · T·R·S` from JNT1 (s16 angle →
radians via `a / 32768.0 * π`). For each SHP1 packet, resolve its matrix table
through DRW1: rigid slot → joint world matrix; weighted slot →
`Σ wᵢ · (worldᵢ · invBindᵢ)`. Transform positions (normals via
inverse-transpose, renormalize). Strips → lists (odd triangles swap first two
indices), fans → `(0, i, i+1)`. Keep Y-up right-handed as-is — same convention
as the viking_room OBJ; the shader's projection handles Vulkan clip space.

*Sanity anchor:* in J3D, EVP1-weighted vertices are stored in **model space**
(at bind pose `Σw·(world·invBind) = I`, so baking ≈ identity), while rigid
DRW1-bound vertices are stored **joint-local** and must be moved by the joint's
world matrix. The converter asserts the weighted-identity property; if rigid
parts (hair, belt, scabbard) render detached, the JNT1 walk is wrong — not the
skinning.

Also emitted for the future (unused in v1): `link.skin.bin` (per-vertex 4 ×
(joint u8, weight f32)) and the skeleton in the manifest.

### 2.3 Intermediate format — `assets/link/converted/`

Everything human-inspectable: JSON manifest + PNGs + flat binaries + a
pretty-printed `mat3_dump.txt` (stage equations rendered as
`C = (d op ((1-c)·a + c·b) + bias) · scale` text, in the spirit of
`matDL_dis.py`).

`link.manifest.json` (types in `src/model_manifest.rs`):

```jsonc
{
  "version": 1,
  "buffers": { "vertices": "link.vtx.bin", "indices": "link.idx.bin",
               "skinning": "link.skin.bin", "vertex_count": 0, "index_count": 0 },
  "textures": [
    { "name": "linktexS3TC", "file": "tex/03_linktexS3TC.png",
      "wrap_u": "Repeat", "wrap_v": "Repeat", "filter": "Linear", "mipmaps": true }
  ],
  "materials": [
    { "name": "eyeLdamA",
      "cull": "Back", "blend": "Opaque", "depth_test": "LessEqual", "depth_write": true,
      "alpha_compare": { "comp0": "GEqual", "ref0": 128, "op": "And",
                         "comp1": "LEqual", "ref1": 255 },
      "tev": { "num_stages": 2,
               "stages": [ /* packed color+alpha selector/op/bias/scale/clamp/dest */ ],
               "konst": [[0,0,0,0], /* ×4 */], "regs": [[0,0,0,0], /* ×4 */],
               "orders": [ { "texcoord": 0, "texmap": 0, "channel": "Color0A0" } ],
               "ksel": [ /* konst selects per stage */ ] },
      "channel": { "lighting": true, "amb": [0,0,0,0], "mat": [0,0,0,0] },
      "texgens": [ { "ty": "Mtx2x4", "src": "Tex0" }, { "ty": "SRTG", "src": "Color0" } ],
      "texmaps": [3, 0, null, null] /* indices into textures[]; ≤4 used per material */ }
  ],
  "batches": [ { "material": 5, "first_index": 0, "index_count": 4200 } ],
  "skeleton": { "joints": [ { "name": "LINK_ROOT", "parent": -1,
                              "t": [0,0,0], "r": [0,0,0], "s": [1,1,1] } ] }
}
```

- `link.vtx.bin`: little-endian interleaved f32 — pos[3], nrm[3], color0[4],
  uv0[2], uv1[2] (14 floats/vertex).
- `link.idx.bin`: u32 triangle list.
- `batches` are already in J3D draw order (opaque pass then translucent pass),
  so the example just draws them in sequence.

## 3. TEV subset shader

Two committed, hand-written files:

- **`shaders/source/tev.slang`** — module with: packed param structs, selector
  evaluation (switch over enum values), the ≤8-stage loop computing
  `out = clamp?((d op lerp(a,b,c)) + bias) · scale` for color and alpha, a GX
  lighting channel (one directional light, `clamp(dot(N, -L), 0, 1)`), texgen
  evaluation including **SRTG** (texcoord from the rasterized channel color —
  this is how the ramp is indexed), alpha-compare → `discard`, and a final
  inverse-sRGB output helper (see color space note).
- **`shaders/source/toon_link.shader.slang`** — vertex struct
  `{ float3 position; float3 normal; float4 color0; float2 uv0; float2 uv1; }`,
  `ParameterBlock` with one `ToonLinkParams` uniform + 4 × `Sampler2D`
  (tex0..tex3; unused slots bound to a 1×1 white dummy).

The shader is a **data-driven interpreter**: per-material TEV configuration
arrives as uniform data built from the manifest. Uniform layout uses **flat
`uint4`/`float4` arrays only** (codegen support for arrays-of-structs is
unverified — see risks; fallback is a `StructuredBuffer`, proven via
sprite_batch):

```slang
struct ToonLinkParams {
    MVPMatrices mvp;
    float4 konst[4];
    float4 reg[4];           // C0 ← light color, K0/K1 ← ambient (setLightTevColorType semantics)
    float4 lightDir;         // world space
    float4 lightColor;
    float4 chanAmbMat;       // channel ambient/material colors
    uint4  stageColor[8];    // a, b, c, d input selectors
    uint4  stageColorOp[8];  // op, bias, scale, dest (+ clamp bit)
    uint4  stageAlpha[8];
    uint4  stageAlphaOp[8];
    uint4  stageOrder[8];    // texcoord, texmap, ras channel, kcsel/kasel
    uint4  texgen[8];        // type, src
    uint4  control;          // numStages, numTexgens, packed alpha-compare, flags
}
```

**The subset gate** lives in the converter's `tev_ir.rs`: a fully typed IR where
every selector/op/texgen value Link's materials may use is an enum variant, and
anything outside the implemented set is a hard error
(`material {name}: unsupported {feature} — extend tev.slang + tev_ir.rs`).
Expected subset (frozen from the P2 dump): ≤4 stages/material; color inputs
CPREV/C0/KONST/TEXC/RASC/ZERO/ONE/HALF; ADD/SUB with bias/scale; SRTG +
identity-MTX2x4 texgens; blend opaque/alpha; Z LEQUAL; alpha-compare on eye
materials. Explicitly out of scope (error if encountered): indirect stages,
non-identity texture matrices. Fog: if enabled in MAT3, warn and hardcode off
(we bake a no-fog daytime context).

**Color space.** GX has no sRGB anywhere — TEV math operates on raw 8-bit
values, and the ramps' banding is authored in those raw values. So: decode and
sample **all** Link textures as `UNORM` (not sRGB), do TEV math on raw values,
and apply the inverse sRGB transfer on the final fragment color so the sRGB
render target round-trips back to the raw value (matching Dolphin's output
path). This is why texture-format options (§4.4) are a hard requirement, not a
nicety.

## 4. Renderer extensions

Four additive changes, each independently landable, ordered below. All 10
existing examples keep compiling and running; no codegen/template changes, so
`just test` snapshots stay untouched. Defaults reproduce today's behavior
exactly.

### 4.1 Multi-draw queue (src/renderer.rs)

```rust
enum PendingDrawCommand {                       // beside PendingComputeCommand (~5027)
    Draw { pipeline_index: usize, draw_call: DrawCallConfig },
}

enum DrawCallConfig {
    VertexCount(u32),
    IndexCount(u32),                                   // existing: whole buffer
    IndexRange { first_index: u32, index_count: u32 }, // new
}

impl<'f> FrameRenderer<'f> {
    pub fn queue_draw_indexed(&mut self, pipeline: &PipelineHandle<DrawIndexed>);
    pub fn queue_draw_index_range(&mut self, pipeline: &PipelineHandle<DrawIndexed>,
                                  first_index: u32, index_count: u32);
    pub fn queue_draw_vertex_count(&mut self, pipeline: &PipelineHandle<DrawVertexCount>,
                                   vertex_count: u32);
    pub fn submit_draws(self, gpu_update: impl FnOnce(&mut Gpu)) -> Result<(), DrawError>;
}
```

- Type erasure is free: store `pipeline.index()` (usize), exactly like compute
  dispatch already does.
- `record_command_buffer` (src/renderer.rs:1358): the bind+draw block
  (1546–1617) becomes a loop over the queued commands **inside the single
  existing render pass** — bind pipeline → bind that pipeline's vertex/index
  buffers → bind its descriptor sets → `cmd_draw`/`cmd_draw_indexed(count, 1,
  first_index, 0, 0)`. Skip re-binding buffers when consecutive draws share
  them. Render pass begin/end, MSAA resolve, blit, sync all untouched.
- Existing `draw_indexed`/`draw_vertex_count`/`draw_vertex_count_with_picking`
  become one-element-queue wrappers — no example changes.
- Hot reload (`check_for_shader_recompile`, ~2284): refactor to take pipeline
  indices and iterate the deduped queued set, mirroring the compute path.

### 4.2 Shared meshes

```rust
pub struct MeshHandle { index: usize }

impl Renderer {
    pub fn create_mesh<V: VertexDescription + GPUWrite>(
        &mut self, vertices: &[V], indices: &[u32]) -> anyhow::Result<MeshHandle>;
}

// src/renderer/pipeline.rs:114
pub(super) enum VertexPipelineConfig {
    VertexAndIndexBuffers(VertexAndIndexBuffers),
    SharedMesh(usize),      // index into Renderer::meshes
    VertexCount,
}

impl<'t, V: VertexDescription, D: DrawCall> PipelineConfig<'t, V, D> {
    pub fn with_shared_mesh(mut self, mesh: &MeshHandle) -> Self;  // replaces vertex_config
}
```

- `Renderer` gains `meshes: Vec<VertexAndIndexBuffers>`, destroyed at renderer
  teardown. `destroy_pipeline` must not free shared buffers.
- Generated `pipeline_config(resources)` is untouched: the example passes empty
  `vertices`/`indices` in `Resources`, then calls `.with_shared_mesh(&mesh)`.
  Document this pattern in the example.

### 4.3 Raster state

```rust
#[derive(Clone, Copy)]
pub struct RasterState {
    pub blend: BlendMode,         // Alpha (today's behavior) | Opaque
    pub cull: CullMode,           // Back (default) | None | Front
    pub depth_test: DepthCompare, // Less (default) | LessEqual | Always | Disabled
    pub depth_write: bool,        // true
    pub color_write: [bool; 4],   // all true; exercised later by the eye trick
}
impl Default for RasterState { /* == today's hardcoded pipeline state */ }

impl PipelineConfig<…> { pub fn with_raster_state(mut self, s: RasterState) -> Self; }
```

- `create_graphics_pipeline` (3305) consumes `&RasterState` instead of scattered
  hardcoded values; the picking pipeline passes its own fixed state.
- `RendererPipeline.disable_depth_test` (pipeline.rs:111) is replaced by
  `raster_state: RasterState` (hot-reload recreate path reads it). The
  `disable_depth_test` builder field stays and maps to `depth_test: Disabled`.

### 4.4 Texture options

```rust
pub enum TextureWrap { Repeat, ClampToEdge, MirroredRepeat }
pub enum TextureColorSpace { Srgb, Unorm }
pub struct TextureOptions {
    pub filter: TextureFilter,
    pub wrap_u: TextureWrap, pub wrap_v: TextureWrap,
    pub mipmaps: bool,
    pub color_space: TextureColorSpace,
}

impl Renderer {
    pub fn create_texture_with_options(&mut self, name: impl Into<String>,
        image: &image::DynamicImage, options: TextureOptions) -> TextureHandle;
}
```

- Plumbs through `create_texture_image`/`create_texture_sampler`
  (3966/4008/4347): `R8G8B8A8_{SRGB|UNORM}`, `mip_levels = 1` + skip
  `generate_mipmaps` when off, sampler address modes from wraps, anisotropy off
  when mipmaps are off. Existing `create_texture` becomes a wrapper with
  today's defaults.
- Ramp textures use: `Unorm`, `ClampToEdge`, `mipmaps: false`, `Linear` filter.

### 4.5 Explicitly deferred

Eye write-mask multi-pass (needs `color_write` exercised + per-pass shape-group
toggling), destination-alpha tricks, stencil, u16 index buffers, BTP/BTK eye
animation, runtime skinning.

## 5. The example — `examples/toon_link.rs`

- **setup**: read `assets/link/converted/link.manifest.json` via
  `CARGO_MANIFEST_DIR`; on missing file,
  `bail!("run `just extract-link && just convert-link` first")`. Load PNGs with
  `create_texture_with_options` (all `Unorm`; ramps clamp/no-mips). Create the
  1×1 white dummy texture. Load `link.vtx.bin`/`link.idx.bin` → `Vec<Vertex>`
  (generated type) → one `create_mesh`. Per material:
  `create_uniform_buffer::<ToonLinkParams>()`, build
  `pipeline_config(resources).with_shared_mesh(&mesh).with_raster_state(from_material(m))`,
  `create_pipeline`. Store `Vec<(PipelineHandle<DrawIndexed>,
  UniformBufferHandle<ToonLinkParams>, ToonLinkParams)>` + the batch list, and
  precompute the static parts of each `ToonLinkParams` from the manifest's TEV
  data.
- **draw**: fixed or slow-orbit camera (`Mat4::look_at_rh` +
  `perspective_rh`, as in viking_room; Link is ~100 GC units tall — scale the
  model matrix ~0.01 or frame accordingly). Hardcoded daytime light: directional
  from up-forward-left, `lightColor ≈ (1.0, 0.98, 0.92)`, ambient ≈
  `(0.45, 0.5, 0.55)` — seeds, tuned visually against noclip, then replaced
  with exact values read from `dKy_tevstr_c` in emulated RAM
  (dolphin-memory-engine + tww symbols, see tests.md); fed into
  `reg[0]`/konst slots the way `setLightTevColorType` does (C0 = light color,
  K0/K1 = ambient; see `../tww/src/d/d_kankyo.cpp`). Then queue one
  `queue_draw_index_range` per batch in manifest order and finish with
  `submit_draws(|gpu| { /* write all per-material uniforms */ })`.

## 6. Phases & verification

Converter (P1–P3) and renderer (P4–P5) tracks are independent and can be
interleaved. Each phase is separately verifiable — full detail on the oracles
(GCFT/gclib, SuperBMD, noclip, Dolphin) and our own tests per phase in
[`link_rendering/tests.md`](link_rendering/tests.md).

| Phase | Deliverable | Verify | Est. |
|---|---|---|---|
| **P0** | `scripts/extract_link.sh`, `just extract-link`, `.gitignore` entry — detailed plan: [`link_rendering/phase_00.md`](link_rendering/phase_00.md) | sizes + SHA256s match `dtk vfs ls` (golden hashes, permanently stable); `J3D2bdl4` magic; idempotent; `git status` clean | ½ day |
| **P1** | converter skeleton: `be.rs`, chunk walk, `--info` chunk table — detailed plan: [`link_rendering/phase_01.md`](link_rendering/phase_01.md) | internal invariants (chunk sizes sum to file size, 42 joints, cross-chunk counts agree); `--info` diffed against a gclib script; `BeReader` unit tests on synthetic buffers | 1 day |
| **P2** | TEX1+BTI decode → PNGs (+ standalone `.bti` re-emit per entry); `--dump-mat3` report — detailed plan: [`link_rendering/phase_02.md`](link_rendering/phase_02.md) | **`just link-verify-textures`**: GCFT pixel-diff over every texture = zero differences; SuperBMD materials-JSON field diff vs `--dump-mat3`; synthetic per-format tile snapshots (insta); ramp texture names confirmed; **freeze the TEV subset from this dump** | 2–3 days |
| **P3** | geometry: baked bind pose, strip→list, manifest v1, `--obj` export | **`just link-verify-geometry`**: weighted-identity check (hard error); 42 joint world positions vs SuperBMD armature; exact triangle counts + per-material AABBs vs SuperBMD DAE; `--obj` overlaid on the DAE in Blender | 3–4 days |
| **P4** | renderer 4.1 + 4.2 (multi-draw, index ranges, shared mesh) + committed `examples/multi_mesh.rs` (multiple pipelines, one shared mesh, disjoint index sub-ranges) | `just test` green (snapshots byte-identical); validation-clean sweep of **all** examples (`timeout 3 just dev <name>` loop); multi_mesh renders its sub-ranges with no gaps/overlaps | 2 days |
| **P5** | renderer 4.3 + 4.4 (raster state, texture options); extend multi_mesh with per-state test objects | multi_mesh: cull-front object inside-out, opaque-vs-alpha blend, depth-write-off artifact on demand; wrap/filter quad (clamp/repeat × linear/nearest); sRGB-vs-UNORM gray-quad brightness check; same validation sweep | 1–2 days |
| **P6** | `toon_link.shader.slang` v0 (normals-as-color debug frag) + example loads manifest, draws all batches | **uniform-array smoke test first** (`uint4[8]`, known pattern as colors); `just shaders`; `timeout 3 just dev toon_link`: correctly shaped Link, smooth normal gradients, silhouette vs noclip; culling off → then on (winding check), no validation errors | 1–2 days |
| **P7** | albedo-only: real textures, tex0 sample, alpha-compare discard, per-material raster state | UV features vs noclip (face decals, eyes, belt buckle, tunic pattern); clean alpha-cutout edges on brows/lashes; no missing parts from per-material cull | 1 day |
| **P8** | full TEV interpreter + lighting channel + SRTG ramp + gamma handling; subset gate final; single-material isolation debug key in the example | structured side-by-side vs noclip + golden Dolphin frames (`just link-dolphin-refs`, headless `.dff` replay) per feature (skin, tunic bands, hair highlight, eye whites); rotate light — terminator bands sweep and stay banded; isolate batch N for any wrong material; TEV semantic disputes adjudicated via FIFO analyzer (runtime BP/XF state) + software-renderer replay; optional CPU TEV reference evaluator if pixel-chasing gets hard | 3–5 days |
| **P9** | optional polish: `--casual` clothes; eye write-mask multi-pass; BCK-sampled pose | casual: P7-style UV checks; eye trick vs **Dolphin** (noclip may not implement it) | 2+ days |

Rough total: ~3 weeks of focused work. Once a converter phase's output is
verified, commit SHA256 golden hashes of `assets/link/converted/` outputs
(hashes of derived data, not the data) so later refactors get free regression
detection. Dev-only oracle dependencies, not needed to build or run: GCFT/gclib
(Python) and SuperBMD (mono).

## 7. Risks & unknowns

Expanded walkthrough of each risk (mechanism, failure mode, why the mitigation
works): [`link_rendering/risks.md`](link_rendering/risks.md).

1. **SHP1 matrix groups** — per-packet matrix tables with `0xFFFF` "inherit from
   previous packet" entries and the per-vertex `PNMTXIDX` attribute (value/3 =
   table slot). Getting this wrong = exploded vertices. Mitigation:
   `J3DShapeFactory.cpp` + noclip as dual references; the weighted-identity
   check.
2. **GX fixed-point vertex formats** — s16/u8 components with per-attribute
   fraction shifts from VTX1's format table; don't assume f32 positions.
   Mitigation: implement the format table generally; log formats in `--info`.
3. **Winding after Y-flip** — clip-space Y reflection flips winding vs GX's
   convention. Mitigation: P6 runs with `cull: None`; once geometry is right,
   enable Back and flip triangle order in the converter if inside-out.
4. **Uniform array codegen** — unverified that the shader atlas codegen handles
   `uint4 foo[8]` uniform arrays. Mitigation: flat arrays only; smoke-test with
   a throwaway shader early in P6; fallback to `StructuredBuffer`
   (sprite_batch-proven).
5. **SRTG texgen details** — exact semantics and which channel feeds it; whether
   Link uses non-identity texture matrices anywhere. The P2 dump decides;
   noclip's `gx_material.ts` as reference.
6. **S10 register semantics** — TEV intermediates are signed 10-bit, clamped
   per-stage only when the clamp bit is set. Respect the clamp bit always; add
   explicit range clamps only if banding artifacts appear.
7. **Fog** — if MAT3 enables it on Link, warn and hardcode off.
8. **Lighting values** — exact daytime `dKy_tevstr_c` values are buried in
   kankyo tables; v1 starts from hand-tuned seeds, then upgrades to ground
   truth by reading the live values from emulated RAM (dolphin-memory-engine
   + tww decomp symbol addresses; see tests.md §Dolphin).
