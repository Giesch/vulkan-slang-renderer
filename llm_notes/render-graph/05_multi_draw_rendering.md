# Multi-Draw Rendering: Ordered Draws, Bindless Textures, Per-Draw Uniforms

> **STATUS: DESIGN.** Extends `04_design.md`'s `.rendering()` section from a single
> terminal draw into an ordered multi-draw list, and moves textures and per-draw data out
> of pipeline identity so pipeline count collapses. Written 2026-07 against the post-BDA,
> post-multi-draw-queue (P4/P5) renderer.
>
> **Amended 2026-07-28** by
> [../remove_pipelined_compute.md](../archived/remove_pipelined_compute.md). Ring
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
> descriptor-indexing device-feature enable (§5, not §6 as this line first said); and real
> push-constant support (§4, not §5) — both new renderer work, deliberately accepted for a
> nicer API.
>
> **Amended 2026-08-14** against the bindless work recorded in
> [../bindless_textures.md](../bindless_textures.md). Both stated dependencies shipped:
> push constants are live end-to-end (§4) and textures are bindless (§5). They shipped at
> the **renderer** level, outside this document's graph — §9 Phases B and C are done and
> the graph itself is still unbuilt. Sections carrying corrections: §4, §5, §7, §8, §9,
> §11, §13.7, §13.8.
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
   is `pub(crate)` (`crates/renderer/src/renderer/pipeline.rs:105-122`; both still hold), so
   an app *cannot* get an id from a handle. It keeps `P_* : usize` consts and parallel `Vec`s
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
    Sampler2D.Handle             albedo;     //  8 B — bindless (§5)
    Sampler2D.Handle             ramp;       //  8 B
    // … ≤128 B total; anything larger goes behind another BDA …
};
[vk::push_constant] MaterialDraw draw;
```

Read directly, in any stage:

```slang
MaterialData m = draw.material.load();          // fragment reads it — no VS→FS plumbing
Sampler2D albedo = draw.albedo;                 // bindless; assign to convert the handle
float4 base   = albedo.Sample(uv);
```

**Corrected 2026-08-14.** A Slang descriptor handle is **8 B, not the 4 B this
block first said**, and the heap's one binding is a *combined* image sampler, so
the type is `Sampler2D.Handle` rather than `Texture2D.Handle`
(`assert!(size_of::<BindlessHandle<Sampler2D>>() == 8)`,
`examples/toon_link/src/generated/shader_atlas/toon_link.rs:100`). The
assignment on the second line is how the shipped shader converts a handle to a
sampler (`examples/toon_link/shaders/source/toon_link.shader.slang:111-112`).

### Discipline, not limit

The 128 B floor is respected by pushing **references**. Budget ≈ one inline `float4x4`
(64 B) + ~8 references (4–8 B each). If a draw needs two matrices inline it overflows —
put the second behind a BDA, or a per-object transform buffer. Codegen enforces the
worst-case block size at generation time and fails loudly if a shader's push struct exceeds
the floor.

Worked worst case (`toon_link`): `ImmutableAddr<MaterialData>` (8) + 4 `Sampler2D.Handle`
(32, at 8 B each — this line first budgeted 16) + one inline `float4x4` model (64) =
**104 B**, not the 88 B first written. Still inside the 128 B floor. Link is one rigid
model, so the transform could instead be frame-global (a single BDA), dropping the block
to ~40 B.

**What `toon_link` actually pushes is 8 B**: one `ImmutableAddr<Material>` and nothing
else. The handles went *behind* that pointer into the std430 `Material`, and the transform
stayed frame-global in the param block, so this worst case never materialised. See §7.

### What stays elsewhere

- **Frame-global data** (view, projection, light context) is the same for every draw. Keep
  it in a small per-pipeline `Params` uniform *or* push a single frame-global BDA. Either is
  fine; a frame-global BDA is the cleaner end state (pipelines then bind nothing but the
  bindless set).
- **Bulk per-material data** (TEV stages, konst/register colors, texgens) lives behind the
  block's `Addr`. Static → `ImmutableAddr`; dynamic → `Addr`/`ReadAddr` minted per frame in
  the submit closure.

  **Corrected 2026-08-14.** The static case cited `write_immutable` with
  `sprite_batch.rs:144` as "written once at setup". That is the wrong pointer twice over:
  `sprite_batch` writes **every frame** (`examples/sprite_batch/src/main.rs:150`, and §13.8
  flags the same mistake), and `create_immutable_buffer` allocates per flight slot, so the
  address moves each frame. Write-once with a stable address is
  `Renderer::create_singleton_buffer` (`renderer.rs:1054`) plus `Gpu::singleton_addr`
  (`:5597`), which Phase 7d added for exactly this and which `toon_link` uses for its
  material table.

### Renderer additions

**Done 2026-08 (Phases 7–9 of `../bindless_textures.md`).** This section first said the
push-constant path was reflected and plumbed into the pipeline layout but "**completely
dead** — no `.slang` declares one, there is no `cmd_push_constants` call, and no `Gpu`
API". All three are false. The path is live end-to-end and all three bullets below landed
as written:

- **Record loop** (the `for pending_draw` loop, `renderer.rs:2086`):
  `cmd_push_constants` (`:2453`) is called at `:2160` for each
  `PendingDrawCommand::Draw`, from bytes the queue carried, between the heap bind (`:2153`)
  and the `cmd_draw*` match (`:2167`) — the position this bullet asks for.
- **Queue + `Gpu` API**: `queue_draw_index_range_with_push_constants` (`:5830`) and its
  siblings carry the per-draw block; `PendingDrawCommand::Draw` holds a `push_constants`
  payload. Addresses are minted through `Gpu::addr` / `current_addr` /
  `current_immutable_addr` / `singleton_addr` (`renderer.rs:5547-5604`).
- **Codegen** (§9): the `#[repr(C)]` push-block struct emits alongside `Params`, with the
  same std430 `size_of`/`offset_of` asserts the BDA fields use, plus a ≤128 B assert.
  `examples/toon_link/src/generated/shader_atlas/toon_link.rs:76-85` is the worked output.

