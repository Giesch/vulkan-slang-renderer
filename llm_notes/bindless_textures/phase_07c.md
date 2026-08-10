# Phase 7c — bounds-checked element addresses, mintable at queue time

Detailed plan for Phase 7c of [../bindless_textures.md](../bindless_textures.md).
**Status: ✅ done.** Line numbers verified during the Phase 7 session; the
Verification section at the bottom records what actually happened.

Renderer-only: no reflection, no codegen, no template changes.

## Goal

After 7c a push block can carry a BDA pointing at one element of a buffer:

```slang
struct ToonLinkDraw { mltrs::ImmutableAddr<Material> material; }
[[vk::push_constant]] ConstantBuffer<ToonLinkDraw> draw;
```

That shape is what
[`../render-graph/05_multi_draw_rendering.md`](../render-graph/05_multi_draw_rendering.md)
§4 specifies, and it is currently unwritable — which is why Phase 9 plans a bare
`uint materialIndex` instead, leaving the two documents in disagreement.

**Codegen already supports the type.** `gather_struct_defs`' `StructField::Pointer`
arm runs under `generate_std430_struct_fields`, so a pointer field in a push block
generates an `ImmutableAddr<T>` Rust field and emits the pointee struct today. Both
blockers below are renderer-side.

## Blocker 1 — no way to mint an address at queue time

`Addr::from_raw` and its siblings are `pub(super)` (addr.rs:17, :72, :139), so only
the renderer mints. Every public minting method hangs off `Gpu` (renderer.rs:5384-5460),
which is constructed at :2478 and handed to `gpu_update` at :2483 — *after*
`submit_draws` has consumed the queued draws.

**This is structural to `FrameRenderer`, not a property of any one method.**
`Renderer::draw_frame` takes `pending_draws` by value with the closure as a separate
argument (:5691-5703), so the vector is complete before the closure can exist. All
three `queue_draw_*` methods hit it identically, and the one-shot `draw_indexed` /
`draw_vertex_count` wrappers do not escape it — they queue then submit internally
(:5638-5655). Worth stating, because it rules out "just use the one-shot helpers"
as a later workaround, and because it is what makes closure-filled push bytes a
genuine fork rather than a style choice.

## Blocker 2 — no pointer arithmetic, deliberately

To point at material *i* you need `base + i * stride`. `to_raw()` is public but
`from_raw` is not, and there is no `offset`/`element`/`add` method anywhere in
`addr.rs`. That is the invariant the type protects, per its own comment
(addr.rs:136-138):

```rust
// pub(crate): minting is restricted to Renderer/Gpu accessors that take
// an ImmutableBufferHandle, which upholds the never-GPU-written invariant
// Access.Immutable requires.
```

An `ImmutableAddr` is always the base of a whole `ImmutableBufferHandle`, which is
what makes `Access.Immutable` — and the SPIR-V `Restrict` it emits — sound. A
fabricated address is undefined behaviour, not garbage pixels.

## The design

A single new method that takes `(handle, index)`, bounds-checks, and returns an
address to one element solves blocker 2 completely — and solves blocker 1 **only if
it is not only on `Gpu`.** `Gpu` exists solely inside the closure, which is
precisely the wrong side of the queue/submit boundary.

So: put the logic in `StorageBufferStorage` beside `get_device_address_for_frame_immutable`
(storage_buffer.rs:146), and expose it from **both** surfaces.

**Deliverables**

1. `crates/renderer/src/renderer/storage_buffer.rs` — one accessor:

   ```rust
   pub(super) fn get_element_device_address_for_frame_immutable<T>(
       &self, handle: &ImmutableBufferHandle<T>, frame: usize, index: u32,
   ) -> vk::DeviceAddress
   ```

2. `crates/renderer/src/renderer.rs` —
   `Gpu::current_immutable_addr_at(&self, handle, index) -> ImmutableAddr<T>`
   (impl at :5384, beside `current_immutable_addr` at :5454), and the same method
   on `FrameRenderer` (impl at :5498). Add `FrameRenderer::current_immutable_addr`
   (whole-buffer) at the same time — having only the indexed form available at queue
   time would be its own trap.

