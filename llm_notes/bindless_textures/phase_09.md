# Phase 9 — `toon_link`, the actual payoff

**Status: done.** The first real consumer of the whole bindless stack, and the
first example in the repo to declare a push constant block at all.

Headline, both halves measured rather than estimated:

| | before | after |
|---|---|---|
| graphics pipelines | 24 | **5** |
| uniform buffers | 24 (× 2 flight slots) | **1** |
| descriptor bindings in the shader | 2 textures + 1 UBO | 1 UBO |

## 1. The shape that shipped

`examples/toon_link/shaders/source/toon_link.shader.slang`:

```slang
[[vk::push_constant]] ConstantBuffer<ToonLinkDraw> draw;
struct ToonLinkDraw { mltrs::ImmutableAddr<Material> material; }

struct Material {          // std430 pointee
    Sampler2D.Handle tex0;          //    0
    Sampler2D.Handle tex1;          //    8
    TevParams tev;                  //   16
    GXAlphaCompare alphaCompare;    // 1344   (size 1376)
}

struct ToonLinkParams {    // the frame's globals, one block for the example
    mltrs::MVPMatrices mvp;
    DebugMode debugMode;
}
```

Both entry points read `draw.material[0]` directly — no interstage varying, no
`FragVertex` change, no vertex-input change. That is the concrete win over the
`firstInstance` alternative Phase 7 rejected.

`[0]` is the dereference: the pushed pointer addresses exactly one `Material`,
not the array. `vertMain` reads **only** `.tev` through it rather than binding
the whole struct, so the vertex stage never loads the two handles or the
alpha-compare state it doesn't use.

### Why the pointer, not the `uint materialIndex` this section originally planned

Landed as an index first, then switched — the master plan's Phase 9 specifies
`uint materialIndex` with the `ImmutableAddr<Material>` left in the param block,
while `render-graph/05_multi_draw_rendering.md` §4 specifies the pointer, and
Phase 8 (`bindless_textures.md:1301-1304`) and `phase_08.md:578-581` both
explicitly left the choice to this phase. Taking the planned one silently was the
mistake; here is the reasoning that settled it.

**The argument that looked decisive was wrong.** It ran: a merged multi-draw sets
push constants once per command, so per-draw data must come from `gl_DrawID`
indexing a base pointer — therefore the index is forward-compatible and the
pointer is not. That assumed `05` means indirect multi-draw. It does not. `05` is
an ordered draw *list*: N nodes over shared pipelines, each still issuing its own
`cmd_draw_indexed`, and §4 specifies "emit `cmd_push_constants` for each
`PendingDrawCommand` before its `cmd_draw_indexed`, from bytes the queue
carried" — which is what the renderer already does. The words "indirect" and
`gl_DrawID` appear nowhere in that document; the one `gl_DrawID` mention is a
speculative aside at `bindless_textures.md:1558`. **Per-draw push constants are
not going away in any planned work.**

With that gone, the pointer wins on two counts:

- **No selecting expression left.** The uniformity invariant is about an index
  used to select the struct carrying a handle. There is no index. This is the
  "uniform by construction, with no index at all" property Phase 11 praises
  watercolor for, and toon_link now has it too.
- **The slot is bounds-checked on the CPU, at queue time.**
  `current_immutable_addr_at` asserts through `element_byte_offset`. That is
  load-bearing, not belt-and-braces: BDA loads are *not* covered by
  `robustBufferAccess`, so under the index shape an out-of-range slot was an
  unchecked read. Phase 7c says as much — an out-of-range element address is UB
  and a plausible device loss.

The index would only win under true indirect multi-draw (`drawCount > 1`).
Nothing in the repo plans that; if it ever lands, this reverts to a base pointer
in the param block plus a `gl_DrawID` index, which is a small change.

`tev.slang` needed **no signature change**: `evalStages` and `tevSampleTexmap`
still take plain `Sampler2D`, and `fragMain` converts at the boundary
(`Sampler2D tex0 = material.tex0;`). Only the comment at `tev.slang:207` moved —
both its premises were invalidated. It is still a branch on a uniform value, but
uniform now because the material index is a per-draw push constant.

