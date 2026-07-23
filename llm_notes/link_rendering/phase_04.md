# Phase 4: multi-draw queue + shared meshes

Detailed plan for P4 of [`../link_rendering.md`](../link_rendering.md) §6
(renderer §4.1 + §4.2). Estimated: 2 days. Verification strategy follows
[`tests.md`](tests.md) §P4 with two re-weightings: the snapshot criterion is
restated (a new shader necessarily touches the atlas-index snapshot — see
Step 4), and the all-examples sweep stays a documented shell loop, not a
justfile recipe (decided in planning). Renderer-only: independent of the
converter track (P0–P3, `a76d0cb`/`6431f0a`/`8a0a4af`/`7704292`) and needs no
Link assets. Line numbers below verified at `aaf479e`.

**Goal**: after P4 the renderer can record N draws per frame inside its single
render pass — each draw binding its own pipeline, descriptor sets, and either
its own vertex/index buffers or a **shared mesh** drawn by **index sub-range**
— behind a queue API (`queue_draw_*` + terminal `submit_draws`) that mirrors
the existing `pending_compute` pattern; all existing examples keep compiling
and running byte-identically through legacy one-element wrappers; and a
committed, asset-free `examples/multi_mesh.rs` proves the new surface with
index ranges whose off-by-ones are visually loud. This is exactly the shape
the P6 toon_link example consumes: 24 manifest batches
(`{material, first_index, index_count}`) over one 1754-vertex/8622-index mesh.

**Deliverables**

1. `src/renderer.rs` — `PendingDrawCommand` queue, `DrawCallConfig::IndexRange`,
   record loop, `queue_draw_indexed`/`queue_draw_index_range`/
   `queue_draw_vertex_count`/`submit_draws`, index-based hot reload,
   `create_mesh` + mesh teardown
2. `src/renderer/pipeline.rs` — `VertexPipelineConfig::SharedMesh`,
   `VertexConfig::SharedMesh`, `with_shared_mesh`, `MeshHandle<V>`,
   `PipelineStorage::get_by_index`/`get_mut_by_index`; delete the unused
   `VertexAndIndexBuffersHandle` stub (pipeline.rs:122, no references)
3. `shaders/source/multi_mesh.shader.slang` — minimal lit+tint shader (new
   generated files + snapshots are *added*; pre-existing per-shader snapshots
   stay byte-identical)
4. `examples/multi_mesh.rs` — 3 pipelines, 1 shared mesh, 5 index-range draws
   that tile the index buffer exactly (the off-by-one detector)
5. Master-plan edits: §6 P4 row links here; §4.5 gains the deferred
   "picking + multi-draw" note
6. No `Cargo.toml` change, no changes to existing examples, Recorded facts
   below filled in

## Renderer facts this phase relies on

All at `aaf479e`; re-verify line numbers before editing (renderer.rs is
actively developed).

- **FrameRenderer** (src/renderer.rs:5162): `renderer: &'f mut Renderer` +
  `pending_compute: Vec<PendingComputeCommand>`. `dispatch`/`memory_barrier`
  (5202/5209) push via `&mut self`; `draw_indexed` (5224),
  `draw_vertex_count` (5243), `draw_vertex_count_with_picking` (5253) all
  consume `self` — the one-draw-per-frame contract is type-enforced — and
  funnel into private `draw_frame` (5280) → `Renderer::draw_frame` (2018).
  The doc comment at 5160–5161 ("a frame's single draw call") goes stale in
  this phase; update it.
- **`DrawCallConfig` already exists** (private enum, 5299–5303:
  `VertexCount(u32)` | `IndexCount(u32)`). P4 *extends* it with `IndexRange`;
  it is not new.
- **`PendingComputeCommand`** (5147–5158) stores a type-erased
  `pipeline_index: usize` — the model for `PendingDrawCommand`.
  `record_compute_commands` (1293) looks pipelines up via
  `compute_pipelines.get_by_index`.
