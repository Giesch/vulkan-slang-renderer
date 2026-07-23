# Render Graph Design (Current)

> **STATUS: DESIGN — the consolidated, current render-graph design.** Supersedes
> the descriptor-era assumptions in `01_flame_render_graph.md` and adopts a
> modified form of `02_explicit_parallelism.md` §"Simulation-Focused Parallelism".
> Written 2026-07 against the post-BDA, post-pipelined-compute renderer.
>
> **Hard prerequisite:** `claude_notes/bda_footguns/03_pipelined_current_read_plan.md`
> (domain-typed addresses) must land before graph codegen work begins — see §9.

## Decisions (settled)

1. **Build cadence: build once at setup.** Runtime variability (parity, Jacobi
   trip count, conditional passes) is expressed through execute-time parameters
   on a fixed structure — not per-frame graph rebuilds.
2. **BDA dependency tracking: handle-based declarations.** Graph nodes declare
   storage-buffer *handles*; the access direction per param field is already
   known to codegen (`Addr` = write, `ReadAddr` = read, `ImmutableAddr` =
   GPU-never-writes). The graph mints addresses itself at execute time.
3. **Ping-pong: graph-managed variants.** A node declares a ping-pong resource
   pair; the graph creates every needed pipeline variant internally and selects
   by parity at execute time. The bind-at-pipeline-creation descriptor model is
   kept.
4. **Domains: the graph owns the pipelined/frame split.** `.simulation()` /
   `.rendering()` sections drive `ComputePlacement`; the graph, not the app,
   calls the pipelining machinery.

---

## 1. Current renderer state (what the graph builds on)

Verified against the code as of commit `d574950`.

### Buffers are BDA pointers, not descriptors

Storage buffers are not bound via descriptor sets. Each shader has a single
`ParameterBlock<Params>` uniform buffer (the only buffer descriptor), and
`Params` carries raw 8-byte GPU pointers:

| Rust (`src/renderer/addr.rs`) | Slang (`shaders/source/addr.slang`) | Meaning |
|---|---|---|
| `Addr<T>` (:10) | `Ptr<T, Access.ReadWrite, …>` | writable |
| `ReadAddr<T>` (:65) | `Ptr<T, Access.Read, …>` | read-only for this shader |
| `ImmutableAddr<T>` (:129) | `Ptr<T, Access.Immutable, …>` | GPU never writes |

Handles (`src/renderer/storage_buffer.rs`): `StorageBufferHandle<T>`,
`ImmutableBufferHandle<T>`, `GpuOnlyBufferHandle<T>` — each a 3-slot ring
(`PRE_WAIT_RING_LEN = MAX_FRAMES_IN_FLIGHT + 1 = 3`, `src/renderer.rs:74`).
Addresses are minted per-frame inside the draw closure via `Gpu` methods
(`addr` :5107, `current_addr` :5118, `previous_addr` :5129,
`current_immutable_addr` :5137).

**Consequence for the graph:** buffer usage is invisible to binding-based
dependency tracking. That is why decision 2 exists.

### Textures and uniforms still bind at pipeline creation

Generated `pipeline_config()` packs `texture_handles`,
`uniform_buffer_handles`, `storage_texture_handles` into the pipeline config
(`src/renderer/pipeline.rs:139-141, 159-161, 235-237`). Resources are baked
into the pipeline at creation — this is the root cause of watercolor's
per-parity pipeline-variant arrays.

Storage textures are created in `GENERAL` layout with `STORAGE | SAMPLED`
usage (`create_storage_texture`, `src/renderer.rs:559`) and never transition.
`storage_texture_as_sampled` (:609) aliases the same `vk::Image` as a
`TextureHandle` (`ImageOwnership::Aliased`, :632).

### Frame command model

`Game::draw(FrameRenderer)` accumulates `PendingComputeCommand::{Dispatch,
Barrier}` (`src/renderer.rs:5156`; barrier = **global** `MemoryBarrier2` only)
and is consumed by exactly one terminal draw (`draw_indexed` /
`draw_vertex_count`), whose `gpu_update: FnOnce(&mut Gpu)` closure uploads
uniforms and mints addresses. Graphics renders only to the swapchain.