Reflection lives at `crates/slang-reflection/src/json/pipeline_builders.rs:12,40`; the
`VkPipelineLayout` is built at `renderer.rs:5364-5377`. The `pipeline_layout.rs` this
section cited does not exist.

**One thing this design did not anticipate.** The payload type is tied to its own pipeline
in the type system: a `PipelineHandle` carries a push slot (`PushBlock<P>` or `NoPush`,
`crates/renderer/src/renderer/pipeline.rs:105`) that codegen fills from reflection. A draw
that supplies the wrong block, or supplies one to a pipeline that declares none, is a
**compile** error at the call site rather than a runtime check. `cmd_push_constants`
therefore validates nothing.

### Why not base-instance or dynamic UBO

- **Base-instance** (`gl_BaseInstance` + a `DrawParams[]` buffer): vertex-only, overloads
  `firstInstance`, re-uploads every frame. See §1's note.
- **`UNIFORM_BUFFER_DYNAMIC`** (per-draw dynamic offset): all-stage and large-payload
  capable, but reintroduces a per-draw *descriptor rebind* and cuts against this renderer's
  deliberate all-BDA, shrink-the-descriptor-set direction. Storage buffers are excluded from
  descriptors entirely — no `vk::DescriptorType::STORAGE_BUFFER` appears anywhere in the
  renderer (the `pipeline_layout.rs:329-332` this line cited does not exist). Set aside.

---

## 5. Bindless textures (Slang handles)

Textures become references too, so materials that differ only by texture stop forcing
pipeline variants.

### Model

- **One global bindless descriptor set**: a large `COMBINED_IMAGE_SAMPLER textures[]` array,
  `PARTIALLY_BOUND` + `UPDATE_AFTER_BIND`, owned by the renderer (or the graph on its
  behalf). Combined image-samplers are the least-invasive retrofit — each `Texture` already
  carries its own sampler (`crates/renderer/src/renderer/texture.rs:65-71`).
- **`create_texture*` yields a stable bindless slot** (a `u32`) in addition to the existing
  `TextureHandle`. The slot goes into the push block; the descriptor array is written
  (update-after-bind) as textures are created.
- **Slang side**: a texture is a bindless handle type (`Texture2D.Handle` /
  `DescriptorHandle<Texture2D>`), a 4 B index Slang resolves against the global array. The
  shader samples `draw.albedo.Sample(uv)` with no per-pipeline `Sampler2D` binding.

**Done 2026-08, with three differences from the above.**

- **The set is Slang's, not the renderer's to number.** `DescriptorHeap`
  (`crates/renderer/src/renderer/descriptor_heap.rs`) creates one 4096-slot
  `COMBINED_IMAGE_SAMPLER` binding at **binding 1** — a number Slang defines, not one this
  code picks (`:18,21,48-70`). The *set* index is likewise Slang's; reflection reports it
  per shader as `bindlessHeapSet` and `None` means the shader declares no handle. The
  binding flags are as written, plus `UPDATE_UNUSED_WHILE_PENDING`.
- **The handle is 8 B and typed**, not a raw 4 B `u32`. Apps and generated structs see
  `BindlessHandle<Sampler2D>`. `register_texture` (`renderer.rs:577`) claims the slot and
  stores it beside the texture, so a `TextureHandle` and its slot are minted together.
