# FrameInputs: declarative per-frame buffer inputs

Status: **Design approved 2026-07-20 — not yet implemented.** Decisions below were
settled in a design interview; this doc is the reference for the implementation
plan. Supersedes the imperative `Gpu` closure API (`write_uniform`,
`write_storage`, `addr`/`current_addr`/`previous_addr`, `sort_storage_by`).

Related notes:
- [vulkan_1_3_migration/bda_renderer_plumbing.md](vulkan_1_3_migration/bda_renderer_plumbing.md) — how BDA addresses got into the `Gpu` API originally.
- [vulkan_1_3_migration/slang_pointer_codegen.md](vulkan_1_3_migration/slang_pointer_codegen.md) — the pointer-field reflection/codegen this design extends.

## 1. Motivation

Three footgun classes survive the current API. Each was established with a
concrete failure mode during the 2026-07 safety reviews:

**Occasional-write flicker.** Per-frame CPU writes touch only ring slot
N mod 3 (`PRE_WAIT_RING_LEN = 3`). A buffer written *sometimes* — the natural
dirty-flag pattern — leaves the three slot copies holding different
generations. Timeline: camera moves at frame 100 → slot 1 updated; no further
writes; frames render *new, old, old, new, old, old…* — a permanent 20 Hz
flicker at 60 fps. The only coherent disciplines are write-every-frame or
write-only-at-setup; the API currently allows the incoherent middle.

**Address stashing.** `Addr<T>`/`ReadAddr<T>`/`ImmutableAddr<T>` are `Copy`,
`'static`, and encode a slot chosen at mint time. An addr stashed at frame N
and embedded at frame N+1 points at the wrong slot. For a gpu-only buffer
that's a writable pointer to the history slot in-flight graphics may be
reading; for a CPU-written buffer, the stale slot is one the CPU will rewrite
pre-wait while an in-flight frame reads it (frame N reads slot N−1's stash;
frame N+2's pre-wait write hits that slot while frame N — unproven until this
frame's wait — may still be executing).

**Pipelined current-read race.** Under pipelined compute (now declared at
setup via `Renderer::enable_pipelined_compute`), frame N's graphics submit
waits only on compute N−1, so compute N runs concurrently with graphics N.
A graphics shader reading a gpu-only buffer's *current* slot (this frame's
compute output) races. The types can't see where an address lands after
`.into()`, so today this is doc-comment-only.

The fixes converge on one design: **the renderer becomes the sole authority on
addresses, timing, and completeness.** User code never holds an address, never
chooses when writes happen, and cannot under-deliver a frame's data.

## 2. Core model

Three principles:

1. **Write discipline is declared at buffer creation** and is the handle type.
   No buffer can be written "sometimes":
   - `StorageBufferHandle<T>` — **PerFrame**: CPU data must arrive via
     `FrameInputs` every frame a pipeline that uses it runs.
   - `ImmutableBufferHandle<T>` — GPU never writes; creation mode chooses
     **Static** (written via `write_immutable_all_frames` at setup, never
     again) or **PerFrame** (data mandatory every referenced frame).
   - `GpuOnlyBufferHandle<T>` — CPU writes only at setup
     (`write_gpu_only_all_frames`); GPU reads/writes during the loop.
2. **Per-frame data is an argument, not an action.** Codegen emits a
   `FrameInputs` struct per shader; every pipeline *used* in a frame requires
   one `frame_inputs` call. Skipping a write is impossible (the argument is
   non-optional); partial delivery is impossible (the struct has all the
   fields). There is **no dirty-flag mechanism of any kind** — no `Unchanged`
   variant, no copy-forward.
3. **Addresses are resolved at write time.** Pointer fields hold
   *handle references*, not addresses; `frame_inputs` resolves them against
   its own `ring_slot` as it writes. Staleness isn't merely prevented — it's
   unrepresentable: a handle ref kept across frames still resolves to the
   correct current slot, and the borrow checker already stops the game struct
   from storing refs to its own handles (self-referential).

## 3. API surface

### Creation (setup, unchanged shape plus one new parameter)