## 2. Every layout prediction in the plan held exactly

Codegen confirmed all four, first try (`just shaders toon_link`):

- `Material` — 1376 bytes, `alphaCompare` at 1344, trailing `_padding_0: [u8; 12]`.
- `TevParams` — 1328 bytes, **byte-identical** moving from std140 to std430, as
  predicted: every member is a `uint4`/`float4` or an array of one, so the array
  stride is 16 under either rule set.
- `GXAlphaCompare` — 32 bytes / align 16 → **20 bytes / align 4**. This is the
  type that forced the design: it must live *entirely* in `Material`, because a
  type reflected under both layouts trips the "shared type has an incompatible
  layout" panic at `build_tasks.rs:1688`. Loud, but better avoided by
  construction, which is what "move it entirely" means.
- `ToonLinkDraw` — `#[repr(C, align(8))]`, size 8 (it was `align(4)`/size 4 under
  the index shape; both are correct, which is the point).

That last one was the open risk going in: `PushConstantBytes::from_value` copies
`size_of::<P>()` bytes and `cmd_push_constants` pushes them at the reflected
`range.offset`, so an over-aligned Rust block would push more bytes than the
range holds. It doesn't — a generated struct's `repr(align)` follows *its own*
std430 alignment, not a blanket 16 (the `std430_nested_structs` snapshot pins
this: `InnerData` is emitted `align(8)`). Reflection agrees:

```json
"pushConstantRanges": [{ "stageFlags": "all", "offset": 0, "size": 8 }],
"bindlessHeapSet": 1
```

`ToonLinkParams` ended the phase at 208 bytes — `mvp` plus `debugMode` — and grew
back to 336 in the follow-up in §4, which moved the frame globals into it.
`Material` correspondingly shrank 1376 → **1312**.

## 3. What the 5 is

`build_materials` (`main.rs`) dedups by `RasterState` with a **linear scan** over
a parallel `Vec<RasterState>` — `Eq + Copy`, 24 candidates, and adding `Hash` to
a renderer type to save 24 comparisons would be backwards. The count is asserted
(`EXPECTED_RASTER_STATES = 5`), so a manifest change that re-explodes it is loud
rather than a silent regression. The derivation is in the const's doc comment.

**Bindless does not collapse toon_link to one pipeline**, and the master plan's
original claim that it would was wrong (corrected there before this phase ran).
`raster_state` varies cull, depth compare, depth write, blend *and* color write
mask per material; none of that is descriptor state, so removing the texture
descriptors leaves it untouched. Bindless removes the *texture*-driven pipeline
explosion; the *state*-driven one survives.

`alpha_compare` is not a sixth dimension — it rides in `Material` as a
shader-side discard.

## 4. The buffer is a singleton — after a follow-up that corrected this section

The phase shipped an `ImmutableBufferHandle<Material>` rewritten every frame, on
the grounds that "the material records are not actually static: `draw` patches
`tev.light_dir` / `tev.light_color` on all 24, plus the environment override on
the lit ones, all from live debug-window state."

**That reasoning was wrong, and the correction is instructive.** Every value being
patched was a frame *global* — `light_dir`, `light_color`, `env_actor_c0`,
`env_actor_k0`, `eflight_konst` are each computed once above the closure and
written identically into all 24 records. Only the *gate* (`chan_control[0].x`) was
per-material, and that is static manifest data. The buffer was being rewritten
every frame to carry globals that had been smeared across 24 copies.

`tev_pack.rs` had said so all along: `light_dir` / `light_color` "are left
zeroed: the manifest has no light data … the game writes it per frame from
`dKy_tevstr_c`." A field the packer cannot fill is a field that does not belong in
per-material data.

**Phase 7d had also already called it** — `bindless_textures.md:1168` says
"`sprite_batch` keeps the ringed type; `toon_link`'s materials move." Shipping the
ringed handle here was a regression against the phase that built the singleton
for this exact consumer, and the plan text was written without checking back
against it.