3. The Phase 7c entry in the parent doc, and the Phase 8 note that this settles its
   payload design.

**Scope: `ImmutableBufferHandle` only.** `StorageBufferHandle` and
`GpuOnlyBufferHandle` are the same shape and can follow when something wants them.
Speculative API here triples the surface to test and buys nothing.

**Landed as three methods plus a free helper, not two methods.** The bounds
check and the stride multiply are a `fn element_byte_offset(index, len, stride)
-> u64` at the bottom of storage_buffer.rs, because it is the only part of the
accessor that can be unit-tested without a device (see Verification).
`Gpu::current_immutable_addr_at`, `FrameRenderer::current_immutable_addr` and
`FrameRenderer::current_immutable_addr_at` all funnel through the one
`StorageBufferStorage` method.

## Why the two surfaces agree — verified, not assumed

`flight_slot` is advanced at renderer.rs:2542, at the *end* of `draw_frame`, before
present, with a comment explaining why ("Advance the frame slot BEFORE present").
So during `queue_draw_*`, `renderer.flight_slot` already holds exactly the value
`Gpu` will be constructed with later in the same frame. A `FrameRenderer` method
reading `self.renderer.flight_slot` returns the identical address.

Minting is also synchronization-free: `get_device_address_for_frame_immutable` reads
a `vk::DeviceAddress` recorded at buffer creation. The timeline wait before
`gpu_update` exists to make *writing* mapped memory safe, and that stays in the
closure. Minting at queue time and writing data in the closure are independent
operations on the same slot, in the correct order.

**This is now pinned in code, not just argued.** The plan asked for a test that
mints from both surfaces in one frame and compares — which cannot be written,
because `renderer.rs`'s test module is pure functions and there is no headless
device harness. The claim is instead asserted where it can actually rot:
`Renderer::draw_frame` captures `queue_flight_slot` at entry and
`debug_assert_eq!`s it against `self.flight_slot` immediately before building
`Gpu`. That runs in every debug frame of every example and every `just sweep`
run, which is strictly more coverage than one test would have given. The advance
site at the end of the method carries a matching comment, and `flight_slot` is
written in exactly two places (`Renderer::new` and that advance) — checked, so
the pair really is exhaustive.

One case the plan did not mention: `draw_frame` can return before `Gpu` exists at
all, via the `ERROR_OUT_OF_DATE_KHR` → `recreate_swapchain` early return. The
queued draws are dropped with it, so addresses minted for that frame are simply
unused; the assert is not reached and nothing is wrong.

## `assert!`, not `debug_assert!`

A deliberate departure from the neighbouring bounds check in `queue_draw_index_range`,
whose comment reads "debug-only: a release-build out-of-range draw renders garbage
silently under robustBufferAccess". **That reasoning does not transfer.**
`robustBufferAccess` covers descriptor-bound buffers and does *not* cover buffer
device addresses — a BDA load bypasses descriptor bounds checking entirely. An
out-of-range element address is undefined behaviour and a plausible device loss,
not a clamped read. The message should name the buffer's `len()` and the offending
index.

## Stride is `size_of::<T>()`, and that is already pinned

Codegen emits `const _: () = assert!(size_of::<T>() == expected_size)` where
`expected_size = align_to(offset, max_alignment)` under std430, and
`pointer_pointee_spirv_layout` (build_tasks.rs) asserts the emitted SPIR-V
`ArrayStride` equals that same number — 112 for its fixture. So the Rust
`size_of::<T>()` *is* the slang std430 array stride, by an existing test rather than
by assumption. It also keeps `base + i * size` correctly aligned, since std430 rounds
struct size up to struct alignment.

## What this unblocks, and what it does not

**Unblocks** the `ImmutableAddr<Material>`-in-push-block shape, letting Phase 9 drop
`materials` from `ToonLinkParams` entirely and stop disagreeing with `05` §4. Also
settles Phase 8's open design question: with queue-time minting proven, Phase 8 keeps
its queue-time-bytes design (inline `[u8; 128]`, `PendingDrawCommand::Draw` stays
`Copy`) rather than switching to `05`'s closure-fills-the-push-block alternative.