- **Hot reload** (debug-only): `draw_frame` 2028–2037 dedups the pending
  compute indices and calls `check_for_shader_recompile` (2427), which
  recreates exactly *one* graphics pipeline — the drawn one — via
  `try_shader_recompile` (2477) using `PipelineStorage::get/get_mut(handle)`.
  `PipelineStorage` (pipeline.rs:51) has no by-index accessors;
  `ComputePipelineStorage` (pipeline.rs:219–226) is the precedent to copy.
  The `old_pipelines` reap at 2432–2460 compares against
  `total_frames - MAX_FRAMES_IN_FLIGHT` — a plain `-` that underflows if a
  reload lands in frame < MAX_FRAMES_IN_FLIGHT.
- **`record_command_buffer`** (1399): picking pass 1421–1567 (separate render
  pass, fullscreen-triangle `cmd_draw`, one per *frame*); main pass
  1569–1786. The bind+draw block 1692–1763: bind pipeline → viewport/scissor
  (per-frame, from `render_extent`) → match `vertex_pipeline_config`
  (VertexAndIndexBuffers: vertex buffer at binding 0 offset 0 +
  `cmd_bind_index_buffer(..., UINT32)`; VertexCount: nothing) → ring-slot
  descriptor sets via `descriptor_sets_for_frame` (1974) →
  `cmd_draw(n,1,0,0)` / `cmd_draw_indexed(n,1,0,0,0)` — **firstIndex and
  vertexOffset hardcoded 0**. The loop must live *inside* the single
  `cmd_begin_rendering`/`cmd_end_rendering` (1687/1766): color+depth use
  `LoadOp::CLEAR`, so a second pass would wipe prior draws; MSAA resolve and
  the blit happen once at pass end and are untouched. The main-pass debug
  label (1571–1584) is derived from *the* drawn pipeline's shader name —
  becomes a fixed `"Main"` pass label + per-draw labels. No push constants
  are recorded anywhere.
- **Buffer ownership**: `VertexAndIndexBuffers` (pipeline.rs:124–132: both
  buffers + allocations + `index_count`) is owned per-pipeline inside
  `VertexPipelineConfig` (117–120). `init_pipeline` (renderer.rs:1207–1239)
  creates the buffers inline via the free functions `create_vertex_buffer`
  (3463) and `create_index_buffer` (3504) — staging-upload helpers that take
  `(&allocator, &device, command_pool, queue, data)` and are directly
  reusable for a standalone mesh store. `destroy_pipeline` (1040–1066) frees
  them at 1051–1057. `FrameRenderer::draw_indexed` (5229–5236) resolves
  `index_count` by matching `vertex_pipeline_config` and panics on the
  non-buffer arm — needs a `SharedMesh` arm.
- **Teardown**: `Drop` (2633–2725) destroys pipelines at ~2694 and the VMA
  allocator (`ManuallyDrop`) at 2715, which **reports leaks** — the mesh
  store must drain before 2715; the leak report is the free regression test.
- **Snapshot mechanics**: the `generated_files` insta test
  (src/shaders/build_tasks.rs:1464) snapshots every emitted file *including
  the atlas index* (`generated_files@src__generated__shader_atlas.rs.snap`),
  which necessarily gains a `pub mod multi_mesh;` line + `ShaderAtlas` field.
  `just test` runs with `INSTA_UPDATE=no`, so the two new snapshots must be
  accepted via `just insta` first.

## Step 1 — internal multi-draw plumbing + `IndexRange` (no public API change)

- Beside `PendingComputeCommand` (~5147):

  ```rust
  enum PendingDrawCommand {
      Draw { pipeline_index: usize, draw_call: DrawCallConfig },
  }
  ```

  and extend `DrawCallConfig` with
  `IndexRange { first_index: u32, index_count: u32 }`.
