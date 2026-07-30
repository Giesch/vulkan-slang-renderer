# Phase 6: `toon_link` v0 — debug-shaded Link

Detailed plan for P6 of [`../link_rendering.md`](../link_rendering.md) §6
(shader §3 v0, example §5). Estimated: 1–2 days. Verification follows
[`tests.md`](tests.md) §P6. This is the first phase that joins the two tracks:
it consumes both the converter output (P0–P3) and the renderer extensions
(P4 `4621112`, P5 `9d8e468`), so it needs `assets/link/converted/` on disk
(`just extract-link && just convert-link`). Line numbers below verified at
`9d8e468`.

**Goal**: after P6, `timeout 3 just dev toon_link` renders a correctly shaped,
correctly proportioned Toon Link — all 24 batches drawn from one shared mesh
through 24 per-material pipelines — with a v0 debug fragment shader
(world-space normals as color, UVs as a second mode), a slow-orbit camera,
per-material cull + a minimal depth mapping, and the winding question (master
plan risk #3) settled one way or the other in the converter.
**Update (2026-07-24, `0d08a7d`)**: Step 1's uniform-array smoke test and the
BDA data-path decision it fed are **superseded** — the vec4-array mini-phase
([`vec4_array_support.md`](vec4_array_support.md)) landed codegen support for
`float4[N]`/`uint4[N]`/`int4[N]` array fields (verified by committed
alignment-test shaders and a runtime pattern-band render), so P8 uses the
master plan §3 flat-array `ToonLinkParams` as sketched. Skip Step 1 entirely.
No textures, no TEV, no lighting: those are P7/P8.

**Deliverables**

1. ~~Uniform-array smoke test~~ — superseded by the vec4-array mini-phase
   (`0d08a7d`, [`vec4_array_support.md`](vec4_array_support.md)); master plan
   risk #4 is closed and the §3 flat-array `ToonLinkParams` sketch stands
2. `shaders/source/toon_link.shader.slang` — v0: `Vertex { position, normal,
   uv0 }`, `ToonLinkParams { mvp, debugMode }`, normals-as-color / UV-as-color
   fragment; new generated files + snapshots *added*, all pre-existing
   per-shader snapshots byte-identical
3. `examples/toon_link.rs` — manifest load (helpful bail when assets are
   missing), `link.{vtx,idx}.bin` → one `create_mesh`, 24 pipelines (one per
   material slot) sharing the mesh and shader, one `queue_draw_index_range`
   per batch in INF1 order, orbit camera, debug-mode keys, batch-isolation
   keys
4. Winding decision executed (risk #3): per-material cull enabled; if the
   model is inside-out, a one-line triangle-order flip in
   `src/bin/convert_link/output.rs` + the `link.idx.bin` line of
   `scripts/link_converted.sha256` regenerated + `just link-verify-p3` re-run
   green
5. Master-plan edits: risk #4 rewritten from "unverified" to its measured
   result; risk #3 closed with the outcome; §6 P6 row ✅ + hash at the end
6. No `Cargo.toml` change, no template change, no changes to existing
   examples or the committed converter oracle scripts; Recorded facts below
   filled in

## Renderer facts this phase relies on

All at `9d8e468`; re-verify line numbers before editing (renderer.rs is
actively developed).

### Draw / mesh / pipeline APIs (P4+P5, shipped)

- `Renderer::create_mesh` → `MeshHandle<V>` (typed; bails on empty inputs);
  `PipelineConfig::with_shared_mesh` / `with_raster_state`;
  `RasterState { blend, cull, depth_test, depth_write, color_write }` with
  `Default` == legacy behavior (Alpha blend, Back cull, Less, write on).
  The generated `pipeline_config(resources)` takes empty `vertices`/`indices`
  vecs when a shared mesh follows — the exact pattern of
  `examples/multi_mesh.rs` (the worked example for everything in this phase).
  **Update (2026-07-24, post-P6)**: those two fields were removed from the
  generated `Resources` struct — it now carries descriptor bindings only, and
  vertex data is a builder step (`.with_vertices(v, i)` or
  `.with_shared_mesh(&mesh)`). Shared-mesh consumers pass no vertex data at
  all. The P6 code sketches below still show the empty-vec form as it was
  written; both examples were migrated with the refactor.
- `FrameRenderer::queue_draw_index_range(&pipeline, first_index, index_count)`;
  the same pipeline may be queued multiple times; terminal
  `submit_draws(self, gpu_update)` writes every pipeline's uniforms in one
  closure. **Uniforms are per-pipeline, not per-draw** — fine here: one
  transform + one debug mode shared by all 24.
- Camera/uniform pattern: `MVPMatrices { model, view, proj }` +
  `gpu.write_uniform`; `shaders/source/mvp.slang` provides
  `MVPMatrices.project()` (Vulkan Y-flip via `reflectY`) and
  `rotateDirection` (model-matrix rotation — exact for P6's rotation-free
  uniform-scale model matrix). `FrameRenderer::aspect_ratio()` exists
  (multi_mesh.rs:389).
- Input: `Game::input(Input)`; available keys (`Key` enum,
  src/game/traits.rs:184): W A S D Q E R F Space Num1–Num4.
  `KeyDown`/`KeyUp` only — isolation stepping acts on `KeyDown`.
- `DepthCompare::Always` exists (P5) — used for the 8 `z_test: false`
  materials (decided in planning; see Step 4).

### Codegen / reflection (the risk-#4 finding)

- `reflect_struct_fields` (src/shaders/reflection/parameters.rs:163) handles
  Scalar / Vector / Matrix / Struct / Resource / Pointer field kinds and hits
  `todo!("field type layout kind not handled: {k:?}")` at
  **parameters.rs:411** for anything else — including arrays. The JSON model
  `StructField` enum (src/shaders/json/parameters.rs) has no Array variant,
  and the askama templates have zero array handling. So a `uint4 foo[8]`
  uniform field should panic `just shaders` — Step 1 confirms.
- The same walker recurses into BDA pointees, so **pointee structs cannot
  contain arrays either**. What *is* proven: pointer-indexing a flat struct —
  `ImmutableAddr<Sprite>` (shaders/source/sprite_batch.shader.slang,
  shaders/source/addr.slang), created with `create_immutable_buffer`,
  pointed at via `gpu.current_immutable_addr`, written via
  `gpu.write_immutable` — see examples/sprite_batch.rs.
- `uint` scalar uniform fields are proven in generated code (space_invaders
  → `pub flags: u32`); **no generated file currently contains a `UVec4`**,
  so vector-of-uint uniforms are unproven — v0 uses a scalar `uint debugMode`.

### Converter / asset facts

- `assets/link/converted/link.manifest.json` matches `src/model_manifest.rs`
  1:1. Measured from the file on disk: 24 batches ↔ 24 material slots,
  **bijectively** (every slot appears exactly once, INF1 order). `cull`
  strings are **`"Cull_Back"` (23) and `"Cull_None"` (1)** — *not* the
  `"Back"`/`"None"` spelling in the master plan §2.3 sketch. `pe_mode`:
  12 Opaque / 12 Translucent. z_func: all `Less_Equal`; **`z_test` false on
  8 materials** (eye/brow decal overlays) — mapped in Step 4.
- `link.vtx.bin`: interleaved LE f32 pos[3] nrm[3] uv[2] = 32 bytes/vertex ×
  1754. `link.idx.bin`: LE u32 triangle list, 8622 indices = 2874 triangles,
  **GX-native winding, not flipped** (P3 recorded fact; *no longer true —
  Step 5 flipped it in the converter, see Recorded facts*). P3's OBJ AABB:
  X 125.36, Y 124.06 (feet at Y ≈ 0), Z 89.49 model units — camera framing
  derives from this.
- Winding-flip plumbing: strips→lists happens in `pose.rs::expand`
  (src/bin/convert_link/pose.rs:279; unit tests at pose.rs:324 encode GX
  strip semantics and must not change). Per-shape indices are concatenated
  into `Converted.indices` in `output.rs::build` (output.rs:32–48), which
  feeds both `link.idx.bin` and the `--obj` export. The canonical
  `--dump-geometry` table is **raw file data only** (module doc,
  bmd/geometry_dump.rs:1–11) — a bake-stage flip cannot move the oracle diff.
- Golden hashes: `scripts/link_converted.sha256`. A flip changes exactly one
  line (`link.idx.bin`); manifest/vtx/skin/tex hashes are untouched by
  construction.

## Step 1 — ~~uniform-array smoke test~~ (SUPERSEDED — skip)

**Superseded (2026-07-24) by the vec4-array mini-phase** (`0d08a7d`,
[`vec4_array_support.md`](vec4_array_support.md)): the codegen now reflects
`float4[N]`/`uint4[N]`/`int4[N]` array fields in uniform and BDA-pointee
structs, with compile-time layout proofs and committed test-shader coverage;
that phase's Step 5 already performed this exact pattern-band experiment
(and it passed). **P8 therefore uses the master plan §3 flat-array
`ToonLinkParams`, not the BDA layout below** — konst/reg stay arrays
(`float4 konst[4]`, `float4 reg[4]`), the per-stage packed configs stay
`uint4 …[8]` arrays. The remainder of this step is preserved as the record
of the pre-mini-phase decision; do not execute it.

Before any Link code: a five-minute experiment. Create a **throwaway,
never-committed** `shaders/source/uniform_array_smoke.shader.slang`:

```slang
module uniform_array_smoke;
import mvp;

ParameterBlock<SmokeParams> params;

struct SmokeParams {
    MVPMatrices mvp;
    uint4 pattern[8];   // the exact shape the master plan §3 sketch assumed
}

struct Vertex { float3 position; }

[shader("vertex")]
float4 vertMain(Vertex v) : SV_Position {
    return params.mvp.project(v.position);
}

[shader("fragment")]
float4 fragMain(float4 pos : SV_Position) : SV_TARGET {
    // known pattern as vertical color bands — only reachable if codegen survives
    let p = params.pattern[uint(pos.x / 64.0) % 8];
    return float4(float3(p.xyz) / 255.0, 1.0);
}
```

Run `just shaders`. **Expected** (from source inspection): a panic —
`todo!("field type layout kind not handled: …")` at
src/shaders/reflection/parameters.rs:411 (or a slang-side error even
earlier; either way arrays are confirmed unsupported). Record the exact
failure text. Optionally repeat with `float4 pattern[8]` to show it is
structural, not uint-specific. If it *succeeds*, the finding is falsified —
record that, keep the master plan's `uint4[8]` layout for P8, and revisit
the decision below.

**P8 data path — originally decided in planning (user-approved): (a) BDA
immutable buffer, pointer-indexed flat structs** — *since reversed; the
vec4-array mini-phase made option (b) cheap and it landed, so P8 uses flat
uniform arrays (see the superseded banner above).* The BDA layout as it was
decided, for the record:

```slang
struct TevStagePacked {      // std430; NO array fields (pointees share the walker)
    uint4 colorIn;  uint4 colorOp;   // a,b,c,d / op,bias,scale,dest+clamp
    uint4 alphaIn;  uint4 alphaOp;
    uint4 order;                     // texcoord, texmap, ras chan, kcsel/kasel, swap
}
struct ToonLinkParams {
    MVPMatrices mvp;
    // fixed-count register sets stay inline as NAMED fields, not arrays:
    float4 konst0; float4 konst1; float4 konst2; float4 konst3;
    float4 reg0;   float4 reg1;   float4 reg2;   float4 reg3;
    // …lighting fields per master plan §3…
    uint4 control;                         // numStages etc.
    ImmutableAddr<TevStagePacked> stages;  // stages[i], i < numStages
}
```

Buffer granularity (one 8-entry buffer per material vs one shared 24×8
buffer + base index) was to be P8's call. Alternatives considered then:
(b) extend codegen for 16-byte-element arrays — at the time judged too much
plumbing for one consumer, **this is what actually happened** (as its own
mini-phase, with zero template changes needed — the cost estimate was
pessimistic); (c) wc_advect-style flattening to ~40 named `uint4` fields —
noisy, never seriously in play.

**Gate:** none — step skipped; master plan risk #4 and §3 already updated by
the mini-phase reconciliation.

## Step 2 — `toon_link.shader.slang` v0 + codegen

```slang
module toon_link;

import mvp;

ParameterBlock<ToonLinkParams> params;

struct ToonLinkParams {
    MVPMatrices mvp;
    // 0 = world-space normals as color (the P6 diagnostic), 1 = uv0 as color.
    // Scalar uint: one mode flag needs no vector (uint4/int4 are supported
    // since the vec4-array mini-phase, so this is a fit choice, not a
    // workaround).
    // NOTE: changing this struct's shape while `just dev` runs trips
    // assert_shader_interface_unchanged — body edits hot-reload, shape
    // edits need `just shaders` + restart.
    uint debugMode;
}

struct Vertex {
    float3 position;
    float3 normal;   // model space; baked pose
    float2 uv0;
};

struct FragVertex {
    float4 position : SV_Position;
    float3 normal;
    float2 uv0;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    // model matrix is rotation-free uniform scale in P6, so rotateDirection
    // is exact; renormalized per-fragment after interpolation
    let worldNormal = params.mvp.rotateDirection(vertex.normal);
    return FragVertex(position, worldNormal, vertex.uv0);
}

[shader("fragment")]
float4 fragMain(FragVertex fv) : SV_TARGET {
    if (params.debugMode == 1) {
        return float4(fv.uv0, 0.0, 1.0);   // UV debug: red = u, green = v
    }
    let n = normalize(fv.normal);
    return float4(n * 0.5 + 0.5, 1.0);     // normals debug
}
```

Alpha is constant 1.0 and every P6 pipeline blends `Opaque`, so batch draw
order carries no blending semantics yet (that starts in P7).

Then `just shaders` → `just test`. **Expected snapshot churn — additions
only** (same shape as P4's multi_mesh addition): new toon_link `.rs` + `.json`
snapshots, the atlas-index snapshot moves, possibly one new branching-snapshot
line. Every pre-existing per-shader snapshot byte-identical.

**Gate:** `just shaders` + `just test` green with exactly that churn;
`just lint` clean.

## Step 3 — example: manifest load + mesh

`examples/toon_link.rs`, setup half. Types from
`vulkan_slang_renderer::model_manifest::Manifest` (shared with the converter
by design — master plan §2.2); `serde_json` is already a dependency.

```rust
fn converted_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/link/converted")
}

fn load_manifest(dir: &Path) -> anyhow::Result<Manifest> {
    let path = dir.join("link.manifest.json");
    let bytes = std::fs::read(&path).with_context(|| {
        format!(
            "{}: not found — run `just extract-link && just convert-link` first \
             (assets are gitignored; you need the disc image, see phase_00)",
            path.display()
        )
    })?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// link.vtx.bin: interleaved LE f32 pos[3] nrm[3] uv0[2] — 32 bytes/vertex.
const VERTEX_STRIDE: usize = 32;

fn load_vertices(path: &Path, expected: u32) -> anyhow::Result<Vec<Vertex>> {
    let bytes = std::fs::read(path)?;
    anyhow::ensure!(bytes.len() == expected as usize * VERTEX_STRIDE, /* … */);
    let f = |b: &[u8], i: usize| f32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
    Ok(bytes.chunks_exact(VERTEX_STRIDE)
        .map(|v| Vertex {
            position: Vec3::new(f(v, 0), f(v, 1), f(v, 2)),
            normal: Vec3::new(f(v, 3), f(v, 4), f(v, 5)),
            uv0: Vec2::new(f(v, 6), f(v, 7)),
        })
        .collect())
}
// load_indices: chunks_exact(4) → u32::from_le_bytes, size ensured
```

In `setup`: load manifest; load vertices/indices sized by the manifest's own
`vertex_count`/`index_count` (1754 / 8622 — assert, don't hardcode);
cross-check `indices.len() % 3 == 0`, `max index < vertex_count`, and that
the batches tile `[0, index_count)` contiguously in order (true by
construction in output.rs — the assert is the honesty check, multi_mesh
style); one `renderer.create_mesh(&vertices, &indices)?`.

**Gate:** `cargo build --examples` clean; running without assets prints the
helpful bail; running with assets reaches end of setup.

## Step 4 — pipelines, draws, camera, debug modes

**Pipeline granularity (decided in planning): one pipeline per material slot
(24), all sharing the one shader and the one mesh.**

- Measured: batches ↔ material slots are 1:1 in cl.bdl, so per-material is
  per-batch anyway — no savings from coarser granularity.
- The Step 5 winding gate needs *per-material* cull (`Cull_None`'s one
  material must not get Back), and cull is pipeline state.
- 24 per-material pipelines is the terminal shape: P7 adds per-material
  textures, P8 per-material TEV data — both per-pipeline resources. No
  rework later; multi_mesh already proved 17 pipelines in one pass.

**P6 raster mapping (deliberately partial — decided in planning, including
the z_test pull-forward):**

- `blend: Opaque` for all (v0 writes alpha 1.0; blending semantics start P7)
- `cull`: manifest-driven through a bring-up override (below)
- `z_test: false` (8 decal materials) → `depth_test: Always`,
  `depth_write: false` (GX disables depth writes when compare is off; INF1
  order draws the decals after the face, and per-material Back cull hides
  them when the head faces away — approximating GX). `z_test: true` →
  default `Less`/write-on; the exact `Less_Equal` mapping and everything
  else (alpha-compare, blend modes) is P7.

```rust
/// Step-5 knob: Some(CullMode::None) during bring-up; None = manifest cull.
/// Committed state: None.
const CULL_OVERRIDE: Option<CullMode> = Some(CullMode::None);

fn raster_state(m: &MaterialEntry) -> anyhow::Result<RasterState> {
    let cull = match CULL_OVERRIDE {
        Some(c) => c,
        None => match m.cull.as_str() {
            // actual strings in link.manifest.json (verified on disk)
            "Cull_Back" => CullMode::Back,
            "Cull_None" => CullMode::None,
            "Cull_Front" => CullMode::Front,
            other => anyhow::bail!("unmapped GX cull mode {other:?}"),
        },
    };
    let (depth_test, depth_write) = if m.z_test {
        (DepthCompare::Less, true)      // full z_func mapping is P7
    } else {
        (DepthCompare::Always, false)   // GX: compare off ⇒ write off
    };
    Ok(RasterState { blend: BlendMode::Opaque, cull, depth_test, depth_write,
                     ..Default::default() })
}
```

Setup tail: per material — `create_uniform_buffer::<ToonLinkParams>()`,
`Resources { vertices: vec![], indices: vec![], params_buffer: &buf }`,
`Shader::init().pipeline_config(resources).with_shared_mesh(&mesh)
.with_raster_state(raster_state(material)?)`, `create_pipeline`. Store
`Vec<(PipelineHandle<DrawIndexed>, UniformBufferHandle<ToonLinkParams>)>`
indexed by material slot.
*(Post-P6: `Resources { params_buffer: &buf }` — the empty vecs are gone, see
the §"Draw / mesh / pipeline APIs" update.)*

Draw + camera (P3 AABB: Link is 124.06 units tall, feet at Y ≈ 0; scale 0.01
→ 1.24 units):

```rust
const MODEL_SCALE: f32 = 0.01;

// draw(): slow orbit, ~20°/s
let model = Mat4::from_scale(Vec3::splat(MODEL_SCALE));
let target = Vec3::new(0.0, 0.62, 0.0);
let eye = target + Mat3::from_rotation_y(orbit) * Vec3::new(0.0, 0.25, 2.8);
let view = Mat4::look_at_rh(eye, target, Vec3::Y);
let proj = Mat4::perspective_rh(45f32.to_radians(), renderer.aspect_ratio(), 0.1, 20.0);

for (i, batch) in self.manifest.batches.iter().enumerate() {
    if self.isolate.is_some_and(|only| only != i) { continue; }
    let (pipeline, _) = &self.pipelines[batch.material as usize];
    renderer.queue_draw_index_range(pipeline, batch.first_index, batch.index_count);
}
let mvp = MVPMatrices { model, view, proj };
renderer.submit_draws(|gpu| {
    for (_, params_buffer) in &mut self.pipelines {
        gpu.write_uniform(params_buffer, ToonLinkParams { mvp, debug_mode: self.debug_mode });
    }
})
```

**Update (2026-07-24, post-P6)** — the shipped loop is the same shape with two
index spaces now newtyped, because cl.bdl maps batches to material slots as a
*permutation*, not identity (batch 1 → slot 17), so mixing them silently drew
the wrong material:

- `isolate: Option<BatchIndex>`, compared against `BatchIndex::from_raw(i)`;
  Q/E use `BatchIndex::{FIRST, next, prev}`.
- material lookup goes through `MaterialSlot::from_manifest(batch.material)`,
  the single conversion point from the manifest's raw `u16`, then
  `self.pipeline(slot)` / `self.material(slot)`.

The sketch's bare `mvp` (no `.clone()`) is also literally correct again: the
shipped code needed `mvp.clone()` until generated GPU-layout structs gained
`Copy`.

**Debug controls** (`Game::input`; keys per traits.rs:184):

| key | action |
|---|---|
| Num1 / Num2 | `debug_mode` = 0 (normals) / 1 (UV) |
| Q / E | isolation: previous / next batch (wrapping; from None starts at 0) |
| Space | clear isolation (draw all batches) |

On isolation change, print `batch {i}: shape {shape} material {slot} "{name}"
[{first_index}..+{index_count}]` so a wrong batch can be named. This pulls
part of P8's "single-material isolation debug key" forward (decided in
planning — ~20 lines of CPU-side queue filtering, and Step 5's triage wants
it). Isolation must live in *queueing*, not a uniform — uniforms are
per-pipeline. *(Post-P6: `setup` also prints this control table plus the
batch/material/vertex counts on startup, so the keys are discoverable without
reading the source.)*

**Gate:** `timeout 3 just dev toon_link` with `CULL_OVERRIDE =
Some(CullMode::None)` shows a recognizable, correctly proportioned Link in
smooth normal-gradient colors; UV mode and isolation keys work; no
validation output.

## Step 5 — winding check (risk #3)

With the Step 4 gate green under cull None:

1. Flip `CULL_OVERRIDE` to `None` (manifest cull: 23× Back, 1× None) and
   rerun. Decision tree:
   - **Nothing disappears, silhouette unchanged** → GX-native winding
     survives the Y-flip as-is. Record, close risk #3, no converter change.
   - **Inside-out / body parts vanish** → flip triangle order **in the
     converter**, then `just convert-link` and re-verify.
   - **Mixed result** (some batches vanish, some fine) contradicts P3's
     Blender reading (uniform face orientation) — stop and investigate; that
     is a bake bug, not a winding convention.
2. **Where the flip lives** (decided in planning): in `output.rs::build`'s
   `Converted` assembly (output.rs:32–48) — swap each triangle's last two
   indices while concatenating per-shape index lists. **Not** in
   `pose.rs::expand` (pose.rs:279): its unit tests (pose.rs:324) document GX
   strip semantics and must keep meaning "what the file says", independent
   of any target-API convention.
3. **Oracle/golden interactions** (in order):
   - `--dump-geometry` prints raw file data only, so
     `just link-verify-geometry` cannot move. Run `just link-verify-p3`
     anyway to prove it stays green post-flip.
   - `scripts/link_converted.sha256`: exactly one line changes
     (`link.idx.bin`). Update that single line (keeps the diff reviewable).
   - `--obj` shares `Converted.indices`, so a Blender face-orientation
     re-check flips from uniform red to uniform blue — free confirmation;
     record if performed.
4. Re-run: per-material cull on, nothing missing over a full orbit, the one
   `Cull_None` material still shows back faces. Commit with
   `CULL_OVERRIDE = None` (manifest-driven) as the final state.

**Gate:** per-material cull enabled; full orbit shows no vanishing or
inside-out geometry; if the converter changed: `just link-verify-p3` green,
converter unit tests green, sha256 line updated in the same commit.

## Step 6 — verification + docs

Run the test plan; fill Recorded facts; update master plan §6 (P6 row ✅ +
hash) and risk #3 (closed, outcome). (Risk #4 and the §3 `ToonLinkParams`
sketch were already reconciled by the vec4-array mini-phase — the sketch's
flat arrays stand; no BDA rewrite.)

## Test plan

**Automated (`just test` / CI):**

- Insta gate: snapshot churn is exactly Step 2's additions; every
  pre-existing per-shader snapshot byte-identical.
- `cargo check --all-targets`, `cargo build --examples`, `just lint` clean.
  (`--all-targets`, not `--all`: the latter means "all workspace members" and
  silently skips examples. `just lint` was given the same flag post-P6.)
- If the converter changed: `just link-verify-p3` + converter unit tests
  green.

**Validation sweep** — documented loop, not a recipe (P4/P5 convention);
`toon_link` only means something on a machine with converted assets:

```sh
for e in $(ls examples | sed 's/\.rs$//'); do
  timeout 3 just dev "$e" 2>&1 | grep -iE "validation|VUID" && { echo "FAIL: $e"; exit 1; }
done; echo "sweep clean"
```

**Eyeball (results → Recorded facts):**

1. **Shape**: correctly proportioned Link over a full orbit — head, ears,
   hair, tunic, belt, scabbard attached and placed (a detached rigid part
   would be a P3 regression, not new breakage).
2. **Normal gradients**: smooth across curved surfaces; hard seams that
   don't follow real creases = normal-transform bug; faceted patches = bad
   baked normals.
3. **Silhouette vs noclip**: compare outline/proportions at 2–3 canonical
   angles (front, 3/4, profile) against noclip.website screenshots
   (tests.md §P6). Colors will differ — silhouette only.
4. **UV mode** (Num2): plausible per-feature UV islands; no garbage.
5. **Isolation** (Q/E/Space): step all 24 batches; each a sensible body
   part; stdout names match.
6. **Decal check**: what the 8 no-ztest eye/brow materials look like under
   `Always`/no-write — record for P7's full depth mapping.
7. **Hot reload**: edit the fragment body while running — all 24 pipelines
   recreate, per-material raster state preserved (P5's check at 24-pipeline
   scale).
8. Clean exit via real window close, **no VMA leak report** (`timeout`'s
   SIGTERM skips Drop — the leak check needs a manual close). [Superseded
   2026-07-29: the parenthetical is wrong; `timeout`'s SIGTERM becomes
   `SDL_QUIT` and `Drop` runs, so no manual close is needed. See
   `build_reproducibility.md` §7.4.]

## Verification (exit checklist)

- [x] ~~Step 1 smoke test executed~~ superseded by the vec4-array mini-phase;
      risk #4 + §3 already reconciled; no smoke-test residue
- [x] `just shaders` green; `just test` green; churn = toon_link additions only
- [x] `just lint`, `cargo build --examples` clean
- [x] toon_link bails helpfully without assets (exercised by renaming the
      converted dir); loads via manifest counts (not hardcoded);
      tiling/range asserts in place
- [x] 24 material pipelines, one shared mesh, 24 index-range draws in INF1
      batch order
- [x] Correctly shaped Link, smooth normal gradients; silhouette recorded
      (noclip side-by-side deferred to P7 — see Recorded facts)
- [x] Debug modes + batch isolation work; all 24 batches identified
- [x] z_test=false decals mapped to Always/no-write; behavior recorded
- [x] Winding settled: **flipped in the converter**; per-material cull on,
      nothing missing over a full orbit; single-line sha256 update;
      `just link-verify-p3` green
- [x] Validation sweep clean (16/16)
- [x] Hot reload preserves per-material raster state across 24 pipelines
- [x] No VMA leak on real-close exit
- [x] No changes to templates, existing examples, `Cargo.toml`, or oracle
      scripts
- [x] Master plan §6 P6 row ✅ + hash; risks #3/#4 updated
- [x] Recorded facts filled in

## Recorded facts (fill in after gates pass)

```
commit:                   9508563

smoke test result:        superseded by the vec4-array mini-phase (0d08a7d);
                          not executed, no residue

winding outcome:          flipped-in-converter. Under manifest cull the model was
                          uniformly inside-out (eye decal + face interior visible
                          through the back of the head in profile; shield visible
                          through the chest from the front) — no mixed/partial
                          batches, consistent with P3's uniform Blender read.
                          Fix in output.rs::build: chunks_exact(3) → [t0, t2, t1]
                          while concatenating per-shape lists; pose.rs untouched.
                          sha256 link.idx.bin 56deed19… → 753489b9…, only that
                          line. just link-verify-p3 green post-flip (71 converter
                          unit tests + canonical geometry table zero-diff + file
                          invariant checks: invBind residual 1.45e-2, weighted
                          dist 7.73e-3 — unchanged). Blender re-check not
                          performed (screenshots decisive).

silhouette vs noclip:     front, left profile, direct back, back-right + two
                          bring-up angles captured (XWayland `import` grabs).
                          Verdict: unmistakably Toon Link — head ~1/3 of height,
                          long trailing hat, sideburns, pointed ears, tunic,
                          belt buckle (podA), boots; T-pose. Live noclip
                          side-by-side not performed (no browser in the loop);
                          revisit alongside P7's per-feature UV checks.

normal-gradient reading:  smooth across all curved surfaces (head, arms, legs,
                          hat); hard transitions only at real creases (hat brim,
                          hair spikes, tunic hem). No patchwork seams → normal
                          bake + rotateDirection path is correct.

per-batch isolation map:  all 24 as expected, stepped E ×25 (wraps to 0) via an
                          XTEST driver; stdout names matched the manifest
                          (ear/ear(2..8) shared-record naming quirk included;
                          eye/mayu decals are the 36-index micro-batches,
                          podA = belt buckle, 72).

z_test=false decal note:  the 8 eye/brow decals (eyeL/R, eyeL/RdamA/B → but
                          drawn: eye + mayu families) under Always/no-write draw
                          over whatever is behind them when not culled: under
                          bring-up cull None the eye quad showed through the back
                          of the head (predicted by plan risk #4); with
                          per-material Back cull they only appear with the face,
                          reading as flat quads floating on the head in normals
                          mode. Nothing louder than expected; full depth/alpha
                          treatment lands in P7.

sweep:                    16/16 examples clean (incl. toon_link; grep
                          validation|VUID over `timeout 3 just dev`)

hot reload / VMA:         fragment-body edit (dim ×0.25) recompiled and applied
                          live across all 24 pipelines, per-material cull
                          preserved (back view stayed solid); revert clean.
                          Real window close (WM_DELETE via Xlib) → exit 0, no
                          VMA leak report.

deviations discovered:    1. eyeball + key gates were driven programmatically
                          (SDL_VIDEODRIVER=x11 + python-Xlib XTEST + ImageMagick
                          import) instead of by hand — screenshots reviewed.
                          2. none in code: plan executed as written (generated
                          params carry an explicit `_padding_0: [u8; 12]` tail,
                          minor construction detail).
```

## Out of scope for P6

- **Textures** (P7): no `create_texture_with_options`, no `Sampler2D`, no
  dummy white texture yet.
- **TEV, lighting, ramps, gamma** (P8) — the P8 *data path* is settled
  (flat uniform arrays per master plan §3, via the vec4-array mini-phase),
  implementation is not.
- **Remaining per-material state** (P7): blend modes, alpha-compare, exact
  `Less_Equal` depth mapping, `pe_mode` opaque-before-translucent ordering
  (meaningless until blending exists). P6 maps only cull + the z_test flag.
- `BlendMode::DstAlpha`, eye write-mask multi-pass (P9, master plan §4.5).
- Runtime skinning, BCK poses, `--casual` (P9).
- ~~Codegen array support — rejected in Step 1's decision~~ — landed after
  all as the vec4-array mini-phase (`0d08a7d`,
  [`vec4_array_support.md`](vec4_array_support.md)).

## Risks / open questions

1. ~~**`uint` uniform field codegen is assumed, `uint4` is not.**~~ —
   resolved: the vec4-array mini-phase landed bare `uint4`/`int4` vector
   fields with committed test-shader coverage (std140_arrays' `flags`/`bias`)
   and a runtime proof. v0 still uses scalar `uint debugMode` (simplest fit),
   just no longer out of caution.
2. **Winding vs cull-mode mapping is one decision, not two.** Flipping
   triangle order and swapping Back↔Front are equivalent fixes; doing both
   re-breaks it. Prescription: fix winding **in the converter** (Step 5),
   keep the cull mapping literal (`Cull_Back → Back`). The full-orbit
   no-missing-parts check is the guard.
3. **Silhouette comparison is subjective.** Mitigate with 2–3 canonical
   angles, outline/proportions only; decisive per-feature comparisons come
   with P7 (textures) and P8 (TEV vs noclip + Dolphin golden frames).
4. **`Always`/no-write decals draw over everything they aren't culled
   behind.** With per-material Back cull this approximates GX (decals cull
   away when the head faces away), but under Step 4's bring-up
   `CULL_OVERRIDE = Some(None)` the eyes/brows will show through the back of
   the head — expected, not a bug; don't chase it during bring-up.
5. **pe_mode Translucent (12 materials) drawn Opaque** — acceptable P6
   artifacts on eye/brow/mouth regions; note anything loud for P7.
6. **Assets are machine-local.** CI and other machines bail on toon_link
   (no assets); the sweep line for toon_link only means something where
   `just convert-link` has run.
7. **Hot-reload interface panic**: `ToonLinkParams` shape edits while
   running panic by design; body edits are safe (comment at the struct).
