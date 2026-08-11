# Multi-Draw Rendering: Ordered Draws, Bindless Textures, Per-Draw Uniforms

> **STATUS: DESIGN.** Extends `04_design.md`'s `.rendering()` section from a single
> terminal draw into an ordered multi-draw list, and moves textures and per-draw data out
> of pipeline identity so pipeline count collapses. Written 2026-07 against the post-BDA,
> post-multi-draw-queue (P4/P5) renderer.
>
> **Amended 2026-07-28** by
> [../remove_pipelined_compute.md](../remove_pipelined_compute.md). Ring
> arithmetic in §13 predates the collapse: `PRE_WAIT_RING_LEN` is gone, so
> the BDA handle ring is `M = MAX_FRAMES_IN_FLIGHT = 2` slots indexed by
> `flight_slot`, and there is only one command stream. The §13.2 formulas
> themselves still hold — see the inline notes there, §13.4 and §13.5.
>
> **Relaxes** `04_design.md` §2's "**v1 constraint:** … exactly one terminal draw in
> rendering" and supersedes the single-draw framing of the rendering section there. The
> compute/simulation half of 04 is untouched.
>
> **Depends on:** 04 Phase 2 (core graph) for the `.rendering()` builder substrate; a
> descriptor-indexing device-feature enable (§6); and real push-constant support (§5) —
> both new renderer work, deliberately accepted for a nicer API.
>
> **§§1–12 are not settled.** A 2026-07 design review found eight synchronization holes
> across 04 and this doc — a wrong slot-count formula, a runtime-variable quantity that
> several build-time precomputations assume static, and a silently-deleted hazard-tracking
> fallback. Read **§13** before implementing any phase.

## Motivation — what 04 leaves unsolved

`04_design.md` is a compute-simulation scheduler. Its parity groups, ping-pong rotations,
variant enumeration, cross-frame modes and BDA declarations all exist to order compute
dispatches and feed **exactly one terminal graphics draw** (04 §1, §2; `frame_inputs_api.md`
calls the single-terminal-submit shape "load-bearing"). It says nothing about drawing a
scene.

But a scene is what the renderer's newer multi-draw queue (P4) is for. `examples/multi_mesh.rs`
and the planned `examples/toon_link.rs` both draw **N batches over one shared mesh, one
pipeline per material**, and both must hand-roll a `usize`-indexed draw table because
neither the graph nor the low-level queue offers anything better. Three coupled costs, all
paid today as application bookkeeping:

1. **Draws referenced by `usize`.** `PipelineHandle` is not `Clone`/`Copy` and its `index()`
   is `pub(crate)` (`src/renderer/pipeline.rs:92-99`), so an app *cannot* get an id from a
   handle. It keeps `P_* : usize` consts and parallel `Vec`s
   (`examples/multi_mesh.rs:71-88`, `DRAWS` at `285-308`); `toon_link` §5 will store
   `Vec<(PipelineHandle, UniformBufferHandle, Params)>` + a batch list and loop the same
   way. Draw order is load-bearing (translucency, depth-write) but enforced only by comment
   (`multi_mesh.rs:16-20`).
2. **Textures bound per-pipeline.** Materials differing *only* by texture still need
   separate pipelines — the `multi_mesh` wrap/filter grid panels, and much of `toon_link`'s
   11 material configs.
3. **Uniforms per-pipeline, not per-draw.** Distinctly-transformed objects each need their
   own pipeline — `multi_mesh`'s per-`(shape, color)` explosion (17 pipelines at P5). The
   example header (`multi_mesh.rs:9-14`) names this "this renderer's no-per-draw-uniform
   design."

This document extends the render graph to fix all three.

---

## 1. The unifying idea: a per-draw resource table in a push constant

The three costs collapse into **one** mechanism.

The renderer already stashes storage-buffer pointers as `Addr`-family fields *inside* the
per-pipeline `Params` uniform (`sprite_batch.rs:25-36`; `src/renderer/addr.rs`). This design
moves those fields — plus **Slang bindless texture handles**, which are the texture analog
of a BDA — out of the per-pipeline uniform and into a **per-draw push constant**. The push
constant becomes the draw's whole **resource table**: a handful of BDAs (8 B each) and
bindless handles (4 B each). Everything the draw touches is *referenced*, nothing is
*bound*.

This fits inside the 128 B `maxPushConstantsSize` floor precisely because you push
**references, not payloads**: a BDA points at arbitrarily large data; a bindless handle
indexes an arbitrarily large texture array. Consequences:

- **Readable in all stages.** Unlike `gl_BaseInstance` (a vertex-only builtin), a push
  constant is visible in the fragment shader, where material/TEV data and texture handles
  are actually used — no vertex→fragment flat-varying plumbing.
- **`firstInstance` stays free** for real hardware instancing later.
- **Static data uploads once.** Bulk per-material data can live in an *immutable* buffer
  written at setup; only its 8 B pointer is per-draw. A static model re-uploads nothing per
  frame, versus rewriting a whole per-draw array every frame.
- **Self-documenting.** A push block of named references beats a "first-instance-means-draw-
  id" convention.

**Pipeline identity therefore shrinks to `shader + vertex layout + raster state`.** A
pipeline binds no per-material textures and no per-material uniform buffer; per-material
variation is entirely in the push block. Taken to its conclusion, even frame-global data
(view/proj) can be a BDA in the push block, leaving pipelines to bind **nothing** but the
one global bindless descriptor set.

> An earlier draft delivered per-draw data via base-instance indexing into a `DrawParams[]`
> buffer. It is **dropped**: base-instance is vertex-only (forces VS→FS plumbing for the
> fragment-stage material data that dominates a TEV renderer), overloads `firstInstance`,
> and re-uploads the array every frame. Pushing BDAs/handles addresses all three.

---

## 2. The single-terminal-submit reconciliation

`frame_inputs_api.md` §6 states as a deliberate, load-bearing constraint:

> **One graphics draw per frame stays.** The terminal draw call takes `self` by value and
> performs acquire + timeline wait + record + submit; this single-terminal structure is
> load-bearing for the "frame_inputs writes are always pre-wait" ring argument.

Ordered multi-draw does **not** violate this. The load-bearing unit is the single terminal
**submit** — one `self`-consuming call, one pre-wait CPU-write window — not a single draw.
The renderer already backs one submit with a multi-draw queue: N
`queue_draw_index_range(&PipelineHandle<DrawIndexed>, first, count)` calls accumulate
`PendingDrawCommand`s, then one `submit_draws(self, gpu_update)` records them all inside the
single `cmd_begin_rendering`/`cmd_end_rendering` and submits once
(`src/renderer.rs:5569-5619`, record loop `1800-1892`).

So an ordered list of draw nodes lowers to: N `queue_draw_*` + one `submit_draws`, whose
`gpu_update` closure fills every draw's push block and mints every referenced address in the
one pre-wait window. The ring argument is unchanged.

---

## 3. Ordered draw nodes + mesh sections

The `.rendering()` section becomes an **ordered list of draw-node declarations**.
Declaration order = record order = painter's order. Nodes are referenced by typed handle,
never by `usize`, exactly like 04's compute nodes.

### Mesh sections

A shared mesh is carved into named, typed ranges so the app stops maintaining running-sum
`first_index` and coverage asserts (`multi_mesh.rs:314-325`):

```rust
let mesh = gb.mesh(&vertices, &indices)?;             // MeshHandle<Vertex>
// contiguous sections, derived from a batch list; the graph runs the running sum
let [cube, pyramid, disc] = mesh.sections([18, 18, 54]);   // [MeshSection<Vertex>; 3]
// or, driven by toon_link's manifest `batches` (each already carries first_index+count):
let sections = mesh.sections_from(&manifest.batches);
```

`MeshSection<V>` is a `{ mesh: MeshIndex, first_index: u32, index_count: u32 }` the graph
validates for in-bounds, contiguity and full coverage at build — the invariant
`multi_mesh.rs` asserts by hand.

### Draw nodes

```rust
gb.rendering(|r| {
    // pipeline = shader + vertex layout + raster state only (§1)
    let opaque   = r.pipeline(shaders.material, RasterState::default());
    let cull_front = r.pipeline(shaders.material, RasterState { cull: Front, ..default() });

    // one draw node per batch, in painter's order; each fills its own push block (§5)
    r.draw(&opaque, &cube).push(|d| MaterialDraw {
        model:    d.mat4(cube_model),
        material: d.immutable(&cube_material),   // ImmutableAddr — static, uploaded once
        albedo:   d.texture(&white),             // bindless handle (§6)
    });
    r.draw(&cull_front, &second_cube).push(|d| MaterialDraw { /* … */ });
    // … remaining batches, order preserved …
});
```

- A **draw node** is `(pipeline, section, push-block closure)`. `r.draw(...)` returns a
  typed node handle the app may store for execute-time updates (mirrors 04 §7's
  `run.uniforms(&node, …)`).
- **Order is the declaration order** and is the only ordering the rendering section owns —
  it is *record order*, not a barrier schedule (§7). The graph replays it onto the queue.