**Does not** oblige Phase 9 to change. The index form still works and needs neither
blocker solved; adopting the pointer form is a judgement call for that phase.

**Ordering: 7c before Phase 8**, since Phase 8's payload design depends on the answer.

## Verification

`cargo check --workspace --all-targets`, `just test`, `just lint`, `cargo fmt`.

**Zero snapshot churn** — this phase touches no reflection, codegen or template code.
Any accepted snapshot means something leaked.

Three targeted checks, in ascending order of what they'd catch:

1. **A `#[should_panic]` test for `index == len()`.** Cheap, and pins that the check
   is a release assert rather than a debug one.
2. **A test that the two surfaces agree.** Mint the same `(handle, index)` from
   `FrameRenderer` at queue time and from `Gpu` inside the closure of the same frame;
   assert the raw `u64`s are equal. This is the entire `flight_slot` claim, tested
   rather than argued, and it is the one thing here that would silently rot — a
   future change to where `flight_slot` advances would break it with no other symptom.
3. **Prove it on a GPU before Phase 9 depends on it.** Temporarily convert
   `sprite_batch` to push one sprite's *element* address per draw instead of the
   whole-buffer address in its param block (examples/sprite_batch/src/main.rs:143-151
   is the existing pattern). Output is unchanged if the arithmetic is right and
   visibly wrong if the stride is off. Revert afterwards — as in Phases 3-6, the
   scaffolding is measurement, not a deliverable.

A green `just sweep` proves nothing on its own here: a wrong stride reads valid
mapped memory and renders a plausible image with no validation output.

**Verified:** `cargo check --workspace --all-targets`, `just test`, `just lint`
and `cargo fmt` all clean. **Zero snapshot churn** — `git status` after the whole
phase lists exactly two modified files, `renderer.rs` and `storage_buffer.rs`.
`just sweep` 16 ok / 0 fail with the injected-fault self-test still firing, which
is also what exercises the new `debug_assert_eq!` across all 16 examples.

Check 1 landed as two unit tests on `element_byte_offset`
(`element_offsets_are_index_times_stride` at a deliberately non-power-of-two
stride, and `one_past_the_end_panics`). Check 2 became the in-situ assert
described above.

**Check 3 could not be done as written, and the substitute is stronger.** The
plan said to "push one sprite's element address per draw" — but
`cmd_push_constants` does not exist until Phase 8, so there is nothing to push
into. The param-block route proves the same arithmetic: point
`SpriteBatchParams.sprites` at element `K` and shorten the draw to `(n - K) * 6`
vertices, so the shader's `sprites[id / 6]` resolves to buffer element `K + i`.

`sprite_batch` as committed re-randomizes all 8192 sprites *every frame*, so an
A/B of it is meaningless — and 8192 scattered sprites are exactly the "plausible
image" the warning above is about. So the scaffolding also cut `SPRITE_COUNT` to
4 and gave each a fixed position, a distinct atlas tile and a distinct tint.
Captured under a real GPU (`SDL_VIDEODRIVER=x11`, `import -window` against the
window id from `xwininfo -root -tree`; the same X11 route Phase 6 fell back to):

- `K = 0` — four sprites at x = 100/250/400/550, one per tile/colour.
- `K = 1` — the last three, **unmoved**. `compare -metric AE` over the crop
  containing them reports **0** differing pixels against the `K = 0` capture, so
  this is pixel equality rather than an eyeball.
- `K = 3` — the fourth sprite alone, still at x = 550.

**Negative control, in the style of Phases 3-6:** with `element_byte_offset`
temporarily returning `index * (stride + 4)`, the `K = 1` run renders **nothing
at all** — the off-by-4 read reinterprets the following sprite's bytes as
position and scale and throws the quads off screen. So the check discriminates;
a green capture is not something any stride would have produced. Reverted
afterwards, as was every line of the sprite_batch scaffolding.