### Pipelined compute already exists

`ComputePlacement::{BeforeGraphics, SeparateCommandBuffer}` (`:5149`), enabled
by `Renderer::enable_pipelined_compute()` (`:853`). In pipelined mode, compute
goes to a separate queue with a timeline semaphore; **graphics frame N always
waits on compute N−1** (VERTEX | FRAGMENT | COMPUTE stages). There is no
same-frame ("synced") wait mode today. This is precisely "Pattern A" from
`02_explicit_parallelism.md` — the graph does not need to invent cross-frame
overlap, it needs to *own and police* it.

**Planned refactor (pre-graph): domains become per-dispatch, not per-renderer.**
The global mode toggle is being replaced by two per-frame command streams
that always coexist: a **frame stream** (`dispatch()` — recorded at the top of
the graphics command buffer with a renderer-guaranteed compute→graphics
barrier; always same-frame-correct) and a **pipelined stream**
(`dispatch_pipelined()` — the compute-queue submission). See
`claude_notes/watercolor_race_fixes.md` for the implementation plan. This
removes the mode-dependent-semantics footgun (the same `dispatch()` call
meaning different queues/guarantees depending on a flag), the frame-0
combined-path special case, and it gives the graph's sections a 1:1 runtime
target.

---

## 2. Core concepts

- **`ParityGroup`** — a graph-owned counter. Users never read or write parity.
  Frame-rate groups advance once per `execute()`; loop-attached groups advance
  once per loop iteration. This deletes watercolor's `sim_parity` /
  `deposit_parity` / `pressure_parity` fields and every piece of parity
  arithmetic in its draw function.
- **`GraphPingPong`** — a graph-owned rotation of N storage textures (N = 2 by
  default; more when read across the queue boundary, §8 — the safe count
  depends on how many nodes advance the rotation per frame: 3 slots for one
  advance, 4 for two). Created by
  the graph itself
  (it calls `create_storage_texture` + `storage_texture_as_sampled`
  internally), replacing watercolor's hand-rolled `PingPong` struct. Each
  ping-pong belongs to exactly one `ParityGroup`; all members of a group share
  a slot count.
- **Node** — one compute dispatch, or the single terminal draw. Structure is
  fixed at build; a node may carry a runtime enable flag (§7).