- At execute the graph issues one `queue_draw_index_range` per node (pushing that node's
  block first, §5) and one terminal `submit_draws`.

---

## 4. Per-draw resource table (push constants + BDAs)

### The push block

Each graphics shader declares one `[vk::push_constant]` struct — its per-draw resource
table. The `Addr`-family fields that live in a `Params` uniform today are simply *promoted*
to this block:

```slang
struct MaterialDraw {
    float4x4                     model;      // 64 B — one inline matrix is the budget's big item
    ImmutableAddr<MaterialData>  material;   //  8 B — points at the bulk TEV/material blob
    Texture2D.Handle             albedo;     //  4 B — bindless (§6)
    Texture2D.Handle             ramp;       //  4 B
    // … ≤128 B total; anything larger goes behind another BDA …
};
[vk::push_constant] MaterialDraw draw;
```

Read directly, in any stage:

```slang
MaterialData m = draw.material.load();          // fragment reads it — no VS→FS plumbing
float4 base   = draw.albedo.Sample(uv);         // bindless
```

### Discipline, not limit

The 128 B floor is respected by pushing **references**. Budget ≈ one inline `float4x4`
(64 B) + ~8 references (4–8 B each). If a draw needs two matrices inline it overflows —
put the second behind a BDA, or a per-object transform buffer. Codegen enforces the
worst-case block size at generation time and fails loudly if a shader's push struct exceeds
the floor.

Worked worst case (`toon_link`): `ImmutableAddr<MaterialData>` (8) + 4 `Texture2D.Handle`
(16) + one inline `float4x4` model (64) = **88 B**. Comfortable. Link is one rigid model,
so the transform could instead be frame-global (a single BDA), dropping the block to ~28 B.

### What stays elsewhere

- **Frame-global data** (view, projection, light context) is the same for every draw. Keep
  it in a small per-pipeline `Params` uniform *or* push a single frame-global BDA. Either is
  fine; a frame-global BDA is the cleaner end state (pipelines then bind nothing but the
  bindless set).
- **Bulk per-material data** (TEV stages, konst/register colors, texgens) lives behind the
  block's `Addr`. Static → `ImmutableAddr`, written once at setup via `write_immutable`
  (`sprite_batch.rs:144` is the existing pattern); dynamic → `Addr`/`ReadAddr` minted per
  frame in the submit closure.

### Renderer additions

The push-constant path is reflected and plumbed into the pipeline layout already
(`src/shaders/json/pipeline_builders.rs:12,36`; `pipeline_layout.rs:44-66`;
`renderer.rs:5330-5337`), but is **completely dead** — no `.slang` declares one, there is no
`cmd_push_constants` call, and no `Gpu` API. This design makes it live:

- **Record loop** (`renderer.rs:1800-1892`): emit `cmd_push_constants` for each
  `PendingDrawCommand` before its `cmd_draw_indexed`, from bytes the queue carried.
