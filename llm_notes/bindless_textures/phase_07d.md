# Phase 7d — a non-ringed buffer for upload-once static data

Detailed plan for Phase 7d of [../bindless_textures.md](../bindless_textures.md).
**Status: not started. Follow-up, not a prerequisite for anything** — Phase 7c and
Phase 9 both work without it. Written down because 7c's design is what made the
waste visible.

## Correction to the framing, first

It is **double**-buffering, not triple. `MAX_FRAMES_IN_FLIGHT = 2`
(renderer.rs:85).

[`../render-graph/05_multi_draw_rendering.md`](../render-graph/05_multi_draw_rendering.md)
§13.1 says static material data "pays 3x memory and a per-frame address mint for
data that never changes". That was true before the ring shrank from 3 to 2, and is
now stale *within its own paragraph* — the same section notes the shrink a few
lines earlier ("the ring shrank from 3 to 2, so the staleness is nearer but no less
wrong") without updating the multiplier below it. Fix that line when this phase
lands.

So the memory argument is 2×, and it is the **weaker** half of the case. The
motivation is address stability, not bytes.

## The problem

`create_immutable_buffer` (renderer.rs:1035) allocates
`[RawStorageBuffer; MAX_FRAMES_IN_FLIGHT]` via `create_storage_buffers_per_frame`
(:1051), and `get_device_address_for_frame_immutable` (storage_buffer.rs:146)
indexes by slot. So a buffer holding data that never changes after setup costs:

- two allocations,
- two uploads (`write_immutable_all_frames`, :1098, which loops over slots), and
- **a different device address every frame**, for bytes that are identical every
  frame.

The third is the one that matters. Every consumer must re-mint per frame, and a
cached address is silently wrong one frame later — the same wrong-slot failure
class as a wrong bindless heap index, with no validation backstop.

## Why the ring exists, and why it is not always needed

`ImmutableBufferHandle` means "the GPU never writes it", not "nobody writes it".
Its own doc says so (storage_buffer.rs:22-27):

> A storage buffer that nothing on the GPU ever writes …
> The CPU may still update it between frames via `Gpu::write_immutable`

and `sprite_batch` does exactly that every frame
(examples/sprite_batch/src/main.rs:150). **That per-frame CPU write is what
requires slots**: a single allocation would be written while the GPU still reads
the previous frame's copy.

Data uploaded once at setup and never touched again requires no slots at all. The
type cannot currently distinguish the two intents, so it pays for the stricter one
always.

## Shape of the fix

A **distinct handle type** — `StaticBufferHandle<T>` or similar — with one
allocation, a stable address, and **no `Gpu` write accessor at all**: a single
setup-time `write_static`, replacing the `_all_frames` loop.

The type distinction is the safety mechanism, not decoration. If
`Gpu::write_immutable` (:5406) were reachable for the new type, the single
allocation would be written while the GPU reads it. The absence of that method is
what makes one allocation sound, exactly as the absence of an `Addr` accessor is
what makes `ImmutableBufferHandle` un-writable on the GPU today.

**Existing `ImmutableBufferHandle` stays as-is** for the rewritten-per-frame case.
`sprite_batch` does not migrate; `toon_link`'s `Material` array does.

Minting becomes trivial: no `flight_slot` parameter, so a `&self` method on
`Renderer`, `FrameRenderer` and `Gpu` all return the same value, and an app may
legitimately cache the address for the lifetime of the buffer.

## Interaction with Phase 7c

A stable address makes 7c's flight-slot agreement **trivially true** for static
buffers, so 7c's two-surface equality test becomes vacuous *for this type*. But
7c is still needed for the API surface, and still needed for
`ImmutableBufferHandle`, which keeps its ring. **7d simplifies 7c's hardest claim
rather than replacing 7c.**

Do 7c first regardless: it is on Phase 8's critical path, and 7d is not on
anything's.

## Relationship to existing analysis

`05` §13.1 already asks for this under a different name — "a non-ringed,
GPU-owned buffer resource — one allocation, stable address, seeded at setup" —
and argues it needs no new synchronization, since consecutive frames are ordered
by the barrier at the top of each command buffer.

That section's ask is **broader** than this phase. It covers two things:

- the CPU-uploaded, GPU-read-only case (this phase), and
- a `Persistent` variant that is GPU-**written** in place — accumulators, atomic
  counters, append/free lists, spatial hashes.

The second is a larger, separate piece of work with real hazard-tracking
consequences (§13.1's "hazard-identity mismatch" bullet: the graph keys the
last-writer table on the handle while the memory identity is
`(handle, flight_slot)`). **7d is only the first half.** The entry should say so
and cite §13.1 rather than restating it, so the two don't drift.

## Sizing, so the motivation stays honest

`toon_link` is the motivating case and its numbers are modest. `Material` is
~1.3 KB by Phase 9's own layout table (`GXAlphaCompare` at offset 1344), so ~24
materials is ~32 KB, doubled to ~64 KB. That is small enough that the entry must
be explicit: this is about address stability and removing a wrong-slot failure
mode, not about saving memory. If it were only bytes, it would not be worth a
new handle type.

## Verification

`cargo check --workspace --all-targets`, `just test`, `just lint`, `cargo fmt`,
`just sweep`.

- **Zero snapshot churn**: renderer-only, no codegen or reflection changes.
- A test that the address is genuinely stable — mint across two consecutive
  frames and assert equality. That is the whole point of the type, and it is
  what a future refactor reintroducing the ring would break.
- A migration of one real consumer is what proves it end to end. `toon_link`
  cannot be it until Phase 9 lands, so the honest options are (a) land 7d after
  Phase 9 and migrate `toon_link`'s materials as the proof, or (b) land it
  earlier with only unit coverage and accept that no example exercises it.
  Prefer (a): an untested allocation path is exactly what tech-debt §14 warns
  about for multiple `ParameterBlock`s.
