# Rendering Toon Link: implementation plan

Goal: a new example (`examples/toon_link.rs`) that renders a **static, cel-shaded
Toon Link** from *The Legend of Zelda: The Wind Waker*, using assets extracted from
the decompilation project at `../tww`. Companion to
[`../tww/docs/link-rendering.md`](../../tww/docs/link-rendering.md), which maps the
original game's code and asset paths; this document plans the port into this
renderer.

Companion documents: [`link_rendering/risks.md`](link_rendering/risks.md)
(expanded risk walkthrough),
[`link_rendering/tests.md`](link_rendering/tests.md) (per-phase correctness
testing: external oracles + our own tests), and
[`link_rendering/follow_up.md`](link_rendering/follow_up.md) (deferred work
with no landing phase, accepted limitations, verification debt).

## Settled decisions

1. **TEV translation — hybrid "subset compiler."** Dump `cl.bdl`'s MAT3 chunk,
   then mechanically implement exactly the TEV feature subset Link's materials
   use (correct stage equations, texgens, register semantics), structured for later
   extension. Not a full generic GX→shader translator (noclip.website-scale
   project), not hand-eyeballed shaders (subtly wrong colors, Link-only).
   *(P2 measured the actual surface: 24 material slots sharing 11 distinct
   configs, and the frozen subset is small — see §3.)*
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
  (rigid-vs-skinned matrix slots), JNT1 (42 joints, `link_root`..`cl_back` —
  lowercase), SHP1 (shapes; GX triangle strips + per-packet matrix tables —
  probing found strips only, no fans), MAT3 (TEV stages, texgens,
  konst/register colors, blend/Z/cull, texture indices), TEX1 (41 embedded
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

**Converter side** (established by P1/P2/P3 implementation; authoritative
detail in the phase docs and their Recorded facts):

- P0–P3 are **done and committed** (`a76d0cb`, `6431f0a`, `8a0a4af`,
  `7704292`), each with its verify gate green.
  Each phase is behind a byte-exact gate against an independent gclib-based
  oracle (`just link-verify-p1/p2/geometry`): P1 chunk table, P2 44/44
  pixel-identical textures + a zero-diff canonical MAT3 table, P3 zero-diff
  canonical geometry table + the file's own invBind/weighted-identity checks.
  Oracle scripts are pinned to gclib 1.0.0 @ `6412774`; golden output hashes
  live in `scripts/link_converted.sha256`.
- Texture inventory: CMPR ×14, I4 ×11, IA8 ×8, IA4 ×7, C8+RGB565 ×1
  (`hitomi`). **Every** texture (TEX1 and standalone) is Clamp/Clamp,
  Linear/Linear, mipmap-free — §4.4's options apply to all of them, not just
  ramps. Names repeat; converted files are index-prefixed.
- Ramp injection rule confirmed (`d_resorce.cpp:70–82`): name prefix `ZA*` →
  toon, `ZB*` → toonEX. `cl.bdl` contains only `ZBtoonEX` — **`toon.bti` is
  unused by Link's body model.**
- MAT3: 24 slots → 11 distinct records via the remap table (the name table
  literally contains `ear(2)`..`ear(8)`; `face` shares record 0 with `ear`,
  R-side eye/brow slots share L-side records). The frozen TEV subset is in
  phase_02.md's Recorded facts; headline in §3 below.
- Geometry (P3, implemented): positions/normals f32, UVs s16 shift-8 — the only
  fixed-point attribute; **no vertex colors, no second UV**; 573 primitives,
  all triangle strips → exactly 2,874 triangles; no billboard shapes; every
  joint scale is exactly 1.0 (scaling-rule semantics moot); EVP1 stores all
  42 inverse bind matrices, so the file is its own FK oracle — invBind identity
  passed (max residual 0.0145, f32 precision) and weighted identity passed (max
  0.0077 model units). Baked output: **1754 deduped vertices, 8622 indices,
  2874 triangles, 24 batches** (INF1 draw order). Rotation composition confirmed
  Z·Y·X. `INF1.vertexCount` (1591) = the position-array count.
- Oracle re-weighting vs tests.md: gclib (scriptable, pinnable) is the
  automated gate everywhere; SuperBMD is a manual second opinion only
  (Blender DAE overlay), never load-bearing.

**Renderer side** (line numbers as of `4621112`, i.e. **post-P4**;
post-BDA-migration — descriptor-based storage buffers no longer exist):

- **N draws per frame, one render pass** (P4): `FrameRenderer`
  (src/renderer.rs:5297) accumulates `PendingDrawCommand`s via
  `queue_draw_indexed` (5388) / `queue_draw_index_range` (5397) /
  `queue_draw_vertex_count` (5426) and is consumed by the terminal
  `submit_draws` (5438). The record loop is src/renderer.rs:1765, inside the
  single `cmd_begin_rendering`/`cmd_end_rendering` of `record_command_buffer`
  (1458). `draw_indexed`/`draw_vertex_count` (5442/5451) survive as
  one-element-queue wrappers with append semantics; picking
  (`draw_vertex_count_with_picking`, 5461) stays single-draw and
  `debug_assert`s an empty queue (§4.5). Compute has the same shape
  (`pending_compute: Vec<PendingComputeCommand>`).
- **Shared meshes** (P4): `Renderer::create_mesh` (981) →
  `MeshHandle<V>` (pipeline.rs:192) → `PipelineConfig::with_shared_mesh`
  (pipeline.rs:236); the buffers live in `Renderer::meshes` and outlive the
  pipelines that draw them (freed at teardown, not by `destroy_pipeline`).
  `examples/multi_mesh.rs` is the worked example of the exact shape P6 needs.
- **Per-pipeline raster state** (P5): every graphics pipeline carries a
  `RasterState` (blend / cull / depth compare / depth write / color write mask),
  set with `PipelineConfig::with_raster_state` and defaulting to the pre-P5
  hardcoded state — see §4.3. Topology, polygon mode, front face (always CCW),
  depth bias and stencil are still hardcoded in `create_graphics_pipeline`
  (src/renderer.rs:3516).
- **Per-texture options** (P5): `Renderer::create_texture_with_options` takes a
  `TextureOptions` (filter / wrap u+v / mipmaps / color space); plain
  `create_texture` is a wrapper preserving the old sRGB + full-mip-chain +
  REPEAT + anisotropy defaults — see §4.4. So Link's `Unorm` + `ClampToEdge` +
  `mipmaps: false` + `Linear` requirement is now expressible.
- Index buffers are u32-only; vertex/index buffers are owned per-pipeline
  unless the pipeline points at a shared mesh.
- Per-pipeline descriptor sets and uniform buffers are allocated at
  `create_pipeline` (a 2-slot ring, `MAX_FRAMES_IN_FLIGHT`; this was a 3-slot
  `PRE_WAIT_RING_LEN` when written, collapsed 2026-07-28 by
  `remove_pipelined_compute.md`); `Gpu::write_uniform` writes mapped memory per
  frame. So N materials = N pipelines, each with its own uniforms and
  textures — no dynamic offsets needed. Corollary proven by multi_mesh:
  **uniforms are per-pipeline, not per-draw**, so every draw sharing a pipeline
  shares its uniform values (fine for Link — one transform, N materials).
- Hot reload (`check_for_shader_recompile`, 2530; index-based since P4, so it
  recreates every queued graphics pipeline, not just one):
  `assert_shader_interface_unchanged` (added `6a8552f`) panics if a reloaded
  shader's reflected interface differs from the build-time-embedded reflection.
  Shader-body edits (TEV math) still hot-reload; changing `ToonLinkParams`'
  shape requires `just shaders` + restart. (This was always unsafe — the panic
  replaces silent GPU-data corruption, not a previously working path.)
- Shader flow: `shaders/source/<name>.shader.slang` (exactly 1 vert + 1 frag
  entry) → `just shaders` → SPIR-V + reflection JSON + generated Rust bindings in
  `src/generated/shader_atlas/`. Multiple `Sampler2D`s per parameter block proven
  (paint_display uses 5). Buffers readable from vertex shaders via BDA pointers:
  sprite_batch declares `ImmutableAddr<Sprite>` in its param block, created with
  `Renderer::create_immutable_buffer` and written via `write_immutable`
  (`StructuredBuffer` was removed in the BDA migration). Codegen covered by
  insta snapshot tests (`just test`).

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
`just convert-link` → `cargo run --bin convert_link -- assets/link/raw assets/link/converted [FLAG]`.
Flags as implemented/planned: `--info` (P1 chunk table), `--dump-mat3` (P2),
`--dump-geometry` and `--obj` (P3), `--casual` (P9) — the `--dump-*` modes
print canonical tables to stdout for the oracle diff gates and emit nothing.

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
  bmd/evp1.rs    envelopes: joint indices, weights, inverse bind matrices (3×4 f32)
  bmd/drw1.rs    matrix slots: isWeighted flags + joint-or-envelope indices
  bmd/jnt1.rs    joints: scale f32x3, rotation s16x3 (0x8000 = π), translation f32x3, names
  bmd/shp1.rs    shapes: attr sets, per-packet matrix tables, GX display-list primitive decode
  bmd/mat3.rs    materials: TEV/blend/zmode/channels/texgens/konst via MAT3's index tables
  bmd/mat3_dump.rs canonical --dump-mat3 table + mat3_dump.txt human report
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
`image::RgbaImage` → PNG. *(P2 shipped all 11 formats; the actual Link set is
CMPR/I4/IA8/IA4/C8 plus C4 for the casual texture, verified pixel-identical
against gclib — 44/44.)*

**Toon-ramp injection** happens in the converter: TEX1 entries whose *name*
matches the runtime-injected ramps get their image replaced. Confirmed
(`d_resorce.cpp:70–82`): the game matches name *prefixes* — `ZA*` → toon
image, `ZB*` → toonEX image; `cl.bdl` has exactly one such entry,
`ZBtoonEX` (an 8×8 I4 placeholder), which receives decoded `toonex.bti`
(CMPR 256×256). No `ZA*` entry exists, so `toon.bti` is decoded/verified but
unused. `--casual` swaps the body texture for `linktexbci4` the same way
(P9). The substitution is wired when the manifest is assembled (P3);
P2 verified both sides of it decode correctly.

**Pose baking.** `world(j) = world(parent(j)) · T·R·S` from JNT1 (s16 angle →
radians via `a / 32768.0 * π`); every scale in cl.bdl is exactly 1.0, so the
Maya scaling-rule/no-inherit-scale subtleties drop out. For each SHP1 packet,
resolve its matrix table through DRW1: rigid slot → joint world matrix;
weighted slot → `Σ wᵢ · (worldᵢ · invBindᵢ)`. Transform positions (normals via
inverse-transpose, renormalize). Strips → lists (odd triangles swap first two
indices), fans → `(0, i, i+1)` (cl.bdl is strips-only; fans implemented for
completeness). Keep Y-up right-handed as-is — same convention as the
viking_room OBJ; the shader's projection handles Vulkan clip space.

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
               "skinning": "link.skin.bin",
               "vertex_layout": ["position3f", "normal3f", "uv02f"],
               "vertex_count": 0, "index_count": 0 },
  "textures": [
    { "name": "linktexS3TC", "file": "tex/12_linktexS3TC.png",
      "wrap_u": "ClampToEdge", "wrap_v": "ClampToEdge",
      "filter": "Linear", "mipmaps": false }
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
  "skeleton": { "joints": [ { "name": "link_root", "parent": -1,
                              "t": [0,0,0], "r_s16": [0,0,0], "s": [1,1,1] } ] }
}
```

- `link.vtx.bin`: little-endian interleaved f32 — pos[3], nrm[3], uv0[2]
  (8 floats/vertex). *(Revised from the original 14-float sketch: cl.bdl has
  no color arrays and one UV channel, and every MAT3 channel sources color
  from registers — see phase_03.md.)*
- `link.idx.bin`: u32 triangle list (2,874 triangles).
- `batches` are in INF1 draw order, each carrying its material slot; the
  example orders opaque before translucent by the material's pixel-engine
  mode (J3D's two-pass rule).

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
  `{ float3 position; float3 normal; float2 uv0; }` (cl.bdl has no vertex
  colors or second UV — rasterized color comes from the register-sourced
  lighting channel), `ParameterBlock` with one `ToonLinkParams` uniform +
  ~~4~~ **2** × `Sampler2D` (tex0/tex1; the unused slot bound to a 1×1 white
  dummy). *(P7 measured the actual surface: no material uses more than two
  texmaps, always slots 0 and 1 — see
  [`link_rendering/phase_07.md`](link_rendering/phase_07.md) decision 1.)*

The shader is a **data-driven interpreter**: per-material TEV configuration
arrives as uniform data built from the manifest. Uniform layout uses **flat
`uint4`/`float4` arrays only** — supported by the codegen since the
vec4-array mini-phase (`0d08a7d`,
[`link_rendering/vec4_array_support.md`](link_rendering/vec4_array_support.md)):
`float4[N]`/`uint4[N]`/`int4[N]` fields generate `[glam::Vec4|UVec4|IVec4; N]`
with compile-time layout proofs, in both std140 and BDA-pointee contexts.
The sketch below is the shape of the P8 layout (the BDA fallback phase_06
Step 1 had decided on is superseded). **The authoritative field list is
[`link_rendering/phase_08.md`](link_rendering/phase_08.md) §Step 2**, which
adds the swap tables, per-stage konst selects, texture-matrix rows and channel
control this sketch omits, and corrects `reg[]`: the manifest's four
`reg_colors` entries load into **REG0/REG1/REG2 plus one unused slot**, not
PREV/REG0/REG1/REG2 (`J3DMatBlock.cpp:810-811`).

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
**The subset is now frozen** (P2 dump; verbatim summary in phase_02.md's
Recorded facts) and is smaller and slightly different than the original
guess:

- 1–3 stages/material; **every op is ADD**, bias ZERO, scale 1, dest PREV
  (no SUB, no ONE/HALF inputs, no COMP ops)
- color inputs C0/CPREV/KONST/RASC/TEXC/ZERO; alpha
  APREV/KONST/RASA/TEXA/ZERO; konst selects K0/K1 and K0_A/K3_A; 2 stages
  with the clamp bit off
- texgens: MTX2x4·TEX0 (identity), **MTX2x4·TEX0 via TEXMTX1** (2
  non-identity texture matrices exist — originally declared out of scope,
  now in scope for the one material using them), SRTG·COLOR0 (identity —
  the ramp path, risk #5 resolved)
- **TEV swap tables** — not in the original guess: 12 materials use
  channel-broadcast tables (identity, RRR+A, GGG+A; ras_sel always 0,
  tex_sel ∈ {0,1,2}) to splat one channel of intensity textures; the
  interpreter and `ToonLinkParams` need a swap-select field
- blends: None, src-alpha, and one **destination-alpha** variant; Z always
  LESS_EQUAL (test/write vary); alpha-compare configs (Always OR Always)
  and (Greater 0 OR Greater 0)
- confirmed absent: indirect stages, fog (declared LINEAR but disabled on
  all 24 materials — the warn-and-force-off path never fires)

**Color space.** GX has no sRGB anywhere — TEV math operates on raw 8-bit
values, and the ramps' banding is authored in those raw values. So: decode and
sample **all** Link textures as `UNORM` (not sRGB), do TEV math on raw values,
and apply the inverse sRGB transfer on the final fragment color so the sRGB
render target round-trips back to the raw value (matching Dolphin's output
path). This is why texture-format options (§4.4) are a hard requirement, not a
nicety.

## 4. Renderer extensions

Four additive changes, each independently landable, ordered below. All
existing examples keep compiling and running; no codegen/template changes, so
`just test` snapshots stay untouched. Defaults reproduce today's behavior
exactly.

Note: the approved-but-unimplemented FrameInputs plan
(`frame_inputs_api.md`) will eventually replace the `gpu_update` closure with
declarative `frame_inputs` calls. P4 deliberately proceeds against the
current closure API, keeping the single-terminal-submit shape
(`submit_draws(self, …)`) that FrameInputs also depends on, so the later
migration stays mechanical.

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
// as shipped in P4: the handle is typed, so a vertex-layout mismatch between
// mesh and pipeline is a compile error (V is inferred at create_mesh)
pub struct MeshHandle<V: VertexDescription> { index: MeshIndex, _phantom_data: PhantomData<V> }

impl Renderer {
    pub fn create_mesh<V: VertexDescription + GPUWrite>(
        &mut self, vertices: &[V], indices: &[u32]) -> anyhow::Result<MeshHandle<V>>;
}

// src/renderer/pipeline.rs:181
pub(super) enum VertexPipelineConfig {
    VertexAndIndexBuffers(VertexAndIndexBuffers),
    SharedMesh(MeshIndex),  // index into Renderer::meshes
    VertexCount,
}

impl<'t, V: VertexDescription> PipelineConfig<'t, V, DrawIndexed> {
    pub fn with_shared_mesh(mut self, mesh: &MeshHandle<V>) -> Self;  // replaces vertex_config
}
```