- **Queue + `Gpu` API**: `queue_draw_index_range` (or the graph's draw-node execution)
  carries the per-draw block bytes; the submit closure fills them and mints any referenced
  addresses (`Gpu::addr` / `current_addr` / `current_immutable_addr`,
  `renderer.rs:5398-5436`) in the pre-wait window.
- **Codegen** (§9): emit the `#[repr(C)]` push-block struct alongside `Params`, with the
  same std430 layout asserts the BDA fields already use.

### Why not base-instance or dynamic UBO

- **Base-instance** (`gl_BaseInstance` + a `DrawParams[]` buffer): vertex-only, overloads
  `firstInstance`, re-uploads every frame. See §1's note.
- **`UNIFORM_BUFFER_DYNAMIC`** (per-draw dynamic offset): all-stage and large-payload
  capable, but reintroduces a per-draw *descriptor rebind* and cuts against this renderer's
  deliberate all-BDA, shrink-the-descriptor-set direction (storage buffers were removed from
  descriptors entirely, `pipeline_layout.rs:329-332`). Set aside.

---

## 5. Bindless textures (Slang handles)

Textures become references too, so materials that differ only by texture stop forcing
pipeline variants.

### Model

- **One global bindless descriptor set**: a large `COMBINED_IMAGE_SAMPLER textures[]` array,
  `PARTIALLY_BOUND` + `UPDATE_AFTER_BIND`, owned by the renderer (or the graph on its
  behalf). Combined image-samplers are the least-invasive retrofit — each `Texture` already
  carries its own sampler (`src/renderer/texture.rs:55-65`).
- **`create_texture*` yields a stable bindless slot** (a `u32`) in addition to the existing
  `TextureHandle`. The slot goes into the push block; the descriptor array is written
  (update-after-bind) as textures are created.
- **Slang side**: a texture is a bindless handle type (`Texture2D.Handle` /
  `DescriptorHandle<Texture2D>`), a 4 B index Slang resolves against the global array. The
  shader samples `draw.albedo.Sample(uv)` with no per-pipeline `Sampler2D` binding.

### Prerequisite: device features

Descriptor indexing is **not enabled today** (no `descriptor_indexing`,
`runtimeDescriptorArray`, or `VK_EXT_descriptor_indexing` anywhere). Add the Vulkan 1.2 core
bits to the existing `vulkan_12_features` builder (`renderer.rs:3373`):
`descriptorIndexing`, `runtimeDescriptorArray`, `shaderSampledImageArrayNonUniformIndexing`,
plus the `DescriptorBindingFlags` `UPDATE_AFTER_BIND_BIT` / `PARTIALLY_BOUND_BIT` on the
global array binding. This is the renderer's first `descriptor_count > 1` binding
(`pipeline_layout.rs` emits count 1 everywhere today).

### Codegen

Reflection-based: a texture field in a shader's parameter block becomes a **handle field**
in the push block (a `u32` bindless slot), not a per-pipeline `Sampler2D` descriptor. This
is the open todo "support bindless textures using slang handles" (`todo.org:59`) — see the
spike in §12.

---

## 6. Hazard tracking & ordering

Extends `04_design.md` §6 (barriers) and §8 (cross-frame reads).

- **Draw order = record order.** Graphics draws in one render pass share attachments and
  need no barriers between them; blend and depth resolve overlap. The graph records nodes in
  declaration order and inserts nothing — distinct from the compute→compute / compute→
  graphics barrier ordering 04 §6 owns.
- **Rendering dependencies are declared, not inferred.** With no per-draw descriptors, 04
  §6's "read the pipeline's `texture_handles` list" analysis sees nothing: BDAs and bindless
  handles in a push constant are invisible to binding-based tracking — the same reason BDA
  storage buffers forced 04 decision 2. So 04 §5's handle-declaration becomes the *primary*
  dependency source for the rendering section, not a supplement.
- **The common case is free.** Immutable material data and static textures are written once
  and read-only on the GPU; they declare nothing and participate in no hazard.
- **Cross-frame sim reads still apply.** A draw sampling *simulation output* — a ping-pong
  texture by bindless handle, or a gpu-only buffer by BDA — must declare it, and 04 §8's
  `CrossFrameMode` (`ExtraSlot` / `SyncWait` / `unsynchronized()`) governs which slot it
  reads and what wait is emitted. Bindless changes only how the texture is *addressed* (an
  index vs a descriptor), not the cross-frame analysis.

---

## 7. What collapses

### `multi_mesh` (validating testbed)

Today at P5: 17 pipelines, 18 draws, 7 textures, plus `DRAWS`/`P_*`/running-sum/coverage
asserts (`link_rendering/phase_05.md` Recorded facts). Under this design:

- **Pipelines** drop to *one per distinct `RasterState`* — the shapes and grid panels share
  a shader and default state; only the cull-front, blend-opaque, and depth-write-off panels
  genuinely need distinct pipeline state. Roughly a handful instead of 17.
- **Per-`(shape, color)` and per-texture variation** move into push blocks: each shape's
  model + tint + bindless texture is a draw node over a shared pipeline.
- **`DRAWS`, the `P_*` consts, the running sum, and the const coverage asserts are deleted**
  — sections carry the ranges; declaration order carries the painter's order.

### `toon_link` (real scene)

24 batches → 11 material configs. The materials run one data-driven TEV-interpreter shader
(`tev.slang`), differing only in *data*, so:

- **Pipelines** = the distinct `RasterState`s among the 11 (blend/cull/depth/alpha-compare),
  not one per material.
- **24 batches** = 24 draw nodes over shared pipelines; the manifest's `batches` array
  (`link_rendering.md:310`, each `{material, first_index, index_count}`) drives sections
  directly — no hand-rolled queue loop (`link_rendering.md:595-596`).
- **Material data** sits behind an `ImmutableAddr<MaterialData>` written once at setup;
  **textures** are bindless. Per draw, the push block is the material pointer + up to 4
  texture handles (`"texmaps": [.., .., null, null]`, ≤4 used) + the shared transform — the
  88 B worst case of §4.

---

## 8. Codegen changes

`src/shaders/build_tasks.rs` + `templates/`:

- Emit the `[vk::push_constant]` push-block `#[repr(C)]` struct per graphics shader, with
  the std430 `size_of`/`offset_of` layout asserts the `Addr` fields already generate
  (`build_tasks.rs:897-899, 1186-1217`), and a compile-time assert that the block ≤ 128 B.
- A texture field becomes a bindless **handle** field in the push block, not a per-pipeline
  `Sampler2D` binding — this removes textures from `PipelineConfig.texture_handles`.
- Generated `pipeline_config()` no longer bakes textures/uniform pointers into pipeline
  identity; it produces the (smaller) pipeline plus the push-block type the draw node fills.

Snapshot churn is expected across `src/generated/shader_atlas/` and the `generated_files`
insta snapshots; gate with `just shaders` + `just test`.

---

## 9. Implementation phases

Each independently landable, mirroring 04 §11.

- **Phase A — ordered draw nodes + mesh sections**, over the *existing* per-pipeline model.
  Deletes the `usize` draw table and moves range/coverage bookkeeping into `MeshSection`. No
  bindless, no push constants yet — pure ergonomic win. Port `multi_mesh`'s `DRAWS` loop as
  the smoke test.
- **Phase B — real push-constant support + per-draw BDA resource table.** Add
  `cmd_push_constants`, the `Gpu`/queue API, the `[vk::push_constant]` codegen; move `Addr`
  fields from `Params` into the push block. Per-draw uniforms without bindless yet
  (textures still per-pipeline).
- **Phase C — descriptor-indexing enable + global bindless set + Slang handles.** Gated by
  the §12 spike. Textures become bindless handle fields in the push block.
- **Phase D — fold texture + uniform out of pipeline identity.** Pipelines become
  shader + vertex layout + raster state; the pipeline-count collapse (§7) lands. Optionally
  push a frame-global BDA so pipelines bind nothing but the bindless set.
- **Phase E — (optional) dynamic raster state.** Promote cull / front-face / depth-test /
  depth-write / depth-compare to dynamic state (Vulkan 1.3 core, promoted
  `VK_EXT_extended_dynamic_state`), collapsing the last per-pipeline axis for those fields
  (blend-enable would need EDS3, not core).

Relationship to 04's phases: Phase A needs 04 Phase 2's `.rendering()` builder substrate.
Phases B–D are independent of 04's compute work and can proceed in parallel.

---

## 10. Migration

- **`multi_mesh` first** — the worked testbed. After Phase A its `DRAWS`/`P_*`/running-sum
  vanish; after Phase D its pipeline count collapses. Its existing raster-state and
  texture-option test objects (`phase_05.md`) keep proving the same renderer knobs, now over
  shared pipelines.
- **`toon_link` second** — the real validating scene (`link_rendering.md`). Its §5 draw loop
  becomes draw nodes over sections from the manifest; material data goes behind an
  `ImmutableAddr`; textures go bindless. This replaces the hand-rolled
  `Vec<(PipelineHandle, UniformBufferHandle, Params)>` + batch-list pattern it would
  otherwise inherit from `multi_mesh`.

---

## 11. Risks / open questions

- **Slang bindless-handle spike (lead risk).** The design leans on Slang's
  `Texture2D.Handle` / `DescriptorHandle<T>` lowering correctly to Vulkan descriptor
  indexing **and** surfacing in reflection so codegen can emit the right field type and the
  global-set binding. This is the genuine unknown (open todo `todo.org:59`). **Do a small
  spike before committing §5/§6 to a specific Slang idiom:** one shader sampling
  `textures[handle]` from a global array, the handle passed in a push constant, verified
  end-to-end (compile → reflect → render). Everything downstream (codegen, `toon_link`'s
  material path) assumes this works.
- **Push-constant size is a discipline.** It holds only because you push references. Codegen
  must fail loudly when a shader's push block exceeds 128 B, and the API should make "push a
  BDA" the easy path for anything large.
- **Hazard tracking is now fully declaration-driven** for the rendering section (§6). This
  is consistent with 04's direction but removes the automatic binding-based fallback — a
  forgotten declaration for a sim-output read is a silent race, so the graph's build-time
  checks (04 §5) must be thorough.
- **Global-set lifetime / update-after-bind.** Textures created after pipelines must be
  reflected into the global array without invalidating recorded command buffers
  (update-after-bind allows this) and without freeing slots still referenced by an in-flight
  push block. Slot lifetime is renderer-owned, freed at teardown like `TextureStorage`
  today.
- **Base-instance conflict avoided.** Because per-draw data rides push constants,
  `firstInstance` is free — if real instancing with per-instance data is wanted later,
  `gl_InstanceIndex` behaves normally. Note this so no one re-introduces the base-instance
  trick.
- **04's compute path stays untouched.** Compute nodes keep their per-pipeline `Params`
  uniform + descriptor-bound storage textures; this document changes only the rendering
  section.

---

## 12. Relationship to other docs

- **`04_design.md`** — this extends its rendering section and reuses its node model, handle
  declarations (§5), and cross-frame modes (§8). It relaxes 04 §2's single-terminal-draw v1
  constraint.
- **`03_bindless.md`** — background on Vulkan descriptor indexing; that doc marked bindless
  "not planned" for *buffers* (the renderer chose BDA). This design adopts bindless for
  *textures* specifically, which BDA does not cover.
- **`frame_inputs_api.md`** — §2 above shows ordered multi-draw preserves its load-bearing
  single-terminal-submit property; the eventual `FrameInputs` migration and this design are
  compatible (both keep one `submit`-shaped terminal).
- **`link_rendering.md`** — `toon_link` is the validating real scene (§7, §10); its manifest
  `batches`/`materials`/`textures` map directly onto sections, immutable material BDAs, and
  bindless handles.

---

## 13. Open synchronization holes (design review)

> Recorded 2026-07 against the renderer at commit `fd06085`. **Most of these belong to
> `04_design.md`, not to this doc** — they are collected here because this is the newest
> document and no phase of either design should start without them. Each entry back-
> references `04 §N` where it applies. Nothing here is fixed; all of it is owed.

Ordered by severity. Sub-numbers are stable references — cite them, don't renumber.

### 13.1 Buffers are excluded from graph-managed rotation

**Claim:** 04's parity groups and rotations make ping-pong the graph's problem, not the app's.

**Reality:** `GraphPingPong` (04 §2) covers storage *textures* only. 04 §5 leaves BDA buffers
on the app-declared `graph::Write::{Storage, Current, Previous}` enum over the existing
3-slot handles, minting "via the existing `Gpu` methods". Three consequences:

- `StorageBufferHandle` still mints `Addr<T>` — GPU-**writable** — over an allocation that
  rotates every frame (`Gpu::addr`, `src/renderer.rs:5401`; `create_storage_buffers_per_frame`,
  `:882-916`). A compute shader that writes through it cannot see those writes next frame:
  frame N+1 binds a physically different `VkBuffer` holding whatever was there
  `MAX_FRAMES_IN_FLIGHT` executes ago (`PRE_WAIT_RING_LEN` before 2026-07-28 — the
  ring shrank from 3 to 2, so the staleness is nearer but no less wrong). Only `GpuOnlyBufferHandle` + `previous_addr` (`:5423`) escapes this, and only
  for strict full-rewrite ping-pong.
- **Hazard-identity mismatch (the dangerous one).** 04 §6 keys the last-writer table on the
  *handle*; the actual memory identity is `(handle, flight_slot)` (`ring_slot` before
  2026-07-28). A write recorded at slot N
  and a read at slot N+1 look *ordered* to the graph while touching different allocations —
  so the graph will certify a dependency that does not exist. That is strictly worse than
  today, where the app at least knows it is hand-rolling.
- No `Persistent` variant. In-place GPU-owned state — accumulators, atomic counters,
  append/free lists, spatial hashes — stays unexpressible. 05 §4's "static material
  data" used to be listed here too, paying 2x memory (not the 3x this line claimed —
  stale from before the ring shrank to 2, as noted a few bullets up) and a per-frame
  address mint for data that never changes. **Phase 7d landed that half**; see below.