- `Renderer::draw_frame` (2018) and `record_command_buffer` (1399) take
  `pending_draws: Vec<PendingDrawCommand>` instead of
  `pipeline_handle: &PipelineHandle<D>` + a single `draw_call`; the `<D>`
  generic on both disappears (type erasure via `pipeline.index()`, exactly
  like compute). `FrameRenderer::draw_frame` (5280) builds a one-element vec,
  so the public surface is unchanged in this step.
- Rewrite 1692–1763 as a loop over the commands, per iteration: per-draw
  debug label → `cmd_bind_pipeline` → buffer binds with skip-if-same
  tracking (`last_bound: Option<(vk::Buffer, vk::Buffer)>` — compare
  **`vk::Buffer` handles**, not config variants, so the skip works across a
  whole-buffer/shared-mesh mix) → `descriptor_sets_for_frame(index)` per
  pipeline (single `ring_slot` per frame is correct for all draws) → the
  draw: `VertexCount(n)` → `cmd_draw(n,1,0,0)`; `IndexCount(n)` →
  `cmd_draw_indexed(n,1,0,0,0)`; `IndexRange { first_index, index_count }` →
  `cmd_draw_indexed(index_count, 1, first_index, 0, 0)`. Viewport/scissor
  stay once-per-frame before the loop.
- Hot reload goes index-based, mirroring compute:
  `check_for_shader_recompile(&mut self, graphics_indices: &[usize],
  compute_indices: &[usize])`; `try_shader_recompile(index: usize)` via new
  `PipelineStorage::get_by_index`/`get_mut_by_index` (the `_mut` one
  `#[cfg(debug_assertions)]`); `draw_frame` dedups queued graphics indices
  exactly like the compute block at 2028–2037. Drive-by fix while touching
  the reap loop: `total_frames.saturating_sub(MAX_FRAMES_IN_FLIGHT)` (2437)
  — pre-existing frame-1 underflow, now reachable more easily with multiple
  reloads in flight.
- Gate: `cargo check --all` clean; run 2–3 existing examples
  (`timeout 3 just dev basic_triangle` etc.) — behavior identical, no
  validation output.

## Step 2 — public queue API

```rust
impl<'f> FrameRenderer<'f> {
    pub fn queue_draw_indexed(&mut self, pipeline: &PipelineHandle<DrawIndexed>);
    pub fn queue_draw_index_range(&mut self, pipeline: &PipelineHandle<DrawIndexed>,
                                  first_index: u32, index_count: u32);
    pub fn queue_draw_vertex_count(&mut self, pipeline: &PipelineHandle<DrawVertexCount>,
                                   vertex_count: u32);
    pub fn submit_draws(self, gpu_update: impl FnOnce(&mut Gpu)) -> Result<(), DrawError>;
}
```

- `queue_draw_indexed` resolves the whole-buffer (or, after Step 3,
  whole-mesh) `index_count` at queue time and pushes `IndexCount`.
- `queue_draw_index_range` bounds-checks
  `first_index.checked_add(index_count) <= total` with a **`debug_assert`**
  whose message names the pipeline's shader and the offending range
  (decided in planning: debug-only; a release-build out-of-range draw
  renders garbage silently under robustBufferAccess — accepted). The check
  lives in a small pure helper so it can be unit-tested.
- Legacy `draw_indexed`/`draw_vertex_count` become one-element wrappers with
  documented **append semantics**: queue + submit — a caller who queued
  draws first submits the union. `draw_vertex_count_with_picking`
  additionally `debug_assert!(self.pending_draws.is_empty())`: picking stays
  single-draw in P4 (deferred note in master plan §4.5) and this is the
  enforcement point. No example changes.
- Update the stale `FrameRenderer` doc comment (5160–5161) to describe the
  queue + terminal-submit model, noting `submit_draws(self, …)` is the
  single-terminal-submit shape the FrameInputs plan
  ([`../frame_inputs_api.md`](../frame_inputs_api.md)) also depends on — the
  later migration only swaps the `gpu_update` closure for `frame_inputs`
  delivery and deletes the wrappers.