- **"The slot goes into the push block" is one option, not a rule.** A handle is ordinary
  struct data and lives wherever a std140/std430 layout can hold it. `toon_link` puts two
  handles in a std430 `Material` reached through the push block's `ImmutableAddr`;
  `depth_texture` puts one directly in the param uniform block. Neither puts a handle in a
  push block itself.

### Prerequisite: device features

**Enabled 2026-08.** This section said descriptor indexing was "not enabled today". The
`vulkan_12_features` builder (`renderer.rs:3780-3783`) now requests four bits:
`descriptorIndexing`, `runtimeDescriptorArray`, `descriptorBindingPartiallyBound` and
`descriptorBindingSampledImageUpdateAfterBind`. Device suitability also rejects any device
whose update-after-bind limits cannot hold 4096 descriptors (`undersized_limits`,
`:3543-3579`), and the binding flags are set on the heap binding as described. The heap is
the renderer's first `descriptor_count > 1` binding, and it is created outside the
per-pipeline layout path rather than by it.

**`shaderSampledImageArrayNonUniformIndexing` is deliberately *not* requested**, against
this section's list. That omission is the uniformity constraint the whole texture path
rests on: **a handle — and any index used to select the struct that carries it — must be
dynamically uniform within a draw.** Do not source a handle or its selecting index from
vertex or instance data, and do not pick between two handles per-invocation. Nothing
enforces this. Slang compiles divergent indexing without `NonUniformEXT` and without
complaint, validation cannot see it (it is data-dependent), and reflection sees
declarations rather than indexing expressions. The failure is wrong-texture rendering on
wave-scalarizing hardware while staying green everywhere else. Divergent indexing becomes
legal only when the feature bit and the `getDescriptorFromHandle` override land **together**
— the decoration without the feature is a validation error. §4's push block is how the
constraint is satisfied today: a push constant is per-draw constant by definition.

### Codegen

Reflection-based: a texture field in a shader's parameter block becomes a **handle field**
in the push block (a `u32` bindless slot), not a per-pipeline `Sampler2D` descriptor. This
is the open todo "support bindless textures using slang handles" (`todo.org:59`) — see the
spike in §12.

**Done 2026-08, and the rule is broader than "in the push block".** Any `Sampler2D.Handle`
field in any GPU-layout struct generates a `BindlessHandle<Sampler2D>` — param block, push
block or std430 pointee alike. The todo and the §11 spike both closed; the spike's answers
are in [../bindless_textures/phase_0_spike.md](../bindless_textures/phase_0_spike.md).

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

**Landed 2026-08 — measured 24 pipelines → 5 and 24 uniform buffers → 1.** Detail in
[../bindless_textures/phase_09.md](../bindless_textures/phase_09.md). Three corrections to
what this subsection predicted, below the original text.

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

Corrections:

- **24 materials → 5 raster states, not "the distinct `RasterState`s among the 11".** The
  count of *materials* is 24, and 11 is a count of texture-and-TEV configurations, which is
  the axis bindless removes. The 5 survivors are cull / depth-compare / depth-write / blend
  / color-write-mask groups, and none of those is descriptor state. Bindless removes the
  texture-driven pipeline explosion; the state-driven one survives.
- **`alpha-compare` is not a `RasterState` component.** It rides in the material data as a
  shader-side discard, so it adds no pipeline axis.
- **The push block is 8 B, not 88 B.** It holds one `ImmutableAddr<Material>`. The two
  handles sit in the std430 `Material` behind that pointer, and the transform stayed
  frame-global in the param block. The material table lives in a `SingletonBufferHandle`
  (§13.1), so its address is stable and needs no per-frame mint.

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

**Done 2026-08, with the second bullet qualified.** The push-block struct, its std430
asserts and the ≤128 B assert all emit. A shader that uses handles emits no texture
descriptors — but `PipelineConfig.texture_handles` still exists
(`crates/renderer/src/renderer/pipeline.rs:313`) and still serves shaders that bind
textures the old way. The removal is per-shader, not structural.

---

## 9. Implementation phases

Each independently landable, mirroring 04 §11.

