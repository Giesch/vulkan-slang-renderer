# Multi-Draw Rendering: Ordered Draws, Bindless Textures, Per-Draw Uniforms

> **STATUS: DESIGN.** Extends `04_design.md`'s `.rendering()` section from a single
> terminal draw into an ordered multi-draw list, and moves textures and per-draw data out
> of pipeline identity so pipeline count collapses. Written 2026-07 against the post-BDA,
> post-multi-draw-queue (P4/P5) renderer.
>
> **Relaxes** `04_design.md` §2's "**v1 constraint:** … exactly one terminal draw in
> rendering" and supersedes the single-draw framing of the rendering section there. The
> compute/simulation half of 04 is untouched.
>
> **Depends on:** 04 Phase 2 (core graph) for the `.rendering()` builder substrate; a
> descriptor-indexing device-feature enable (§6); and real push-constant support (§5) —
> both new renderer work, deliberately accepted for a nicer API.

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