- Gate: build + full example sweep (everything routes through the wrappers).

## Step 3 — shared meshes

```rust
pub struct MeshHandle<V: VertexDescription> { index: usize, _marker: PhantomData<V> }

impl Renderer {
    pub fn create_mesh<V: VertexDescription + GPUWrite>(
        &mut self, vertices: &[V], indices: &[u32]) -> anyhow::Result<MeshHandle<V>>;
}
```

- **Typed handle** (deviation from the master plan's untyped sketch, decided
  in planning): `with_shared_mesh` sits on `PipelineConfig<'t, V, D>`, which
  already carries `V`, so `with_shared_mesh(&MeshHandle<V>)` makes a
  vertex-layout mismatch a compile error at zero ergonomic cost (`V` is
  inferred at `create_mesh`). Untyped, a mismatch silently fetches garbage
  vertices — validation layers don't reliably catch it.
- `Renderer.meshes: Vec<VertexAndIndexBuffers>`; `create_mesh` reuses the
  free functions `create_vertex_buffer`/`create_index_buffer` (3463/3504)
  verbatim and `bail!`s on empty vertices or indices (zero-size
  `vkCreateBuffer` is invalid).
- `VertexPipelineConfig::SharedMesh(usize)` + `VertexConfig::SharedMesh(usize)`;
  `PipelineConfig::with_shared_mesh` overwrites `vertex_config`, discarding
  the empty vecs the generated `pipeline_config(resources)` was given — the
  generated code is untouched; the example documents the
  empty-`Resources` + `.with_shared_mesh(&mesh)` pattern.
- New `SharedMesh` arms: `init_pipeline` (pass-through, creates no buffers;
  its `VertexAndIndexBuffers` arm gains a friendly bail on empty vertex data
  — "empty vertex data — did you mean `.with_shared_mesh()`?"), the record
  loop (binds `self.meshes[i]`), `draw_indexed`/`queue_draw_indexed` count
  resolution, and `destroy_pipeline` (**frees nothing** — shared buffers
  outlive pipelines).
- Lifecycle is teardown-only (decided in planning): no `destroy_mesh`;
  `Drop` drains `meshes` next to the pipeline teardown (~2694), safely
  before the allocator `ManuallyDrop` at 2715. The VMA leak report enforces
  the ordering.
- Gate: build; existing examples unaffected (nothing uses the new arm yet).

## Step 4 — shader + codegen (parallelizable with Steps 1–3)

`shaders/source/multi_mesh.shader.slang`, deliberately minimal but the base
P5 extends (its raster-state test objects reuse this shader with more
pipelines):

- `Vertex { float3 position; float3 normal; }`
- `ParameterBlock` with `MultiMeshParams { MVPMatrices mvp; float4 tint; }`
  (nested-struct uniform codegen is snapshot-proven; **no** `uint4[8]`-style
  arrays here — the uniform-array smoke test stays where the master plan put
  it, P6)
- vertex: `mvp` transform, pass world normal
- fragment: fixed-direction lambert
  `float3 c = params.tint.rgb * (0.35 + 0.65 * max(dot(n, L), 0)); return float4(c, 1.0);`
  — alpha 1.0 because blending is still hardcoded SRC_ALPHA until P5.

Then `just shaders` → `just insta` (accept exactly: 2 new snapshots —
`…multi_mesh.rs.snap`, `…multi_mesh.json.snap` — plus the multi_mesh
additions to the atlas-index snapshot) → `INSTA_UPDATE=no cargo test` green
proves every pre-existing per-shader snapshot is byte-identical. That is the
restated tests.md criterion; "all snapshots byte-identical" is impossible
verbatim once a shader is added.

