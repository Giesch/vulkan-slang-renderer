# BDA footguns in the current codegen API

Status: **catalogue, 2026-07-21.** Companion to
[frame_inputs_api.md](frame_inputs_api.md) (design on hold). Purpose: enumerate
every BDA-related correctness/safety concern in the current API so solutions
can be evaluated per-footgun, without presupposing the FrameInputs design.
Code references verified against main @ 5756fab.

Ring model context: `MAX_FRAMES_IN_FLIGHT = 2`, `PRE_WAIT_RING_LEN = 3`
(src/renderer.rs:70-75). Uniform, storage, immutable, and gpu-only buffers all
ring across 3 slots (src/renderer/uniform_buffer.rs:23,
src/renderer/storage_buffer.rs:78); CPU writes and `addr`/`current_addr` use
`ring_slot`, `previous_addr` uses `(ring_slot + 2) % 3` (src/renderer.rs:5185).

## The original three

### 1. Occasional-write flicker
Per-frame CPU writes touch only ring slot N mod 3. A buffer written
*sometimes* — the natural dirty-flag pattern — leaves the three slots holding
different generations: camera moves at frame 100 → slot updated; no further
writes; frames render *new, old, old, new, old, old…* — a permanent 20 Hz
flicker at 60 fps. Only write-every-frame or write-only-at-setup are coherent;
the API allows the incoherent middle. Guard today: none.

### 2. Address stashing across frames
`Addr<T>`/`ReadAddr<T>`/`ImmutableAddr<T>` are `Copy`, `'static`, and encode a
slot chosen at mint time. An addr stashed at frame N and embedded at frame N+1
points at the wrong slot — for gpu-only buffers, a writable pointer into the
history slot in-flight graphics may read; for CPU-written buffers, a slot the
CPU rewrites pre-wait while an unproven frame may still read it. Guard today:
doc comments only.

### 3. Pipelined current-read race
Under pipelined compute (`Renderer::enable_pipelined_compute`), frame N's
graphics waits only on compute N−1, so compute N runs concurrently with
graphics N. A graphics shader reading a gpu-only buffer's *current* slot (this
frame's compute output) races. Types can't see where an address lands after
`.into()`. Guard today: doc comments only.

## CPU-write coherence

### 4. Uniforms flicker too — and have no all-frames escape hatch
`write_uniform` writes only the current slot (src/renderer.rs:5109-5115), so
uniforms have exactly footgun #1. Worse: `write_*_all_frames` exists for
storage/immutable/gpu-only but there is **no `write_uniform_all_frames`** —
a truly constant uniform *must* be rewritten every frame or it reads stale
2 of 3 frames.

### 5. Oversized writes silently truncate
Not an overflow — every path clamps with `.min()` — but the tail is dropped
with zero diagnostics: `write_storage`/`write_immutable` have a
`debug_assert!` then clamp (src/renderer.rs:5117-5128, 5144-5159; release:
silent); `write_storage_all_frames`/`write_immutable_all_frames`/
`write_gpu_only_all_frames` have **no assert at all** — silent even in debug
(src/renderer.rs:894-932).

### 6. Duplicate same-frame writes: silent last-write-wins
Two pipelines sharing one uniform/storage handle, written twice with
disagreeing data — second write silently wins (src/renderer.rs:5109-5128).
No dirty flag, no diagnostic. The intent mismatch ("each pipeline gets its
own data") is invisible.

### 7. `sort_storage_by` is a triple footgun
(src/renderer.rs:5130-5142; sole user examples/space_invaders.rs:391)
- Sorts **only the current slot** → same per-slot divergence class as #1
  unless called every frame.
- Sorts **full buffer capacity**, not the written prefix — garbage tail
  participates in the sort.
- Comparator **reads persistently-mapped `HOST_ACCESS_SEQUENTIAL_WRITE`
  memory** (src/renderer.rs:3617-3619) — VMA may place it write-combined,
  where reads are pathologically slow.

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
silently serves 3-frame-old state on the next read (the ring rotated;
nothing produced). Known from the FrameInputs interview (its §8); no
detection exists.

## Bounds

### 13. No out-of-bounds backstop on BDA accesses, period
Device creation enables no robustness features (src/renderer.rs:3177-3223) —
and `robustBufferAccess`/robustness2 would not cover physical-storage-buffer
pointer accesses anyway. A BDA pointer carries no length; any shader
indexing bug past the allocation is unbounded GPU UB (reads/writes arbitrary
device memory) with no validation-layer coverage. All bounding relies on
app-delivered counts (#8).

## Dev-loop

### 14. Hot reload never revalidates layout
Debug hot reload recompiles SPIR-V and swaps the pipeline
(src/renderer.rs:2452-2623) but never compares the reloaded shader's
reflected layout against the compile-time-generated Rust structs (whose
layout asserts run only at Rust build time). Edit a shader's block layout →
the running app writes old offsets into the new pipeline: silent garbage
until rebuild.

## Structural fragilities (safe today by API-shape accident)

### 15. All-slot writes and buffer drops have no in-flight guard
`write_*_all_frames` writes all 3 slots unconditionally; `drop_storage_buffer`
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
| 3 | Pipelined current-read | Yes (stage-aware ref types) |
| 4 | Uniform flicker / no all-frames | Yes (uniforms in FrameInputs) |
| 5 | Silent truncation | Yes (promoted to panic) |
| 6 | Duplicate-write disagreement | Debug assert (dedup byte-compare) |
| 7 | sort_storage_by | Deleted (CPU-side sort instead) |
| 8 | Stale tail / count desync | Partial (atomic same-struct delivery; tail stayed documented-undefined) |
| 9 | Addresses at rest | **No** (refs only covered parameter blocks) |
| 10 | Manual barriers | Partial (compute→graphics eliminated via previous-only; compute→compute still manual) |
| 11 | Same-slot aliasing | **No** (compute could still take current() as both read and write) |
| 12 | Skipped-dispatch rewind | No (future work) |
| 13 | BDA out-of-bounds | No (future work: length-carrying buffers) |
| 14 | Hot-reload layout drift | **No** |
| 15 | Unguarded all-slot writes/drops | **No** (structure unchanged) |

Even the full FrameInputs design left 9–15 largely open — solutions should be
weighed per-footgun rather than as one omnibus API.