```rust
let particles  = renderer.create_gpu_only_buffer::<Particle>(N)?;          // GpuOnly
let spheres    = renderer.create_storage_buffer::<Sphere>(MAX)?;           // PerFrame
let palette    = renderer.create_immutable_buffer::<Vec4>(K, ImmutableMode::Static)?;
let weights    = renderer.create_immutable_buffer::<f32>(K, ImmutableMode::PerFrame)?;
let params     = renderer.create_uniform_buffer::<SimParams>()?;           // uniform: always per-frame
```

Uniform handles stay explicit and shareable across pipelines (watercolor binds
one params buffer to 2–4 parity pipeline variants); `Resources` at pipeline
creation is unchanged. Setup init keeps `write_gpu_only_all_frames` and
`write_immutable_all_frames` (Static mode only); `write_storage_all_frames` is
deleted (PerFrame buffers get data every frame anyway).

### Handle-ref constructors (replace all addr minting)

```rust
gpu_only.current()      // writable ref: compute output slot
gpu_only.previous()     // read-only history ref (the ping-pong input)
storage.write(&slice)   // PerFrame storage: ref + this frame's data travel together
immutable.read()        // Static immutable: ref, no data
immutable.write(&slice) // PerFrame immutable: ref + data (mode-checked at runtime)
```

These return small borrow-carrying values (`&'a Handle` + role (+ data));
they are consumed by `FrameInputs` fields and resolved inside `frame_inputs`.
`Addr`/`ReadAddr`/`ImmutableAddr` remain as the 8-byte GPU-layout types but
become internal to the renderer/codegen write path — no public minting.

### Per-frame delivery

```rust
fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
    renderer.frame_inputs(&self.compute_pipeline, particles_compute::FrameInputs {
        sim_params: particles_compute::SimParamsInput {
            particles_in: self.particle_buffer.previous(),
            particles_out: self.particle_buffer.current(),
            delta_time,
        },
    });
    renderer.frame_inputs(&self.render_pipeline, particle_render::FrameInputs {
        render_params: particle_render::RenderParamsInput {
            particle_count: NUM_PARTICLES,
            particles: self.particle_buffer.previous(),   // fixed-role: previous, not current
        },
    });

    renderer.dispatch(&self.compute_pipeline, workgroup_count, 1, 1);
    renderer.draw_vertex_count(&self.render_pipeline, vertex_count)?;  // no closure
    Ok(())
}
```

- One `frame_inputs` call per pipeline **used this frame**; the writes happen
  immediately in the call (any point between the previous frame's timeline
  wait and this frame's submit is safe under the ring proof — the old step-2
  closure placement was ordering convenience, not a safety requirement).
- `dispatch`/`draw_*` keep their shapes minus the closure. Dispatch count is
  independent of data delivery: watercolor's Jacobi pipeline at 20 iterations
  still takes its inputs once.
- `_padding_0`-style fields disappear from user code: Input structs are
  natural Rust structs; padding is codegen's problem in the write function.

## 4. Codegen

Per parameter block, codegen emits an `*Input` struct mirroring the block with
pointer fields replaced by **stage-aware ref types**; per shader, a
`FrameInputs` struct nesting one field per block (no name collisions, and the
per-block struct is the dedup comparison unit):

| slang field type | compute-stage Input field accepts | graphics-stage Input field accepts |
|---|---|---|
| `Addr<T>` (writable) | `gpu_only.current()` only | — (writable pointers are compute-only) |
| `ReadAddr<T>` | `storage.write(..)`, `gpu_only.previous()`, `gpu_only.current()` | `storage.write(..)`, `gpu_only.previous()` **only** |
| `ImmutableAddr<T>` | `immutable.read()` / `immutable.write(..)` | same |

- **Fixed-role enforcement is static**: reflection knows each block's stage,
  so the graphics-side ref type simply has no constructor from `current()`.
  A block shared across stages takes the stricter (graphics) typing.
- **Write function replaces memcpy**: Input structs no longer match GPU
  layout (ref fields differ in size, padding fields are gone), so codegen
  emits a per-block write function — a fully unrolled sequence of field
  stores using the offsets it already computes for the layout asserts, with
  pointer fields resolved through the renderer at the call.
- **Descriptor-bound PerFrame storage buffers** (the non-BDA path, where the
  handle was fixed in `Resources`) appear in `FrameInputs` as plain `&[T]`
  slice fields.