**The follow-up** moved them into two new `tev.slang` types carried on the param
block, which is already rewritten each frame:

- `GXLights { float4 dir[2]; color[2]; }`, threaded through
  `evalChannel` / `evalRasterColor` as a parameter.
- `GXTevColorOverride { actorC0, actorK0, eflightKonst, eflight }`, applied at the
  top of `evalStages` to **local copies** of the register file and konst table.

`Material` became genuinely static, so the buffer became
`SingletonBufferHandle<Material>`: one allocation, stable address, filled at
creation (`create_singleton_buffer` takes the data — there is no `write_singleton`,
which is why making `Material` static was a prerequisite rather than a tidy-up).

What that bought:

- **No per-frame material upload at all.** The submit closure is now a single
  `write_uniform`; the 24 × 1376 B copy and the `frame_materials` scratch vector
  are gone.
- **`Access.Immutable` is unconditionally true** rather than true-by-timing. The
  ringed version was sound — the CPU write landed before the frame's draws, same
  as `sprite_batch` — but the singleton removes the reasoning step.
- Addresses are still minted per draw, now via `singleton_addr_at`. Kept on the
  draw path deliberately: the mint carries the bounds `assert!`, and BDA loads get
  no `robustBufferAccess` coverage.

**Two invariants moved from Rust into the shader**, and both are silent if broken,
so they are now commented at the point of application in `evalStages`:

- **`.rgb` only, never `.a`** — the game copies existing alpha back before writing
  (`d_kankyo.cpp:1820`, `:1826`), and `sleeve` stage 1's alpha reads K0's.
- **The `chanControl[0].x != 0` gate** — the eye and brow decals are unlit and must
  keep their MAT3 values.

The Rust helper that enforced the first one (`set_rgb`) is deleted, which is worth
noting as a hazard: the rule now exists only as two `.rgb` suffixes in slang, with
nothing failing loudly if someone widens them.

What is given up: the ability to patch arbitrary per-material data per frame from
the debug window. Nothing used it — the UI drives only globals — but live-editing
one material's TEV state would now require reverting to a ringed buffer.

## 5. Uniformity

Worth restating because nothing enforces it: toon_link issues one index-range
draw per batch, so the material is dynamically uniform *by construction*. This
needs no `NonUniformEXT` and no
`shader_sampled_image_array_non_uniform_indexing`.

The pointer shape states this more strongly than the index did — there is no
selecting expression in the shader that *could* diverge, only a pushed address.

Draw-per-material is load-bearing here, not a limitation to design away — it is
what makes the material uniform for free, and a push constant is the cleanest
possible expression of that, being per-draw constant by definition.
`MaterialSlot::push()` is the single place a slot crosses to the GPU, and the
only place the index still exists.

## 6. Verification, and what it is worth

- `just shaders toon_link` — where all of §2 was confirmed.
- `cargo check --workspace --all-targets`, `cargo fmt`, `just lint` — clean.
- `just test` — green, with **zero snapshot churn**, as expected: no `crates/cli`
  change. Churn here would have been a signal, not noise.
- `just sweep toon_link` and the full `just sweep` — 16/16 ok, self-test fired.
  This is *real* evidence for exactly one of the four risks: the
  `EXPECTED_RASTER_STATES` assert runs during `setup`, so a green sweep proves
  the count is 5. It proves nothing about material *selection*.
- **Not run: the visual A/B.** A wrong material index produces no validation
  output at all, so a green sweep is weak evidence for the rest. The four checks
  the master plan lists (tunic/hat colors, eyes compositing through the bangs,
  `sleeve` still double-sided, `RawTex0`/`RawTex1` per batch) each isolate a
  different failure and still need a human at a window.

## 7. What this leaves for Phase 10

The `docs/` work, unchanged — in particular writing the uniformity invariant
down as a hard rule, and saying how it is satisfied *today*: the material index
rides in a push constant. This phase is the pattern that section should point at.

Also now false and listed there: `render-graph/05_multi_draw_rendering.md` §4's
claim that the push-constant path is "completely dead — no `.slang` declares
one". `toon_link.shader.slang` does.