## Step 5 — `examples/multi_mesh.rs`

One shared mesh, three shapes concatenated into a single vertex/index buffer
(positions baked in world space, `model = I`, slow orbit camera as in
viking_room):

| index range | content | pipeline |
|---|---|---|
| `[0, 36)` | cube at (−2.2, 0, 0), per-face normals | A (red) |
| `[36, 54)` | square pyramid at (+2.2, 0, 0) | B (green) |
| `[54, 72)` | disc sector 1 (wedges 0–5) | A (red) |
| `[72, 90)` | disc sector 2 (wedges 6–11) | B (green) |
| `[90, 108)` | disc sector 3 (wedges 12–17) | C (blue) |

- The disc: center vertex + 19 rim vertices at the origin, radius ~1.2,
  tilted ~20° off the orbit plane so it never goes fully edge-on; 18 wedges
  laid out wedge-major, 3 indices each, split into three 120° sectors.
- **The five ranges tile `[0, 108)` exactly** — asserted at setup (disjoint,
  contiguous, sum == index buffer length). This is what makes a
  `first_index` off-by-one loud: a correct frame is a complete tricolor disc
  with clean sector boundaries against the clear color; any in-bounds shift
  leaves a contrast-colored **gap wedge** (overlapping re-draws would be
  masked by depth-LESS, but exact tiling means every shift opens a gap
  somewhere); an out-of-bounds shift trips the Step 2 `debug_assert`; a
  single-index (not whole-triangle) shift scrambles wedges into spikes.
- 3 pipelines (one per tint), 3 uniform buffers, **5 queued draws** —
  exercising: the same pipeline appearing twice in one queue (A, B),
  consecutive draws sharing buffers (draws 2–5 skip re-binding), per-pipeline
  ring-slot descriptor sets in the loop, and one `submit_draws` closure
  writing all three uniforms.
- Comments document the shared-mesh pattern (empty `Resources` vecs +
  `.with_shared_mesh`), per master plan §4.2.

## Step 6 — verification + docs

Run the full test plan below; do the perturbation and hot-reload checks;
append the picking note to `../link_rendering.md` §4.5 and link this doc from
the §6 P4 row; fill Recorded facts.

## Test plan

**Automated (`just test` / CI):**

- Insta: the `generated_files` test picks up the new shader automatically;
  gate is *pre-existing per-shader snapshots byte-identical*, atlas-index
  snapshot changed by exactly the multi_mesh additions, two new snapshots.
- Unit test the pure bounds-check helper: in-range, exact-fit,
  off-by-one-over, `u32` overflow via `checked_add`.
- `cargo check --all` + `cargo build --examples` (multi_mesh compiles);
  `just lint` clean.

**Validation sweep** — the real test for the recording loop; documented loop,
not a recipe (decided in planning):

```sh
for e in basic_triangle depth_texture dragon gpu_picking koch_curve multi_mesh \
         particles ray_marching sdf_2d serenity_crt space_invaders sprite_batch \
         suzanne viking_room watercolor; do
  timeout 3 just dev "$e" 2>&1 | grep -iE "validation|VUID" && { echo "FAIL: $e"; exit 1; }
done; echo "sweep clean"
```

(Adjust the list to `ls examples/` at run time.) This vets descriptor/binding
correctness in the loop, the shared-mesh bind path, the legacy wrappers
(every old example), picking (`gpu_picking`), and pipelined compute
(`particles`/`watercolor`) coexisting with the refactored `draw_frame`.

**Eyeball (results → Recorded facts):**

1. multi_mesh correct render: complete tricolor disc, clean 120° boundaries,
   shaded cube/pyramid, orbiting.
2. **Perturbation test — test the test, then revert:** sector C
   `first_index` 90→87 → visible gap wedge; 90→91 → scrambled spikes;
   90→93 → `debug_assert` (93+18 > 108). Confirms the example detects the
   bug class it exists for.