- `Renderer` gains `meshes: Vec<VertexAndIndexBuffers>`, destroyed at renderer
  teardown. `destroy_pipeline` must not free shared buffers.
- Generated `Resources` carries descriptor bindings only; vertex data is a
  builder step, so the example calls `.pipeline_config(resources)` followed by
  `.with_shared_mesh(&mesh)` and supplies no vertices at all. (Pipelines that
  own their buffers use `.with_vertices(vertices, indices)` instead — this
  replaced an earlier design where `Resources` had `vertices`/`indices` fields
  that shared-mesh consumers had to fill with empty vecs.)

### 4.3 Raster state — **shipped in P5**

```rust
// src/renderer/pipeline.rs:216 (re-exported by `pub use pipeline::*`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterState {
    pub blend: BlendMode,         // Alpha (today's behavior) | Opaque
    pub cull: CullMode,           // Back (default) | None | Front
    pub depth_test: DepthCompare, // Less (default) | LessEqual | Always | Disabled
    pub depth_write: bool,        // true
    pub color_write: [bool; 4],   // all true; exercised later by the eye trick
}
impl Default for RasterState { /* == the pre-P5 hardcoded pipeline state */ }

impl PipelineConfig<…> { pub fn with_raster_state(mut self, s: RasterState) -> Self; } // 309
```