**Owed:** a non-ringed, GPU-owned buffer resource — one allocation, stable address, seeded at
setup. This needs *no new synchronization*: consecutive frames' compute is ordered by the
barrier at the top of each command buffer (was `compute_timeline` before 2026-07-28 —
a stronger guarantee, so the conclusion is unchanged), so the ring is the only thing
preventing it. Alternatively extend rotation to
buffers so the graph owns it. Either way, hazard-table identity must become per-slot.

**Half done as of Phase 7d (2026-08-10).** `SingletonBufferHandle` is the CPU-uploaded,
GPU-read-only case: one allocation, stable address, seeded at creation, minting the same
`ImmutableAddr<T>` shaders already see. It sidesteps the hazard-identity problem entirely
rather than solving it — nothing on the GPU writes it, so it has no last-writer entry to
key. **The `Persistent` variant above — GPU-*written* in place — is still owed**, and it is
the half that forces hazard-table identity to become per-slot.

### 13.2 `ExtraSlot` sizing is wrong past one advance per frame

**Claim:** 04 §2 and §8 — "3 slots at one advance/frame, 4 at two".

**Reality:** let `R` = slots, `A` = advances per execute, `M` = `MAX_FRAMES_IN_FLIGHT`.
Frame N+2 submits only after `frame_timeline >= N` (host wait, `renderer.rs:2221-2226`), so
graphics N has retired before compute N+2 is ever submitted. The writers that can overlap
graphics N are therefore compute N and compute N+1 — and compute N only in `PreviousFrame`
mode, since `SameFrame` orders it ahead.