3. **Hot reload:** edit the shader body while multi_mesh runs → all three
   graphics pipelines recompile via the deduped-index path; keep running a
   few seconds (old_pipelines reap fires), exit clean.
4. **Empty-queue submit:** one-off local check that `submit_draws` with
   nothing queued renders clear color + egui with no validation output;
   record the observed behavior.
5. Clean multi_mesh exit with **no VMA leak report** (mesh teardown
   ordering).

## Verification (exit checklist)

- [X] `just test` green: pre-existing per-shader snapshots byte-identical;
      atlas-index snapshot diff is exactly the multi_mesh additions; 2 new
      snapshots accepted
- [X] Bounds-check helper unit tests green
- [X] `just lint` clean; `cargo build --examples` clean
- [X] Validation sweep clean over all examples (loop above)
- [X] multi_mesh renders correctly; perturbation test performed and reverted,
      results recorded
- [X] Hot-reload multi-pipeline pass performed, clean
- [X] Empty-queue `submit_draws` behavior recorded
- [X] No VMA leak report on multi_mesh exit
- [X] No changes to existing examples or `Cargo.toml`; `git diff` on
      `src/generated/` is additive only
- [X] Master plan updated: §6 P4 row links here; §4.5 picking note added
- [X] Recorded facts filled in

## Recorded facts (fill in after gates pass)

```
commit:                   4621112 ("add queued multi-draw support")
final API line numbers:   (as committed at 4621112, after the two example restructures
                          noted below — the earlier draft's numbers were pre-restructure)
                          PendingDrawCommand @ renderer.rs:5279, submit_draws @ 5438,
                          create_mesh @ 981, queue_draw_indexed @ 5388,
                          queue_draw_index_range @ 5397, queue_draw_vertex_count @ 5426,
                          legacy wrappers @ 5442/5451, draw_vertex_count_with_picking @ 5461,
                          index_range_in_bounds @ 5513 (+ unit tests @ 5533),
                          record loop @ 1765, hot-reload dedup @ 2126,
                          MeshHandle @ pipeline.rs:192, VertexPipelineConfig::SharedMesh
                          @ pipeline.rs:183, with_shared_mesh @ pipeline.rs:236,
                          PipelineStorage::get_by_index @ pipeline.rs:146
snapshot churn:           new: multi_mesh.rs.snap, multi_mesh.json.snap
                          changed: atlas-index snap (+3 lines) AND
                          shader_branching_snapshots.snap (+2 lines — the per-.spv
                          branch-count snapshot; additive, not anticipated by the plan)
wrapper semantics:        append (queued + wrapper draw submit together) — confirmed;
                          draw_vertex_count_with_picking debug_asserts an empty queue
empty-queue behavior:     renders the clear color, runs stably, no validation output,
                          clean exit with no VMA leak report
perturbation results:     87 → black gap wedge in the disc (overlap with sector B
                          z-masked, missing [105,108) loud); 89 → jagged rim spikes +
                          z-fighting patch at the B/C boundary; 93 → debug_assert
                          "index range [93, 93 + 18) out of bounds for pipeline
                          multi_mesh.shader.slang (index count 108)".
                          NOTE: the planned 91-shift case is wrong — sector C is the
                          last range, so 91+18 > 108 hits the debug_assert, not
                          spikes; 89 (in-bounds, non-triangle-aligned) is the spike
                          case. Perturbation applied at queue time so the data
                          validation doesn't preempt the frame-level detection.
hot-reload:               3 pipelines recreated per edit event (2 events per sed
                          write), rendering continued seconds after (reap window
                          passed), no validation errors
sweep:                    15/15 examples clean (particles + watercolor cover
                          pipelined compute; gpu_picking covers the picking wrapper)
deviations discovered:    - shader_branching_snapshots churn (above)
                          - master-plan §6 link + §4.5 picking note already landed
                            with the planning commit 56eaed8 ("planning link
                            rendering phase 4"); no doc edit needed
                          - type-erased indices are newtypes, not the plan's bare
                            usize: GraphicsPipelineIndex for pipelines and MeshIndex
                            for meshes (PendingDrawCommand, get_by_index,
                            VertexPipelineConfig::SharedMesh all use them)
                          - MeshHandle<V> shipped typed, as Step 3 decided; the
                            master plan's §4.2 sketch still showed the untyped
                            form and was corrected during P4 close-out
                          - clean-exit/VMA verification needed a WM_DELETE_WINDOW
                            close (timeout's SIGTERM skips Drop)
                          - example restructured post-verification: DRAWS stores
                            (count, pipeline) with first_index derived as a running
                            sum (contiguity by construction); coverage is a const
                            assert against INDEX_COUNT, plus one runtime assert
                            tying INDEX_COUNT to the built mesh
                          - example restructured again for per-shape model matrices:
                            shapes baked at the origin, placement/animation in
                            shape_models(); 5 pipelines (one per shape×color, since
                            uniforms are per-pipeline) and 6 draws (cube split in
                            two ranges — same pipeline queued twice, the Link
                            multi-batch-per-material pattern); mvp.slang gained
                            MVPMatrices.rotateDirection (w=0 model-matrix rotate,
                            valid for rotation/uniform scale) so lighting tracks
                            the animated model matrices; no reflection change, no
                            new snapshot churn
```