- `create_graphics_pipeline` (3516) consumes `&RasterState` instead of the old
  `depth_test_enable: bool, blend_enable: bool` pair, deriving each field
  through greppable, unit-tested helpers (`vk_cull_mode` 3479,
  `vk_depth_compare` 3489, `vk_color_write_mask` 3498). Three call sites:
  picking (1060, its own literal — uint target, no depth attachment),
  `init_pipeline` (1263), hot-reload recreate.
- `RendererPipeline.disable_depth_test` was replaced by
  `raster_state: RasterState` (pipeline.rs:177); the hot-reload recreate reads
  it back, verified live. The `disable_depth_test` builder field **stays** on
  `PipelineConfig` / `PipelineConfigBuilder` and wins over `depth_test` when
  set — it is emitted by `templates/shader_atlas_entry.rs.askama`, so removing
  it would rewrite every generated file and ~20 snapshots.
- **`PipelineConfigBuilder` has no `raster_state` field** (deviation from the
  phase plan): the template emits a complete struct literal, so `build()`
  defaults the state and `with_raster_state` overrides it. This is what keeps
  P5 snapshot-neutral outside `multi_mesh`.
- `BlendMode::DstAlpha` deliberately does not exist yet; Link's
  destination-alpha materials land with the eye trick in P9. *(Refined by P7's
  measurement: there are **4** of them — `eyeL`, `eyeR`, `mayuL`, `mayuR` —
  not one, and P7 maps them to `Opaque`, which is numerically exact while the
  framebuffer alpha at those pixels is 1. It is: the clear is alpha 1.0, every
  opaque albedo is alpha-255 at every texel, and those four are the first
  translucent batches drawn. See
  [`link_rendering/phase_07.md`](link_rendering/phase_07.md) decision 2 and
  risk 3.)*
  Detailed plan and results:
  [`link_rendering/phase_05.md`](link_rendering/phase_05.md).