- `PreviousFrame` — graphics N reads `base_N - 1`; overlapping writers cover
  `base_N .. base_N + 2A - 1`. Need **`R >= A*M + 1`**.
- `SameFrame` / `SyncWait` — graphics N reads `base_N + A - 1`; only compute N+1 overlaps.
  Need **`R >= A*(M-1) + 1`**.

| A | `PreviousFrame` | 04 says | `SameFrame` | 04 implies |
|---|---|---|---|---|
| 1 | 3 | 3 — ok | 2 | 2 — ok |
| 2 | **5** | 4 — **wrong** | **3** | "no memory cost" — **wrong** |

Two things the design never states and must: every hardcoded 3/4 silently assumes `M = 2`, so
raising `MAX_FRAMES_IN_FLIGHT` invalidates the whole document; and the old
`PRE_WAIT_RING_LEN = M + 1` was simply the `A = 1, PreviousFrame` instance of the same
formula.

> **2026-07-28:** the renderer is now unconditionally `SameFrame` — compute and graphics
> share one submit, ordered by a barrier — so the live row is `SameFrame`, `R >= A*(M-1) + 1`,
> which at `A = 1, M = 2` gives `R = 2`. That is exactly the collapse
> [../remove_pipelined_compute.md](../remove_pipelined_compute.md) performed:
> `PRE_WAIT_RING_LEN` deleted, one 2-slot ring. The `A = 2` cell still stands as a warning:
> a ping-pong advanced twice per execute needs 3 slots and would have to manage its own.

