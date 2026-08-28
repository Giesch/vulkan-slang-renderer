# BDA footguns in the current codegen API

Status: **catalogue, 2026-07-21.** Companion to
[frame_inputs_api.md](frame_inputs_api.md) (design on hold). Purpose: enumerate
every BDA-related correctness/safety concern in the current API so solutions
can be evaluated per-footgun, without presupposing the FrameInputs design.
Code references verified against main @ 5756fab.

Ring model context — **updated 2026-07-28** by
[remove_pipelined_compute.md](archived/remove_pipelined_compute.md), which deleted
`PRE_WAIT_RING_LEN` and `ring_slot`: there is now one ring of length
`MAX_FRAMES_IN_FLIGHT = 2` (src/renderer.rs:65), indexed by `flight_slot`.
Uniform, storage, immutable, and gpu-only buffers all ring across those 2 slots
(src/renderer/uniform_buffer.rs, src/renderer/storage_buffer.rs); CPU writes and
`addr`/`current_addr` use `flight_slot`, `previous_addr` uses
`(flight_slot + 1) % 2`. CPU writes now happen *after* the frame_timeline wait,
so there is no pre-wait window at all.

## The original three

### 1. Occasional-write flicker
Per-frame CPU writes touch only ring slot N mod 2 (was 3 before 2026-07-28).
A buffer written *sometimes* — the natural dirty-flag pattern — leaves the
slots holding different generations: camera moves at frame 100 → slot updated;
no further writes; frames render *new, old, new, old…* — a permanent 30 Hz
flicker at 60 fps. Halving the ring halved the period; it did **not** fix the
footgun. Only write-every-frame or write-only-at-setup are coherent; the API
allows the incoherent middle. Guard today: none.