### 4.4 Texture options — **shipped in P5**

```rust
// src/renderer.rs:2893/2905/2914
pub enum TextureWrap { Repeat, ClampToEdge, MirroredRepeat }
pub enum TextureColorSpace { Srgb, Unorm }
pub struct TextureOptions {
    pub filter: TextureFilter,
    pub wrap_u: TextureWrap, pub wrap_v: TextureWrap,
    pub mipmaps: bool,
    pub color_space: TextureColorSpace,
}
impl Default for TextureOptions { /* Linear, Repeat, Repeat, true, Srgb */ }

impl Renderer {
    pub fn create_texture_with_options(&mut self, name: impl Into<String>,   // 511
        image: &image::DynamicImage, options: TextureOptions)
        -> anyhow::Result<TextureHandle>;
}
```

- Plumbs through `create_texture` (492, now a wrapper preserving today's
  defaults for all seven existing call sites) / `create_texture_image` (4169) /
  `create_texture_sampler` (4710): `texture_format` (4092) picks
  `R8G8B8A8_{SRGB|UNORM}` in place of the old `TEXTURE_IMAGE_FORMAT` const,
  `mip_levels = 1` when mipmaps are off, `vk_address_mode` (4702) maps the
  wraps, and anisotropy plus `max_lod` are both derived from the same `mipmaps`
  flag that sizes the image (risk #3). `create_texture_with_mips` keeps its
  public signature, so `mipmaps: false` can never reach the pre-baked-mip path.
- **Non-obvious:** `generate_mipmaps` also performs the final
  TRANSFER_DST → SHADER_READ_ONLY layout transition, so the `mipmaps: false`
  path has to do that transition explicitly or the image is left in the copy's
  layout.
- P2's inventory showed **every** Link texture wants the same non-default
  options: `Unorm`, `ClampToEdge`, `mipmaps: false`, `Linear` filter — the
  ramps aren't special; §4.4 is load-bearing for all 44 textures.
- Color-space plumbing is numerically verified: a mid-gray (128) image sampled
  `Srgb` vs `Unorm` renders at 117 vs 172 through the `B8G8R8A8_SRGB`
  swapchain, a linear ratio of 2.32 == 0.502/0.2158. Note the **`Unorm` sample
  is the brighter one** — the phase plan had this backwards.

### 4.5 Explicitly deferred

Eye write-mask multi-pass (needs `color_write` exercised + per-pass shape-group
toggling), destination-alpha tricks, stencil, u16 index buffers, BTP/BTK eye
animation, runtime skinning.

Also deferred from P4: **picking + multi-draw.** Picking stays on the legacy
single-draw wrapper (`draw_vertex_count_with_picking`, guarded by a
`debug_assert` that the draw queue is empty); revisit integrating a picking
config into `submit_draws` when something needs to pick over a multi-draw
frame.

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
  `(0.45, 0.5, 0.55)` — seeds, tuned visually against noclip. *(P8 measured
  the ambient: it comes from the manifest, `[50,50,50,50]` on all 24
  materials, and `lit_mask` is 3, so **two** lights are needed. Replacing the
  light seeds with exact `dKy_tevstr_c` values read from emulated RAM is now
  an optional follow-up, not a P8 gate — see risk #8.)* Fed into
  `reg[0]`/konst slots the way `setLightTevColorType` does (C0 = light color,
  K0/K1 = ambient; see `../tww/src/d/d_kankyo.cpp`). Then queue one
  `queue_draw_index_range` per batch — INF1 order in P6, and from P7 a stable
  partition of it into opaque-then-translucent by `pe_mode` (J3D's two-pass
  rule; INF1 order is preserved within each group) — and finish with
  `submit_draws(|gpu| { /* write all per-material uniforms */ })`.

## 6. Phases & verification

Converter (P1–P3) and renderer (P4–P5) tracks are independent and can be
interleaved. Each phase is separately verifiable — full detail on the oracles
(GCFT/gclib, SuperBMD, noclip, Dolphin) and our own tests per phase in
[`link_rendering/tests.md`](link_rendering/tests.md).

| Phase | Deliverable | Verify | Est. |
|---|---|---|---|
| **P0** ✅ `a76d0cb` | `scripts/extract_link.sh`, `just extract-link`, `.gitignore` entry — detailed plan: [`link_rendering/phase_00.md`](link_rendering/phase_00.md) | sizes + SHA256s match `dtk vfs ls` (golden hashes, permanently stable); `J3D2bdl4` magic; idempotent; `git status` clean | ½ day |
| **P1** ✅ `6431f0a` | converter skeleton: `be.rs`, chunk walk, `--info` chunk table — detailed plan: [`link_rendering/phase_01.md`](link_rendering/phase_01.md) | internal invariants (chunk sizes sum to file size, 42 joints, cross-chunk counts agree); `--info` diffed against a gclib oracle (`just link-verify-p1`, zero-line diff); `BeReader` unit tests on synthetic buffers | 1 day |
| **P2** ✅ `8a0a4af` | TEX1+BTI decode → PNGs (+ standalone `.bti` re-emit per entry); full MAT3 parse + `--dump-mat3` — detailed plan: [`link_rendering/phase_02.md`](link_rendering/phase_02.md) | **as run** (`just link-verify-p2`): gclib pixel-diff 44/44 zero differences; canonical MAT3 table zero-line diff vs a gclib oracle (SuperBMD demoted to manual backup); synthetic per-format tile snapshots (insta); ramp names confirmed; **TEV subset frozen** (see §3) | 2–3 days |
| **P3** ✅ `7704292` | geometry: baked bind pose, strip→list, manifest v1 (full TEV materials), `--obj`+`.mtl` export — detailed plan: [`link_rendering/phase_03.md`](link_rendering/phase_03.md) | **`just link-verify-p3` green**: invBind identity (residual 0.0145) + weighted-identity (0.0077) hard checks; canonical geometry table **zero-diff** vs a gclib+struct-walk oracle; exactly 2,874 triangles; Blender pass partial (face orientation uniform red, rigid attachment + per-batch isolation OK). Stored-AABB cross-check dropped as redundant; full Blender pass outstanding | 3–4 days |
| **P4** ✅ `4621112` | renderer 4.1 + 4.2 (multi-draw, index ranges, shared mesh) + committed `examples/multi_mesh.rs` (multiple pipelines, one shared mesh, disjoint index sub-ranges) — detailed plan: [`link_rendering/phase_04.md`](link_rendering/phase_04.md) | **as run**: `just test` green (pre-existing per-shader snapshots byte-identical); `just lint` clean; validation sweep 15/15 examples; multi_mesh tiles its index buffer exactly, perturbation test confirms gaps/spikes/`debug_assert`; multi-pipeline hot reload clean; no VMA leak on exit | 2 days |
| **P5** ✅ | renderer 4.3 + 4.4 (raster state, texture options); `examples/multi_mesh.rs` grew 12 view-space test panels (17 pipelines, 18 draws, 7 texture handles, 3 procedural images) — detailed plan: [`link_rendering/phase_05.md`](link_rendering/phase_05.md) | **as run**: `just test` green, snapshot churn exactly multi_mesh's `.rs`+`.json` (branching snapshot unmoved, no atlas-index change); `RasterState` default-equivalence + enum-mapping unit tests; `just lint` clean; validation sweep 15/15 (run twice — once with examples untouched); every test object shows its artifact, **all six fields perturbed and reverted**, each artifact vanishing; sRGB-vs-UNORM verified numerically (117 vs 172, linear ratio 2.32); hot reload keeps per-pipeline raster state; clean exit with no VMA leak, and the leak check itself validated by injecting one | 1–2 days |
| **P6** ✅ `9508563` | `toon_link.shader.slang` v0 (normals-as-color debug frag) + example loads manifest, draws all batches — detailed plan: [`link_rendering/phase_06.md`](link_rendering/phase_06.md) (Step 1's smoke test superseded by the vec4-array mini-phase `0d08a7d`) | **as run**: `just test` green (churn = toon_link additions only); correctly shaped Link over a full orbit, smooth normal gradients; **winding flipped in the converter** (risk #3: model was inside-out under manifest cull; one-line `link.idx.bin` sha256 update, `just link-verify-p3` green); all 24 isolation batches identified; UV mode clean; hot reload at 24-pipeline scale keeps raster state; validation sweep 16/16; no VMA leak on real close | 1–2 days |
| **P7** 🚧 code landed, **unverified** | albedo-only: real textures, tex0 sample, alpha-compare discard, per-material raster state; plus `pe_mode` draw ordering and inverse-sRGB output (pulled forward from P8) — detailed plan: [`link_rendering/phase_07.md`](link_rendering/phase_07.md) | **static gates pass** (shader + bindings regenerated, churn confined to `toon_link`, `cargo check --all-targets` / clippy debug+release / fmt clean, converter and golden hashes untouched). **Every runtime gate is still outstanding**: written in a headless container with no video device and no converted assets, so UV features vs noclip, alpha-cutout edges, per-material cull, and especially the **numeric** gamma check (the gate for the sRGB transfer direction) have not been observed. Run these before marking ✅ | 1 day |
| **P8** | full TEV interpreter (`shaders/source/tev.slang`) + lighting channel + SRTG ramp + pupil texture matrices; the subset gate lands as `src/bin/convert_link/tev_ir.rs`; `src/tev_pack.rs` + expanded debug modes and light controls in the example — detailed plan: [`link_rendering/phase_08.md`](link_rendering/phase_08.md) | structured side-by-side vs noclip per feature (skin, tunic bands, hair highlight, eye whites); rotate light — terminator bands sweep and stay banded, and only the 12 lit materials respond; isolate batch N for any wrong material and diff its equations against `mat3_dump.txt`; converter gate + `tev_pack` unit tests + `COLOR0`/SRTG-texcoord debug modes as the emulator-free cross-checks. **Dolphin is explicitly not P8's oracle** — the `.dff`/FIFO/software-renderer suite is an optional escalation in [`link_rendering/follow_up.md`](link_rendering/follow_up.md) §5, so risks #6 and #8 ship reasoned rather than measured | 3–5 days |
| **P9** | optional polish: `--casual` clothes; eye write-mask multi-pass; BCK-sampled pose | casual: P7-style UV checks; eye trick vs **Dolphin** (noclip may not implement it) | 2+ days |

Rough total: ~3 weeks of focused work. Once a converter phase's output is
verified, commit SHA256 golden hashes of `assets/link/converted/` outputs
(hashes of derived data, not the data) so later refactors get free regression
detection — in place since P2 (`scripts/link_converted.sha256`). Dev-only
oracle dependency, not needed to build or run: gclib (Python, uv PEP-723
scripts, pinned to 1.0.0 @ `6412774`); SuperBMD (mono) is optional/manual
only.

## 7. Risks & unknowns

Expanded walkthrough of each risk (mechanism, failure mode, why the mitigation
works): [`link_rendering/risks.md`](link_rendering/risks.md).

1. **SHP1 matrix groups** — per-packet matrix tables with `0xFFFF` "inherit from
   previous packet" entries and the per-vertex `PNMTXIDX` attribute (value/3 =
   table slot). Getting this wrong = exploded vertices. *Real for cl.bdl: 77
   inherit-entries, 240 of 270 DRW1 slots weighted.* **Resolved in P3 (green):**
   the file's own inverse bind matrices gave a numeric FK oracle
   (`world·invBind = I`, residual 0.0145), the weighted-identity check passed
   (0.0077), and the canonical `--dump-geometry` diff pinned the raw tables — no
   exploded mesh. (Note: `mUseMtxIndex` is *not* the matrix-table head; the
   useMtx table drives everything.)
2. ~~**GX fixed-point vertex formats**~~ — *resolved by the P3 probe*:
   positions/normals are f32; only UVs are fixed-point (s16, shift 8). The
   format table is still implemented generally and diffed via
   `--dump-geometry`.
3. ~~**Winding after Y-flip**~~ — *resolved in P6*: under manifest cull the
   model rendered uniformly inside-out (eye decals and shield visible through
   the head/torso), confirming GX winding does **not** survive the Y-flip.
   Fixed where planned: `output.rs::build` swaps each triangle's last two
   indices while concatenating per-shape lists (`pose.rs` stays GX-native);
   exactly one golden-hash line (`link.idx.bin`) changed and
   `just link-verify-p3` stayed green. Cull mapping stays literal
   (`Cull_Back → Back`); full orbit shows no missing or inside-out geometry.
4. ~~**Uniform array codegen**~~ — *resolved by the vec4-array mini-phase*
   (`0d08a7d`,
   [`link_rendering/vec4_array_support.md`](link_rendering/vec4_array_support.md)):
   the codegen now supports `float4[N]`/`uint4[N]`/`int4[N]` array fields
   (plus bare `uint4`/`int4` vectors and `ScalarType::Int32`) in std140
   uniform and std430 BDA-pointee contexts, emitting `[glam::…; N]` with
   compile-time offset/size proofs; anything outside that 16-byte-element
   subset is a hard, actionable error instead of a `todo!`. Verified by
   committed alignment-test shaders, the check_crate compile of the emitted
   asserts, and a runtime pattern-band render. P8 uses the §3 flat-array
   `ToonLinkParams` as sketched; the BDA fallback recorded in
   [`link_rendering/phase_06.md`](link_rendering/phase_06.md) Step 1 is
   superseded, and P6's planned smoke test is moot.
5. ~~**SRTG texgen details**~~ — *resolved by the P2 dump*: SRTG sources
   COLOR0 with the identity matrix; no texture matrix on the ramp path. Two
   findings replace it: (a) 2 **non-identity texture matrices** exist
   (TEXMTX1 on one MTX2x4 texgen) — small, but in scope now; *P7 pinned them
   down: they are `eyeL`/`eyeR`'s **texcoord 1** (the `hitomi` pupil), so
   nothing before P8 evaluates them, and every material's texcoord 0 is the
   plain identity `MTX2x4 · TEX0 · GX_IDENTITY`*; (b) **swap
   tables** are used by 12 materials (channel broadcasts) and must be in the
   interpreter — see §3.
6. **S10 register semantics** — TEV intermediates are signed 10-bit, clamped
   per-stage only when the clamp bit is set. Respect the clamp bit always
   (*2 real stages run with it off*); add explicit range clamps only if
   banding artifacts appear. Low risk: all ops are ADD/scale-1, so values
   stay near range.
7. ~~**Fog**~~ — *resolved*: declared LINEAR but disabled on all 24
   materials; the warn-and-force-off path never fires.
8. **Lighting values** — exact daytime `dKy_tevstr_c` values are buried in
   kankyo tables; P8 ships hand-tuned seeds. Upgrading to ground truth means
   reading the live values from emulated RAM (dolphin-memory-engine + tww
   decomp symbol addresses), which is now an **optional** escalation in
   [`link_rendering/follow_up.md`](link_rendering/follow_up.md) §5 rather than
   a P8 gate. Practical consequence for P8: a color mismatch against noclip
   could be our TEV math *or* our light seeds, and nothing in the phase can
   tell them apart — so adjudicate on band *structure*, which the seeds do not
   affect, not band *color*, which they do.