### 13.3 Advance count is runtime-variable; 04 §4/§6/§8 precompute against a static one

**Claim:** 04 §7 — a disabled node "skips its dispatch but keeps its barriers … every
precomputed downstream schedule stays valid regardless of enable state".

**Reality:** `.optional()` and `run.iterations()` both change `A`. Three build-time artifacts
depend on `A` being fixed:

- `reachable_states(g)` (04 §4) — a frame that advances fewer times lands on a base position
  no variant was ever instantiated for.
- The per-frame-start barrier schedules (04 §6).
- The `ExtraSlot` slot count (04 §8, and 13.2 above).

04 §10 already says the opposite of §7 — "a node that advances a rotation must still run its
copy when disabled, or the group must not advance". **Resolve in favor of §10.** Separately,
a disabled advancing node leaves its target slot holding content from `R` executes ago while
the last-writer table still credits it — the same class of bug as `previous_addr` with a
skipped dispatch in today's renderer.

**Watercolor hits this immediately**, so the validating example cannot be ported until it is
resolved: the brush is the `.optional()` node (`examples/watercolor.rs:945-950`) *and* it
advances `sim`.

**Owed:** a hard build-time rule that a group's per-frame advance count is enable- and
iteration-independent (either `.optional()` may not advance a rotation, or a disabled
advancing node compiles to a copy pass). For `repeat`, the loop group's advance count *is*
`iterations`, so downstream variant keys and barrier schedules must be indexed by
`iterations mod R` as well as by frame-start state — 04 §6's "simulate the node sequence once
per reachable frame-start state" is under-specified as written.

### 13.4 `SyncWait` is declared per-resource but costs the whole frame — **moot 2026-07-28**

> `SyncWait` is no longer a mode: it is the only behavior, and it costs nothing, because
> compute and graphics share one submit ordered by a barrier rather than by a submit-level
> semaphore. There is no pipelining left for it to serialize against. The critique below
> stands as reasoning about any future reintroduction of a per-resource sync knob.


04 §8 puts `CrossFrameMode` on the ping-pong, but `SyncWait` is implemented as a submit-level
semaphore wait (graphics N waits compute N). Selecting it for a single resource serializes the
entire `.simulation()` section against graphics — the exact overlap the section exists to
create. A knob whose table entry reads "no memory cost" while silently costing all pipelining
is a footgun.

**Owed:** move the sync mode to graph scope, or document the frame-wide effect in the §8 table
and have `build()` warn when `SyncWait` is mixed with `ExtraSlot` resources.

### 13.5 The frame stream is never analyzed — **the hazard was real; fixed 2026-07-28**