> **Status 2026-08-14. Phases B and C are done; A, D and E are not.**
>
> They were delivered by [../bindless_textures.md](../bindless_textures.md) at the
> **renderer** level, outside this document's graph. Nothing here built a graph, a draw
> node or a mesh section. Read the phase list below as a list of renderer capabilities,
> which is what it turned out to describe.
>
> - **A — not started.** `examples/multi_mesh/src/main.rs:72-74` still keeps its `P_*`
>   consts.
> - **B — done.** Every listed item shipped: `cmd_push_constants`, the queue and `Gpu`
>   API, the `[vk::push_constant]` codegen, and "move `Addr` fields from `Params` into the
>   push block" — which is exactly `ToonLinkDraw`. The blocker recorded at
>   `../bindless_textures/phase_08.md:483` — `Gpu` is constructed after `queue_draw_*`, so
>   an address minted in the submit closure does not exist at queue time — was removed by
>   Phase 7c's `&self` minting on `FrameRenderer`.
> - **C — done.** Device features, the heap and Slang handles all landed.
> - **D — partly done.** `toon_link`'s texture-driven pipeline collapse landed (§7), by
>   hand in the example rather than through a graph. Pipelines are still
>   shader + vertex layout + raster state **+ uniform buffer**; the frame-global-BDA option
>   was not taken, so pipelines still bind a param UBO alongside the heap.
> - **E — not started.**

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
  **Resolved.** The spike ran and the assumption held; answers in
  [../bindless_textures/phase_0_spike.md](../bindless_textures/phase_0_spike.md). It also
  settled the idiom: a `Sampler2D.Handle` lights up heap binding 1 alone, while the
  separate texture-plus-sampler form lights up 0 and 2 — which is why §5's model is
  combined image samplers.
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
  **Settled by construction: slots are never released.** `insert_texture` is a bump
  allocator over `next_slot`, and its full-heap error says so
  (`descriptor_heap.rs:111-112`). Nothing recycles a slot, so nothing can recycle one out
  from under an in-flight command buffer. See §13.7. The residual risk is the opposite of
  the one written here: a hard 4096-slot ceiling with no free path, which a
  texture-streaming workload would exhaust.
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
- **`03_bindless.md`** — background on Vulkan descriptor indexing. Its buffer half holds:
  the renderer chose BDA, and buffers are not bindless. Its texture half is superseded —
  this design's bindless textures shipped, so that doc's status header points here and at
  `../bindless_textures.md` rather than describing per-pipeline texture descriptors. Its
  Vulkan feature and update-after-bind material stays accurate; its GLSL and API sketches
  do not match the shipped `DescriptorHeap`.
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
  rotates every frame (`Gpu::addr`, `renderer.rs:5547`; `create_storage_buffers_per_frame`,
  `:1076`). A compute shader that writes through it cannot see those writes next frame:
  frame N+1 binds a physically different `VkBuffer` holding whatever was there
  `MAX_FRAMES_IN_FLIGHT` executes ago (`PRE_WAIT_RING_LEN` before 2026-07-28 — the
  ring shrank from 3 to 2, so the staleness is nearer but no less wrong). Only `GpuOnlyBufferHandle` + `previous_addr` (`:5569`) escapes this, and only
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
> [../remove_pipelined_compute.md](../archived/remove_pipelined_compute.md) performed:
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
> [../remove_pipelined_compute.md](../archived/remove_pipelined_compute.md) Phase 3 had to solve, and
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

**Closed 2026-08-14 — the second option, and both premises of the hazard are gone.**

- **No `drop_texture`.** There is no texture-drop API on `Renderer` at all. Textures are
  destroyed at teardown, which is what §11 claimed and this entry disputed.
- **egui never enters the heap.** `insert_texture` has one caller, `register_texture`
  (`renderer.rs:577`), on the `create_texture*` path. egui runs `egui_ash_renderer` with
  its own descriptors, so its per-frame texture churn never touches a heap slot. The live
  egui interaction is the reverse one: it clobbers the heap *binding* while recording, and
  is harmless only because it records last (`renderer.rs:2330-2334`).
- **Slots are never released.** `insert_texture` bumps `next_slot` and never reclaims
  (`descriptor_heap.rs:109-116`).

**What replaces it:** the heap holds 4096 slots and has no free path, so the failure mode
is exhaustion rather than aliasing. `insert_texture` returns an error at the ceiling and
`register_texture` destroys the texture rather than leaking it. Any future
texture-streaming workload re-opens this entry, and the deferred-recycle design above is
the answer it should reach for.

### 13.8 Smaller items

- **§4's "static data uploads once"** cites `sprite_batch.rs:144`
  (`examples/sprite_batch/src/main.rs:150`), which in fact writes **every frame**. Uploading
  once requires `Renderer::write_immutable_all_frames` (`renderer.rs:1144`) *and* still
  minting `current_immutable_addr` per ring slot each frame. A naive setup-time
  `write_immutable` seeds 1 of 2 slots — **one** bad frame at startup, not the two this line
  said, since the ring is `MAX_FRAMES_IN_FLIGHT = 2` and not 3 — then correct forever, which
  is exactly the failure mode nobody catches. Subsumed if 13.1's non-ringed buffer lands.
  **It landed** (`create_singleton_buffer`, Phase 7d), so the whole item is moot for data
  that never changes after setup; §4 is corrected to point at it.
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