## Out of scope for P4

- Raster state + texture options — **P5** (extends this same multi_mesh
  example with per-state objects)
- Picking + multi-draw — deferred; note lives in master plan §4.5; enforced
  here by the `debug_assert` in `draw_vertex_count_with_picking`
- FrameInputs migration ([`../frame_inputs_api.md`](../frame_inputs_api.md))
  — P4 keeps the `Gpu`-closure API; only the terminal-submit shape is shared
- `destroy_mesh` / mesh streaming — teardown-only lifecycle for now
- Instancing (instanceCount stays 1), u16 index buffers, vertexOffset ≠ 0
- A committed example mixing DrawIndexed + DrawVertexCount in one frame —
  structurally supported (type erasure), cheap to add when P5 grows the
  example

## Risks / open questions

1. **Second render pass would CLEAR** — the loop must stay inside the one
   `cmd_begin_rendering`/`cmd_end_rendering` (1687/1766); color and depth
   are cleared once, MSAA resolve and blit at pass end are untouched. Depth
   LESS across queued draws is then well-defined.
2. **Overlap invisibility under depth-LESS** — an overlapping re-draw
   z-fails and shows nothing; mitigated by the example's exact-tiling design
   (every shift produces a gap or an assert, never a silent overlap).
3. **Bind-skip correctness** — compare `vk::Buffer` handles, not
   `VertexPipelineConfig` variants, or a whole-buffer pipeline followed by a
   shared-mesh pipeline over the same buffers re-binds (harmless) or a
   different buffer is wrongly skipped (broken).
4. **Hot reload with multiple pipelines** — `old_pipelines` can now receive
   several entries in one frame; the reap loop already iterates all of them,
   but fix the frame-1 `usize` underflow (`saturating_sub`) while there.
5. **Release-build out-of-range draws render garbage silently** —
   consequence of the debug_assert-only bounds check (accepted decision);
   robustBufferAccess makes OOB index fetches non-faulting.
6. **Uniform codegen for `MultiMeshParams`** — nested struct + `float4` only,
   snapshot-proven territory; the risky `uint4[8]` array smoke test remains
   P6's first task (master plan risk #4), deliberately not pulled forward.
7. **FrameInputs drift** — keep `Renderer::draw_frame(pending_draws,
   picking_config, pending_compute, gpu_update)` as the single internal
   submit point so the FrameInputs migration only swaps the closure for the
   written-handle set and deletes the wrappers.