- Templates: `templates/*.askama` gain the Input/FrameInputs emission; all
  insta snapshots churn (expected); `just shaders` regenerates.

## 5. Runtime

Per-frame state in the renderer:

- **Written-handle set** (by handle index): `frame_inputs` writes each
  referenced buffer's current slot on first sight; a later provide of the
  same handle in the same frame is **skipped, with a debug assert that the
  bytes equal what was written**. This is what makes watercolor's Jacobi case
  safe: both parity variants run in one frame, both bind the same params
  buffer, both `FrameInputs` carry identical data — first writes, second
  verifies.
- **Used-pipeline tracking**: `dispatch` and the draw call record pipeline
  indices. At submission, validation asserts every used pipeline with at
  least one per-frame block had `frame_inputs` called this frame. Keyed on
  *used*, not created: watercolor's ~13 idle parity twins owe nothing, and
  the conditional brush stage owes nothing on frames it doesn't dispatch.
- **Immutable-mode checks**: `immutable.write(..)` on a Static-mode handle
  (or `read()` on a PerFrame-mode handle) is a runtime error.
- Sets clear after submission. Egui and picking internals manage their own
  buffers and are exempt.
- **Partial slices**: `storage.write(&slice)` with `slice.len() < buffer len`
  writes only the prefix; shaders must bound reads by a count delivered in
  the same frame's uniforms (the existing `point_count` pattern). The tail
  beyond the slice is stale per-slot and **undefined to read** — see Future
  work.

## 6. Enforcement matrix

| Footgun | Old status | New status |
|---|---|---|
| Address stashed across frames | UB-adjacent race | **Unrepresentable** (no address values exist; refs resolve at write) |
| Graphics reads gpu-only `current()` | doc comment | **Compile error** (stage-aware ref types) |
| CPU write to gpu-only mid-loop | compile error (since GpuOnlyBufferHandle) | unchanged |
| Skipped per-frame write → flicker | silent visual bug | **Impossible** (mandatory FrameInputs) + completeness assert |
| Shared handle, disagreeing data | silent last-write-wins | **Debug assert** (byte equality on dedup) |
| Immutable written occasionally | silent flicker | **Runtime error** (creation mode + mode checks) |
| Stale slice tail read | silent stale data | **Documented-undefined** (future work) |
| Skipped dispatch in a live ping-pong chain | 3-frame sim rewind stutter | unchanged (see Future work) |

## 7. Migration notes (big-bang: all examples in one change)

The `Gpu` struct, its closure parameter on draw calls, and all its methods are
deleted. Highlights beyond the mechanical rewrite:

- **particles**: render pass switches from `current()` to `previous()`
  (fixed-role) — output is one frame stale, and the compute→vertex
  `memory_barrier` call is deleted (nothing in-frame to order against).
  `_padding_0` fields vanish from draw code.
- **watercolor**: ~9 `frame_inputs` calls per frame (one per used stage
  variant); the Jacobi both-parity frame exercises dedup; the conditional
  brush stage provides inputs only on frames it dispatches. Its ping-pong
  *textures* and parity pipeline scheme are untouched (textures are outside
  the buffer ring).
- **ray_marching / gpu_picking / sprite_batch**: `write_storage` +
  count-uniform becomes `storage.write(&slice)` inside FrameInputs with the
  count in the same struct — the partial + count pattern, now atomic per
  frame.
- **sort_storage_by** had no example users; sorting happens on user data
  before providing the slice.

## 8. Future work

- **Variable-length data without the undefined tail** (explicit note from the
  design interview): a length-carrying buffer abstraction — e.g. the write
  records this frame's count per slot and the generated shader binding
  exposes it, or a `BoundedSlice` input type whose count field codegen wires
  to a designated uniform — so reading beyond the provided data becomes
  checkable rather than documented-undefined.
- **Descriptor-path `StorageBufferFrameStrategy::PingPong`**: unused since
  the BDA migration and unenforceable by this design (it lives at pipeline
  creation, not delivery). Candidate for outright removal during
  implementation.
- **Dispatch-skip detection for gpu-only chains**: a live ping-pong buffer
  whose producing pipeline doesn't dispatch for a frame silently rewinds
  3 frames of state next read. Debug-mode tracking (buffer → producing
  pipeline → dispatched-this-frame) could assert on the gap.