> This section identified the exact hazard that
> [../remove_pipelined_compute.md](../remove_pipelined_compute.md) Phase 3 had to solve, and
> it is no longer hypothetical: with pipelining gone, *all* compute is frame-stream compute.
> The fix is a barrier at the top of every command buffer whose first synchronization scope
> covers all commands earlier in submission order on the queue, which orders frame N+1's
> compute after frame N's compute *and* graphics — so `R = A*(M-1) + 1 = 2` holds without
> growing the ring. The "Owed" item below is discharged for the renderer; a graph would
> still owe the static check over its own declared writers.

04 §8 examines only the pipelined-queue boundary. After Phase 0.5
(`watercolor_race_fixes.md`), frame-stream compute for execute N+1 is recorded at the top of
graphics CB N+1 — a **separate submit** from graphics N on the same queue, with no semaphore
between them (the host wait at frame N+1 is only `frame_timeline >= N-1`). Vulkan permits
those submissions to overlap, so frame-stream compute N+1 can write a storage texture that
graphics N is still sampling. The bound is `A*(M-1) + 1`, as in 13.2.

Dormant under 04 v1 (no compute in `.rendering()`), and live the moment 04 §2's post-v1
relaxation lands. **Owed:** extend §8's static check to frame-stream writers and state the
frame-stream slot formula.

### 13.6 §8 of this doc deletes the automatic half of 04's hazard model

04 §6's headline property is "descriptor-bound images: fully automatic, zero codegen
changes", derived from the handle vectors `pipeline_config` already populates
(`src/renderer/pipeline.rs:139-141`), with `storage_texture_as_sampled` aliasing giving
storage↔sampled tracking for free (`renderer.rs:614-637`).

§8 above removes textures from `texture_handles` entirely. That path does not fail loudly —
it returns an empty handle list, which the analysis reads as "no hazards". §11 acknowledges
the added declaration burden but not that the automatic fallback is gone.

Also unreconciled at the API level: 04 §8's declaration verb for a rendering-section read of
simulation output is `cross_frame_sampled(&pp)`, which lives on a resources closure. §3's draw
nodes have no resources closure — only `.push(|d| …)` with `d.texture(&handle)`. **There is
currently no spelling for "this draw samples simulation output."**

**Owed:** a `d.cross_frame_texture(&pp)` (or equivalent) in §3/§4, and push-block codegen that
makes the *undeclared* spelling impossible rather than merely discouraged — a forgotten
declaration is a silent race with no build-time signal.

### 13.7 Bindless slot lifetime

§11's "Slot lifetime is renderer-owned, freed at teardown like `TextureStorage` today" is
wrong on both halves: `drop_texture` exists (`renderer.rs:569`), and egui frees textures
every frame through a `MAX_FRAMES_IN_FLIGHT`-deferred ring (`src/renderer/egui.rs:18, 54,
118`). A slot freed and recycled while an in-flight command buffer's push block still
references it samples the wrong image.

**Owed:** specify recycling deferred by at least `MAX_FRAMES_IN_FLIGHT` frames — the egui
`pending_free_textures` pattern is the model — or state that bindless slots are never
recycled and keep egui textures off the global array.

### 13.8 Smaller items

- **§4's "static data uploads once"** cites `sprite_batch.rs:144`, which in fact writes
  **every frame**. Uploading once requires `Renderer::write_immutable_all_frames`
  (`renderer.rs:930`) *and* still minting `current_immutable_addr` per ring slot each frame.
  A naive setup-time `write_immutable` seeds 1 of 3 slots — two bad frames at startup, then
  correct forever, which is exactly the failure mode nobody catches. Subsumed if 13.1's
  non-ringed buffer lands.
- **§6's "draws need no barriers between them"** holds for attachments, but not if a fragment
  shader writes a BDA that a later draw in the same list reads — and that dependency cannot be
  barriered inside a render pass. Owed: a build-time rule forbidding a rendering-section node
  from writing a BDA resource read by a later node in the same section.
- **Hot reload vs precomputed schedules.** 04 §6 precomputes barrier schedules at build;
  `check_for_shader_recompile` (`renderer.rs:2571`) rebuilds pipelines at runtime. A reloaded
  shader whose texture bindings changed leaves those schedules stale. 04 Phase 1 flags the
  pipeline-rebuild risk but not the hazard-reanalysis one.
- **The "dedicated compute queue" is same-family.** It is queue index 1 of the *graphics*
  family (`renderer.rs:325-327`), which is precisely why every barrier can use
  `QUEUE_FAMILY_IGNORED` and why buffers and images are `SharingMode::EXCLUSIVE` (`:3866`,
  `:4506`). Correct today, but both documents discuss cross-queue work as though the family
  choice were open. Record same-family as a graph precondition: a distinct compute family
  would require queue-family ownership transfers throughout.