### 2. Address stashing across frames
`Addr<T>`/`ReadAddr<T>`/`ImmutableAddr<T>` are `Copy`, `'static`, and encode a
slot chosen at mint time. An addr stashed at frame N and embedded at frame N+1
points at the wrong slot — for gpu-only buffers, a writable pointer into the
history slot in-flight graphics may read; for CPU-written buffers, a slot an
unproven frame may still be reading. (The "CPU rewrites pre-wait" half of this
is gone since 2026-07-28: CPU writes now happen after the frame_timeline wait.
Pointing at the wrong slot's *data* remains.) Guard today: doc comments only.

### 3. Pipelined current-read race — **not applicable; removed with pipelined compute**
*Resolved 2026-07-28 by [remove_pipelined_compute.md](archived/remove_pipelined_compute.md),
by deletion rather than by a guard.* Compute now always runs before graphics in
the same command buffer, with a renderer-emitted compute→graphics barrier, so a
graphics shader reading a gpu-only buffer's *current* slot reads output that is
already visible. There is no concurrent compute to race.

*Original text:* Under pipelined compute (`Renderer::enable_pipelined_compute`),
frame N's graphics waits only on compute N−1, so compute N runs concurrently
with graphics N. A graphics shader reading a gpu-only buffer's *current* slot
(this frame's compute output) races. Types can't see where an address lands
after `.into()`. Guard today: doc comments only.

## CPU-write coherence

### 4. Uniforms flicker too — and have no all-frames escape hatch
`write_uniform` writes only the current slot (src/renderer.rs:5109-5115), so
uniforms have exactly footgun #1. Worse: `write_*_all_frames` exists for
storage/immutable/gpu-only but there is **no `write_uniform_all_frames`** —
a truly constant uniform *must* be rewritten every frame or it reads stale
2 of 3 frames.

### 5. Oversized writes silently truncate — **done**
Not an overflow — every path clamps with `.min()`. All write paths
(`write_storage`/`write_immutable` and the three `write_*_all_frames`
functions) now `debug_assert!` the length before clamping, so oversized
writes are caught in debug builds; in release the tail is still dropped
with zero diagnostics.

### 6. Duplicate same-frame writes: silent last-write-wins
Two pipelines sharing one uniform/storage handle, written twice with
disagreeing data — second write silently wins (src/renderer.rs:5109-5128).
No dirty flag, no diagnostic. The intent mismatch ("each pipeline gets its
own data") is invisible.

### 7. `sort_storage_by` is a triple footgun - **done**
(src/renderer.rs:5130-5142; sole user examples/space_invaders.rs:391)
- Sorts **only the current slot** → same per-slot divergence class as #1
  unless called every frame.
- Sorts **full buffer capacity**, not the written prefix — garbage tail
  participates in the sort.
- Comparator **reads persistently-mapped `HOST_ACCESS_SEQUENTIAL_WRITE`
  memory** (src/renderer.rs:3617-3619) — VMA may place it write-combined,
  where reads are pathologically slow.
- removed 2026-07-21

### 8. Partial writes leave stale tails; count/data can desync
A short `write_storage` slice updates only the prefix; the per-slot tail
holds data from 3 frames ago. Safety depends on the shader bounding reads by
a count uniform delivered *the same frame* — but count and data travel in
separate buffers via separate calls, so nothing ties them together. A count
written without data (or vice versa) reads a stale tail as live.

## Address validity

### 9. Addresses at rest in GPU buffer data (latent)
Codegen emits pointer fields in *any* struct position, including
buffer-element structs (src/shaders/build_tasks.rs:929-965; element path
887-891) — nothing restricts pointers to per-frame parameter blocks. A user
could write element data containing minted addresses at setup
(`write_gpu_only_all_frames`), but an address encodes one fixed slot while
the containing buffer rotates — the stored address is wrong for 2 of 3
slots. No in-tree example does this; nothing prevents it.

## GPU-side hazards

### 10. Barrier discipline is fully manual
**CLOSED 2026-08-06.** All three compute edges are now renderer-owned. The
compute→graphics one was automated 2026-07-28 (`remove_pipelined_compute.md`);
`FrameRenderer::dispatch` now also inserts a conservative COMPUTE→COMPUTE
barrier whenever the previous queued command was a dispatch, and the public
`memory_barrier` was removed along with watercolor's `compute_barrier` helper
and its call sites. Conservative, not derived: dispatches on disjoint resources
are serialized until a parallel-dispatch opt-out exists. The original text
below is false as of that change.

`record_compute_commands` replays exactly the queued Dispatch/Barrier list —
**no implicit barrier** between consecutive dispatches, none between compute
and the render pass (src/renderer.rs:1318-1403, 1441-1443). A forgotten
`memory_barrier` (src/renderer.rs:5273-5286) is a silent GPU race that may
only manifest on some hardware. particles gets it right by discipline
(examples/particles.rs:97-102); nothing checks.

### 11. Same-slot read/write aliasing is unchecked
Nothing prevents passing one buffer's current slot as both read and write
pointer to a single dispatch (`current_addr` twice; `Addr` converts freely
to `ReadAddr`, src/renderer/addr.rs:84-91) — an intra-dispatch data race.
The current/previous pairing in particles is user discipline only.

### 12. Skipped dispatch rewinds ping-pong state
A live gpu-only ping-pong chain whose producing pipeline skips a frame
silently serves stale state on the next read (the ring rotated; nothing
produced). Still true at 2 slots, and *worse*: `previous_addr` now points at
the slot the skipped frame would have written, so one skipped dispatch serves
2-frame-old state rather than 3-frame-old. Known from the FrameInputs interview
(its §8); no detection exists.

## Bounds

### 13. No out-of-bounds backstop on BDA accesses, period
Device creation enables no robustness features (src/renderer.rs:3177-3223) —
and `robustBufferAccess`/robustness2 would not cover physical-storage-buffer
pointer accesses anyway. A BDA pointer carries no length; any shader
indexing bug past the allocation is unbounded GPU UB (reads/writes arbitrary
device memory) with no validation-layer coverage. All bounding relies on
app-delivered counts (#8).

## Dev-loop

### 14. Hot reload never revalidates layout — **done**
Debug hot reload recompiles SPIR-V and swaps the pipeline but now compares
the freshly reflected interface against the build-time reflection embedded
in the binary (`assert_shader_interface_unchanged`, called from both debug
`create_from_atlas` variants): whole-reflection `serde_json::Value`
equality. A successful recompile whose interface diverged from the
generated Rust structs panics with a rebuild instruction instead of
writing old offsets into the new pipeline. Compile errors remain
non-fatal (old pipeline kept); the check is per-shader, so editing one
shader's interface doesn't block reloads of untouched shaders.

## Structural fragilities (safe today by API-shape accident)

### 15. All-slot writes and buffer drops have no in-flight guard
`write_*_all_frames` writes all slots (2 since 2026-07-28) unconditionally;
`drop_storage_buffer`
/ `drop_immutable_buffer` / `drop_gpu_only_buffer` destroy immediately with
no timeline wait (src/renderer.rs:894-932, 934-953). Both are safe **only**
because `&mut Renderer` is unreachable once the loop starts (`FrameRenderer`
holds it privately, src/renderer.rs:5227; Game::update gets no renderer).
Any future API exposing `&mut Renderer` mid-loop — an editor hook, say —
silently reintroduces write-under-read and free-in-use races. The invariant
lives in the API surface, not in the methods.

## Coverage map: which of these the shelved FrameInputs design addressed

| # | Footgun | FrameInputs coverage |
|---|---|---|
| 1 | Occasional-write flicker | Yes (mandatory per-frame inputs) |
| 2 | Address stashing | Yes (no address values exist) |
| 3 | Pipelined current-read | n/a — removed with pipelined compute (2026-07-28) |
| 4 | Uniform flicker / no all-frames | Yes (uniforms in FrameInputs) |
| 5 | Silent truncation | Yes (promoted to panic) |
| 6 | Duplicate-write disagreement | Debug assert (dedup byte-compare) |
| 7 | sort_storage_by | Deleted (CPU-side sort instead) |
| 8 | Stale tail / count desync | Partial (atomic same-struct delivery; tail stayed documented-undefined) |
| 9 | Addresses at rest | **No** (refs only covered parameter blocks) |
| 10 | Manual barriers | Yes as of 2026-08-06 (compute→compute now auto-inserted by `dispatch`; was "partial" — compute→graphics only) |
| 11 | Same-slot aliasing | **No** (compute could still take current() as both read and write) |
| 12 | Skipped-dispatch rewind | No (future work) |
| 13 | BDA out-of-bounds | No (future work: length-carrying buffers) |
| 14 | Hot-reload layout drift | **No** |
| 15 | Unguarded all-slot writes/drops | **No** (structure unchanged) |

Even the full FrameInputs design left 9–15 largely open — solutions should be
weighed per-footgun rather than as one omnibus API.