- **Sections** — `.simulation(…)` (pipelined domain) and `.rendering(…)`
  (frame domain). These map 1:1 onto the renderer's two per-frame command
  streams (see §1 planned refactor and `watercolor_race_fixes.md`): simulation
  nodes are emitted via the pipelined stream, rendering-section work via the
  frame stream + terminal draw. **v1 constraint:** compute nodes only in
  simulation; exactly one terminal draw in rendering. Post-v1, the frame
  stream makes frame-domain compute in the rendering section (e.g.
  post-processing that must see this frame's draw inputs) a natural
  relaxation — the stream and its automatic barrier already exist.

---

## 3. Builder API (setup time)

```rust
let mut gb = RenderGraph::builder(renderer);       // graph decides pipelining
let sim = gb.parity_group("sim");                  // advances once per execute
let dep = gb.parity_group("deposit");
let prs = gb.parity_group("pressure");             // attached to the Jacobi loop

let wet_mask = gb.ping_pong(&sim, W, H, vk::Format::R32_SFLOAT)?;
let pressure = gb.ping_pong(&prs, W, H, vk::Format::R32_SFLOAT)?;
let deposit_0_3 = gb.ping_pong(&dep, W, H, vk::Format::R32G32B32A32_SFLOAT)?;
let divergence = gb.storage_texture(W, H, vk::Format::R32_SFLOAT)?; // fixed

gb.simulation(|s| {
    let brush = s.node("brush", shaders.paint_brush_compute)
        .optional()                                // execute-time enable flag
        .dispatch_2d(W, H)                         // groups from WORKGROUP_SIZE
        .resources(|v| paint_brush_compute::Resources {
            wet_mask_in: v.front_sampled(&wet_mask),   // write-forward: copy + stamp
            wet_mask_out: v.back_storage(&wet_mask),   // advances the rotation
            pressure: v.front_storage(&pressure),      // in-place stamp (compute-internal)
            // …
            brush_params_buffer: &brush_params_buffer,
        })
        .params_buffers(paint_brush_compute::ParamsBuffers {
            stroke_points: graph::Write::storage(&stroke_points_buffer), // §5
        });

    s.node("update_velocity", shaders.wc_update_velocity_compute)
        .dispatch_2d(W, H)
        .resources(|v| wc_update_velocity_compute::Resources {
            u_in: v.front_sampled(&velocity_u),
            u_out: v.back_storage(&velocity_u),    // write target = back slot
            pressure: v.front_sampled(&pressure),
            paper_height: &paper_height_sampled,   // plain fixed handle
            // …
        });

    let jacobi = s.repeat("jacobi", &prs, |l| {    // advances prs per iteration
        l.node("pressure_jacobi", shaders.wc_pressure_jacobi_compute)
            .dispatch_2d(W, H)
            .resources(|v| wc_pressure_jacobi_compute::Resources {
                pressure_in: v.front_sampled(&pressure),
                pressure_out: v.back_storage(&pressure),
                divergence: &divergence_sampled,
                params_buffer: &pressure_jacobi_params_buffer,
            });
    });
    // … remaining nodes …
});

gb.rendering(|r| {
    r.draw_vertex_count("display", shaders.paint_display, 3)
        .resources(|v| paint_display::Resources {
            deposit_0_3: v.cross_frame_sampled(&deposit_0_3), // §8
            wet_mask: v.cross_frame_sampled(&wet_mask),
            // …
        });
});

let graph = gb.build(renderer)?;  // creates ALL variants, precomputes barriers
```

Key property: **the resources closure returns the shader's existing generated
`Resources<'_>` struct — no codegen change is needed for textures.** The
`VariantSelector` (`v`) resolves ping-pong references to concrete
`&TextureHandle` / `&StorageTextureHandle` for the variant being instantiated,
and records which groups/slots the node touched. Selector verbs mirror
watercolor's three access patterns exactly:

| Selector | Watercolor equivalent | Meaning |
|---|---|---|
| `front_sampled(&pp)` | `read_sampled(parity)` | sample the rotation's newest slot *at this node's position in the frame* |
| `front_storage(&pp)` | `read_storage(parity)` | in-place RMW on that slot (no advance) |
| `back_storage(&pp)` | `write_storage(parity)` | write the next slot and advance the rotation's in-frame cursor |
| `cross_frame_sampled(&pp)` | (the display reads) | rendering-section read of simulation output across the queue boundary; which slot and what wait is governed by the resource's `CrossFrameMode` — §8 |

"Front" is positional, not per-frame: a rotation advanced by two nodes in one
frame presents each node with the newest slot as of its place in the sequence,
which is what makes per-node variant selection work (§4).

---

## 4. Variant enumeration (no combinatorial explosion)

At `build()`, each node's closure is first probe-called to discover which
parity groups it touches. Variants are then instantiated once per reachable
slot-state **of only those groups**:

```
variants(node) = ∏ reachable_states(g) for g in groups_touched(node)
```

where `reachable_states(g)` is the number of distinct frame-start positions
the rotation actually visits — not the raw slot count. A 2-slot ping-pong
advanced once per frame has 2; a 4-slot rotation advanced twice per frame
also has 2 (its base alternates 0 ↔ 2). Each node's variant key
uses the group's offset *at that node's position in the sequence* (a group
advanced mid-frame by an earlier node presents later nodes with the advanced
cursor — see the selector-table note in §3).

Watercolor check: brush → {sim} = 2; jacobi → {pressure} = 2; advect →
{sim, deposit} = 4; display → {sim, deposit} = 4. This reproduces the
hand-rolled 2/2/4/4 counts — not 2³ = 8.

Each variant is built through the **existing** `pipeline_config(…)` →
`create_compute_pipeline(…)` path and stored as
`variants: Vec<PipelineHandle<Compute>>` keyed by the node's variant index; at
execute the graph computes the key from the current group counters. The graph
additionally *inspects* each variant's `*PipelineConfig` handle lists for
hazard tracking (§6).

---

## 5. BDA buffers: handle-based declaration

Codegen addition (templates in `src/shaders/build_tasks.rs`): for each shader
whose `Params` contains pointer fields, emit two small parallel structs:

```rust
// Declaration-time: handles + access, for dependency tracking + domain checks.
pub struct ParamsBuffers<'a> {
    pub stroke_points: graph::Write<'a, StrokePoint>,  // field was Addr → Write
    // ReadAddr → graph::Read<'a, T>; ImmutableAddr → graph::Immutable<'a, T>
}

// Execute-time: minted addresses handed to the uniforms closure.
pub struct ParamsPtrs {
    pub stroke_points: Addr<StrokePoint, PipelinedDomain>, // domain per shader kind
}
```

`graph::Write<'a, T>` / `graph::Read<'a, T>` are enums over
`Storage(&StorageBufferHandle<T>)`, `Current(&GpuOnlyBufferHandle<T>)`,
`Previous(&GpuOnlyBufferHandle<T>)` (plus `Immutable(&ImmutableBufferHandle<T>)`
on the read side). Build-time rules the graph enforces, on top of the
compile-time domain markers:

- `Current` of a gpu-only buffer is legal only on simulation-section nodes
  (mirrors `Gpu::current_addr` returning `Addr<T, PipelinedDomain>`).
- Rendering-section reads of pipelined buffers must be `Previous` (mirrors the
  domain-generic `previous_addr`).
- Write/write and write/read orderings between nodes touching the same handle
  produce barriers (§6).

At execute, the graph mints via the existing `Gpu` methods and fills a
`ParamsPtrs` for the node's uniforms closure (§7). Shaders without pointer
fields skip all of this.

---

## 6. Hazard tracking and barrier emission

- **Descriptor-bound images: fully automatic, zero codegen changes.** After
  building each variant, the graph reads the handle vectors that the generated
  `pipeline_config` already populates: `texture_handles` = sampled reads,
  `storage_texture_handles` = conservative read+write
  (`src/renderer/pipeline.rs:139-141`). Handles resolve to `vk::Image`
  identity inside the renderer, and `storage_texture_as_sampled` aliases record
  the same image (`src/renderer.rs:614-637`) — so **storage↔sampled aliasing is
  tracked for free** by keying the last-writer table on `vk::Image`.
- **BDA buffers:** tracked from the declared `ParamsBuffers` handles (§5);
  `Immutable` participates in no hazards.
- **Barrier emission (v1):** at most one `cmd_pipeline_barrier2` per node
  boundary. All buffer hazards, and all image hazards that need no layout
  transition — which today is *all* of them, since storage textures live
  permanently in `GENERAL` — coalesce into a single global `MemoryBarrier2`
  (COMPUTE→COMPUTE, SHADER_WRITE → SHADER_READ | SHADER_WRITE). This reuses
  `PendingComputeCommand::Barrier` unchanged: **v1 needs no renderer changes
  for barriers.** Per-image `ImageMemoryBarrier2` (and a
  `PendingComputeCommand::ImageBarrier` variant) is deferred until offscreen
  render targets introduce actual layout transitions.
- **Precomputation:** the frame-start parity state of all groups is a small
  deterministic space (∏ slot counts; ≤ 8 for watercolor — runtime-odd Jacobi
  counts only toggle one group). At build, the graph simulates the node
  sequence once per reachable frame-start state and stores each barrier
  schedule; `execute()` looks the schedule up and replays it. Acceptable
  fallback if memoization proves fiddly: recompute from the last-writer table
  every execute — O(nodes × resources) is trivial at ~12 nodes.
- **Dividend:** dependency-derived barriers legalize what watercolor does by
  hand today — one global barrier after every dispatch, except the one it
  deliberately omits (project-velocity → blur, disjoint resources). The graph
  omits *every* unnecessary barrier, and can never forget a necessary one.

---

## 7. Execute-time parameters, loops, conditionals

```rust
fn draw(&mut self, frame: FrameRenderer) -> Result<(), DrawError> {
    self.graph.execute(frame, |run| {
        run.enable(&self.brush, point_count > 0);        // default: enabled
        run.iterations(&self.jacobi, JACOBI_ITERATIONS); // runtime trip count
        run.write_storage(&mut self.stroke_points_buffer, &points);
        run.uniforms(&self.brush, |ptrs: paint_brush_compute::ParamsPtrs| {
            paint_brush_compute::Params {
                stroke_points: ptrs.stroke_points,       // pre-minted, domain-typed
                point_count,
                brush_radius,
                // …
            }
        });
        run.uniforms_plain(&self.update_velocity, wc_update_velocity_compute::Params {
            // shaders without pointer fields skip ParamsPtrs
        });
        // …
    })
}
```

`run` closes over the same pre-wait CPU-write window as today's `gpu_update`
closure; internally the graph drives `frame.dispatch` / `frame.memory_barrier`
and the terminal `draw_*` with a `gpu_update` that performs the buffered
uniform writes plus address minting.

- **Parity is advanced by the graph itself** — frame groups at the end of each
  execute, loop groups per iteration. Users never see a parity value.
- **Loops:** `repeat(name, &group, body)` is structured at build; at execute
  the body replays `run.iterations(…)` times, advancing the attached group
  each iteration and re-selecting body variants by parity. The intra-loop
  barrier (write back slot → next iteration reads it as front) is identical
  every iteration, so the precomputed per-variant barrier applies unchanged.
  Because downstream readers also variant-select from the group counter, **odd
  trip counts are legal** — watercolor's `JACOBI_ITERATIONS must be even`
  assert and its "pressure always at index 0" special cases disappear.
- **Conditionals:** `.optional()` + `run.enable(node, bool)`. A disabled node
  skips its dispatch but **keeps its barriers** (conservative: a global
  compute→compute barrier with no intervening write is nearly free, and every
  precomputed downstream schedule stays valid regardless of enable state).
  Correctness of the fixed structure must never depend on enable flags.

---

## 8. Cross-frame reads (and the watercolor race)

### The finding (verified 2026-07)

Watercolor's display pass does **not** read the previous frame's simulation
output, despite the comment at `examples/watercolor.rs:1025-1027`. The trace:

1. Frame N starts with `sim_parity = s`. Capillary flow dispatches
   `capillary_flow_pipelines[s]`, built with `wet_mask.write_storage(s)` —
   which negates: it writes `storage[!s]` (`watercolor.rs:87-89, 746-757`).
2. Parities flip (`:1022-1023`), so `sim_parity = !s`.
3. `display_idx` (`:1051`) selects the variant built with
   `wet_mask.read_sampled(!s)` (`:779`) = `sampled[!s]` — **the slot written
   this same frame**. The deposit chain traces identically.

Watercolor enables pipelined compute (`:414`), and graphics N waits only on
compute N−1 (`src/renderer.rs:2158-2173`). Frame N's compute and frame N's
graphics therefore run concurrently on separate queues while touching the same
image: a same-frame cross-queue data race. It is visually benign (the sim is
convergent; a torn read shows slightly newer paint) but it is not the "reads
previous frame's results" the comment claims.

> **Status: unfixed.** The example still exhibits this race: it runs under
> `enable_pipelined_compute()` (graphics N waits compute N−1), and the trace
> line numbers above are current. The fix belongs to the render graph's
> per-resource `CrossFrameMode` (below); a renderer-level stopgap (`SyncWait`)
> would need a same-frame wait mode the renderer does not have yet — see
> `watercolor_race_fixes.md`.

Two deeper structural facts, which any fix must respect:

- **(a) Two slots cannot give a clean previous-frame read.** If graphics N
  reads the slot compute N−1 wrote, compute N+1 — which waits only on compute
  N, not on graphics N — writes that same slot: a WAR race. This is exactly
  why the BDA uniform/storage rings are 3 slots (`PRE_WAIT_RING_LEN = 3`,
  `src/renderer.rs:74`).
- **(b) In-place RW breaks snapshots at any slot count.** Brush and
  flow-outward write ping-pong *front* slots in place (`front_storage`), so
  the previous frame's final state is mutated by the current frame before
  graphics could read it. Only write-forward restructuring or an explicit
  unsynchronized read can handle those resources.

### The design

A rendering-section read whose last writer is a *previous execute's*
simulation node emits **no barrier at all** — ordering comes from the existing
timeline semaphores. The graph statically checks, per resource, that the slot
graphics reads is written neither by this execute's simulation section (RAW)
nor by the next one before graphics retires (WAR, fact (a)). Where the check
fails, the ping-pong must declare a `CrossFrameMode`:

| Mode | Behavior | Cost / when |
|---|---|---|
| `ExtraSlot` (default) | Grow the rotation so graphics N reads a slot no in-flight compute touches (3 slots at one advance/frame, 4 at two — §2) | +1–2 textures of memory; the safe default for write-forward resources |
| `SyncWait` | Graphics N additionally waits compute N; compute N+1 still overlaps graphics N | No memory cost; **not implemented today** — needs a same-frame wait mode on the renderer (`watercolor_race_fixes.md`) |
| `unsynchronized()` | Explicit, loudly-named opt-in reproducing today's watercolor behavior | Required for in-place-RW resources (fact (b)) until they are restructured |

`cross_frame_sampled(&pp)` is only legal on rendering-section nodes, and only
for ping-pongs with a declared mode (or ones that pass the static check).

---

## 9. Domain integration

- `.simulation()` nodes are emitted into the **pipelined stream** and
  `.rendering()` work into the **frame stream** + terminal draw — the two
  always-available per-frame command streams from the pre-graph refactor
  (`watercolor_race_fixes.md`). There is no global pipelining toggle for the
  graph to drive: routing nodes to streams *is* the pipelining decision. A
  graph that wants no overlap simply puts its compute in the frame stream
  (which carries the renderer-guaranteed compute→graphics barrier), replacing
  the earlier `Pipelining::Auto | Off` build-option idea. The remaining
  policy knob is how graphics consumes pipelined output — the sync mode
  (previous-frame vs same-frame wait), which per-resource `CrossFrameMode`
  (§8) eventually subsumes.
- **Per-pipeline domain typing (hardening step, with `bda_footguns/03`):**
  once streams are per-dispatch, the natural end state is choosing the domain
  at pipeline creation (e.g. `PipelineHandle<Compute, PipelinedDomain>`), so
  `dispatch_pipelined` only accepts pipelined handles and stream routing is
  compile-checked. Note for 03: its "compute shaders are conservatively
  `PipelinedDomain`" codegen stance stays *sound* under per-dispatch streams
  (a frame-stream dispatch of a pipelined-typed shader is over-restricted,
  never under-restricted), but per-pipeline domains are the cleaner final
  assignment — key the domain off the pipeline, not the shader kind.
- BDA minting maps 1:1 onto sections: simulation nodes may receive
  `Addr<T, PipelinedDomain>` from `Current` declarations; rendering nodes only
  ever get `Previous` (→ `ReadAddr<T, FrameDomain>`) or plain / `Immutable`
  addresses. The generated `Params` field types make violations compile
  errors; the graph's build check on `ParamsBuffers` reports them earlier,
  with graph-aware messages.
- **Prerequisite confirmed: the domain-marker plan
  (`claude_notes/bda_footguns/03_pipelined_current_read_plan.md`) lands
  first.** `ParamsPtrs` must be spelled with `Addr<T, Domain>`; building graph
  codegen against single-parameter `addr.rs` would all be rework. That plan is
  also the semantic foundation of the section split: the hazard axis is
  *execution domain* (frame submission vs pipelined compute submission), not
  shader stage. Its stance that compute shaders are conservatively
  `PipelinedDomain` even when pipelining is off is what lets `Pipelining::Off`
  work without regenerating bindings.

---

## 10. Watercolor migration (validating example)

### Deleted (~350 lines)

- The `PingPong` struct (`watercolor.rs:77-94`) and all manual texture-pair
  creation.
- Every `[PipelineHandle; 2 | 4]` variant array and the 17 extra
  `ShaderAtlas::init()` calls that exist only because `pipeline_config(self, …)`
  consumes its atlas entry.
- The three parity fields (`:185-187`), all parity arithmetic in `draw`, and
  the even-iterations assert (`:61`).
- `compute_barrier()` (`:229`) and its 10 call sites — barriers become derived.
- The "pressure always at index 0" special cases.

### Setup becomes

Parity groups `sim` / `deposit` / `pressure`; one `gb.ping_pong(…)` per pair;
one node declaration per pass with a single resources closure (the graph
enumerates the 2/2/4/4 variants internally); Jacobi as
`s.repeat("jacobi", &prs, …)`; brush as `.optional()`; display as the
rendering node — i.e. the sketch in §3.

### Draw becomes

The `graph.execute(frame, |run| …)` block of §7: enable brush when
`point_count > 0`, set iterations, write stroke points, and pass the existing
per-pass uniform structs unchanged.

### Migration decision — still open

The watercolor race (§8) is currently **unfixed** in the example, so the
migration must choose a `CrossFrameMode` for each cross-frame-read resource
rather than inherit a settled one:

- **wet_mask / deposit**: today 2-slot `PingPong`s (`watercolor.rs:77-94`)
  whose display reads race this frame's compute. Candidate modes: `SyncWait`
  (cheapest, but needs the renderer's same-frame wait mode from
  `watercolor_race_fixes.md`), `ExtraSlot` (grow the rotation), or
  `unsynchronized()` (reproduce today's racy behavior explicitly).
- **In-place-RW resources** (brush and flow-outward use `read_storage`,
  `watercolor.rs:91-93`): fact (b) means `ExtraSlot` alone cannot snapshot
  them — they need write-forward restructuring first. The brush is also
  currently conditional (`if point_count > 0`, `:945-950`), which interacts
  with `.optional()`: a node that advances a rotation must still run its copy
  when disabled, or the group must not advance. This is a requirement §2–§4
  should absorb — slot count and per-frame advance count become properties of
  the group, and a node's variant key depends on the group's *offset at that
  node's position in the sequence*, not just the frame-start counter.

None of this is resolved yet; it is the substantive design work the watercolor
migration (Phase 6) still owes.

---

## 11. Implementation phases

- **Phase 0 — domain markers** (`bda_footguns/03`). Prerequisite; already
  fully planned and independently valuable.
- **Phase 0.5 — per-dispatch compute streams** (`watercolor_race_fixes.md`).
  Replace the pipelined-mode toggle with coexisting frame + pipelined command
  streams selected by dispatch method. Prerequisite for §9's section→stream
  mapping; independently fixes the frame-0 combined-path barrier gap and
  removes the mode flags. Can land before or in parallel with Phase 0.
- **Phase 1 — N pipelines from one atlas entry.** `pipeline_config(self, …)`
  consumes the generated `Shader` (e.g.
  `src/generated/shader_atlas/wc_pressure_jacobi_compute.rs:56`) — the root
  cause of the ShaderAtlas duplication. Fix: derive `Clone` on generated
  `Shader` structs and the reflection-json chain (`src/shaders/json.rs` — all
  plain data). Hot reload must also dedupe by `source_file_name`
  (`check_for_shader_recompile`, `src/renderer.rs:2455`) since K variants now
  share one source, and rebuild all K variants from one recompile. **This is
  the flagged risky interaction:** the graph must retain enough per-variant
  config (or the resources closures) to re-run pipeline creation on reload.
- **Phase 2 — core graph.** Builder + sections, fixed-resource nodes, hazard
  analysis from the `*PipelineConfig` handle lists, global-barrier emission,
  `execute()` routing nodes onto the two `FrameRenderer` command streams from
  Phase 0.5. No ping-pong yet. Port a simple example (e.g. particles) as the
  smoke test.
- **Phase 3 — parity groups, graph-managed ping-pongs, variant enumeration**
  (probe-recording selector, per-parity-state barrier schedules).
- **Phase 4 — loops, optional nodes, the execute-time params API.**
- **Phase 5 — BDA handle declarations.** `ParamsBuffers` / `ParamsPtrs`
  codegen in `build_tasks.rs` (two small extra structs per pointer-bearing
  shader; snapshot churn expected), graph minting + domain build checks,
  `CrossFrameMode` variants.
- **Phase 6 — watercolor migration** as the validation gate; delete the
  hand-rolled machinery. (The wet_mask cross-frame decision is already
  resolved — see §10; what remains is the mechanical port.)
