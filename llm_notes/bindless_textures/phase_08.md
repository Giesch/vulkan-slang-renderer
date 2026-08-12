# Phase 8 — push constants: renderer and per-draw API

Detailed plan for Phase 8 of [../bindless_textures.md](../bindless_textures.md).
**Status: not started**, written 2026-08-11. Line numbers verified against
`6a73e29` (the commit that landed 7d); re-check them before editing, since
`renderer.rs` is 5936 lines and every phase so far has moved them.

Almost renderer-only: no reflection change and no JSON change, but §2.2 adds
**one** codegen line (the `PushConstantBlock` marker impl) plus its fixture stub.
That costs the "zero snapshot churn" tripwire the earlier phases used, so §6
replaces it with the tighter one-named-snapshot form.

## Why this phase exists

Phase 7 made push constant blocks reflect and generate; **nothing writes one**.
The range is built by reflection, reaches the JSON, and `ToVk` turns it into a
`vk::PushConstantRange` at renderer.rs:5418 — then `vk_create` feeds it to
`create_pipeline_layout` (:5333) and `ShaderPipelineLayout` (:5067-5079) drops it
on the floor. `cmd_push_constants` is called nowhere in the workspace; the only
two hits for the name are comments in cli tests explaining its absence
(build_tasks.rs:3059, :3248).

Phase 9 (`toon_link`) needs a per-draw material selector, and this renderer still
has no per-draw data channel: descriptor sets are bound per-pipeline with no
dynamic offsets (renderer.rs:2147, `&[]`), and every draw records
`cmd_draw_indexed(index_count, 1, first_index, 0, 0)` (:2165). Phase 8 is the
renderer half that closes that.

**7c settled this phase's open design question.** With
`FrameRenderer::current_immutable_addr_at` (renderer.rs:5629) and
`singleton_addr_at` (:5646) landed, addresses can be minted at queue time — so the
payload stays **queue-time bytes** rather than switching to
[`../render-graph/05_multi_draw_rendering.md`](../render-graph/05_multi_draw_rendering.md)
§4's closure-fills-the-block alternative. Those are incompatible designs, not
styles, which is why 7c had to come first.

## 1. Retain the range

`ShaderPipelineLayout` gains one field beside `bindless_heap_set`:

```rust
/// The single `All`-stage range this pipeline's push block declares, if any.
/// Kept because `cmd_push_constants` needs the stage flags, size and offset
/// that `vk_create` otherwise consumes and discards.
push_constant_range: Option<vk::PushConstantRange>,
```

Populate at the two `Ok(ShaderPipelineLayout { .. })` sites — renderer.rs:5117
(`#[cfg(debug_assertions)]`, source `reflection_json.pipeline_layout`) and :5138
(release, source `reflected_layout`). Both already have `push_constant_ranges` in
scope, so `vk_create`'s return tuple does not change.

Add a free helper beside the `ToVk` impl at renderer.rs:5418:

```rust
fn single_push_constant_range(
    ranges: &[shaders::json::ReflectedPushConstantRange],
) -> Option<vk::PushConstantRange>
```

which asserts `ranges.len() <= 1` and maps through the existing `to_vk()`.

**The ≤1 claim is enforced upstream rather than assumed**, and that is the whole
reason 7b exists. Three separate guards hold it up: a *global* push block reflects
as **one** `All`-stage range, because `current_stage_flags` is `All` when
`add_global_scope_parameters` reaches it (pipeline_layout.rs:272), so vertex +
fragment do not produce two; Phase 7b rejected the second, *implicit* source
(`uniform` entry point parameters, which no guard over globals could see); and
codegen rejects a second declared block outright
(`a_second_push_constant_block_is_rejected`, build_tasks.rs:2983).

> **Correction to the parent doc.** It says "populate it at the same four
> `create_from_atlas` sites". There are **two** for `ShaderPipelineLayout`; the
> other two constructions (:5193, :5213) are `ComputeShaderPipelineLayout`, which
> is Phase 12.

## 2. The payload

```rust
#[derive(Clone, Copy)]
struct PushConstantBytes {
    bytes: [u8; MAX_PUSH_CONSTANT_BYTES],
    len: u32,
}
```

on `PendingDrawCommand::Draw` (renderer.rs:5570), **not** inside `DrawCallConfig`
(:5831) — that enum is `Copy` and shared by all three draw shapes.
`Option<PushConstantBytes>`, so "declares no block" stays distinct from "a
zero-length block"; that distinction is exactly what the two-direction assert in
§5 reads.

Inline `[u8; 128]` rather than `Vec<u8>`: 128 is the spec floor so it is exactly
right-sized, it keeps the variant `Copy`, and it costs no allocation per draw per
frame. `multi_mesh` queues 18 draws → ~2.3 KB in a `Vec` that is already reused.

**Where 128 comes from.** There are **three** copies to reconcile, not two:
`MAX_PUSH_CONSTANTS_SIZE`, private to crates/cli/src/build_tasks.rs:41; the
hand-typed literal `128` in the emitted assert at
shader_atlas_entry.rs.askama:99; and the buffer size this phase adds.
**Recommended: move the constant to `mltrs-slang-reflection`** as a `pub const`
(under the name `MAX_PUSH_CONSTANT_BYTES` this doc uses throughout).
Both crates already depend on that one (renderer/Cargo.toml:11,
cli/Cargo.toml:15), so the codegen budget assert and the renderer's buffer
become literally the same number instead of two constants that can drift apart
silently. The template copy is unified differently — thread the constant into
the atlas-entry template context so the emitted literal is *generated from* it
(§2.2 item 4); the rendered output is still the text `128`, so that part has
zero snapshot impact. The drift this closes is real: bump the constant alone
and the cli budget assert loosens while every generated `<= 128` stays strict,
failing consumers at compile time with no hint of why. It is otherwise a pure
move — no JSON change, no snapshot change. If all of this reads as scope creep,
declare a renderer-side constant *and* a comment naming the cli one; do not
duplicate the bare literal.

**Byte capture:**

```rust
impl PushConstantBytes {
    fn from_value<P: PushConstantBlock>(value: &P) -> Self {
        const { assert!(size_of::<P>() <= MAX_PUSH_CONSTANT_BYTES) };
        // mirrors write_to_gpu_buffer (gpu_write.rs:16-28)
        ...copy_nonoverlapping(value as *const P as *const u8, ...)
    }
}
```

An inline `const { }` block can reference the enclosing fn's generic parameters,
so the budget check is compile-time — matching the generated
`const _: () = assert!(std::mem::size_of::<DrawConstants>() <= 128)`
(shader_atlas_entry.rs.askama:99). If it will not compile that way, fall back to a
release `assert!` rather than dropping the check.

**Reading a generated block as bytes is sound here, not merely conventional.**
Codegen emits *explicit* padding fields — the push_constants fixture snapshot has
`pub _padding_0: [u8; 4]` and `pub _padding_1: [u8; 4]` inside `DrawConstants` —
so every byte of a generated push block is an initialized `u8`, and the copy reads
no uninitialized memory. Worth recording; it is the non-obvious part, and it is
what makes this stronger than `write_to_gpu_buffer`'s existing whole-`T` copy
rather than merely as strong.

### 2.1 The marker trait

```rust
pub trait PushConstantBlock: GPUWrite {}
```

in crates/renderer/src/renderer/gpu_write.rs, beside `GPUWrite` (:9). It is the
bound on `from_value` above and on §4's three queue methods.

**Not `GPUWrite` itself.** That marker is workspace-wide GPU *layout*, not push
constants: hand-implemented for `u8`, `f32`, `u32` and `NoVertex` (gpu_write.rs:11-14),
a supertrait of `VertexDescription` (vertex_description.rs:5), and emitted by
codegen for **every** struct carrying a layout annotation — ~24 impls across
`examples/*/src/generated/`, gated only on `alignment.is_some()`
(build_tasks.rs:1224-1226). `from_value::<P: GPUWrite>` would therefore accept
`&f32`, `&u32`, every vertex struct and every uniform/storage element struct in the
workspace. The bound would document nothing and refuse nothing a caller would
plausibly get wrong.

**A supertrait rather than a standalone marker.** `VertexDescription: super::GPUWrite`
is the existing precedent for refining it, and this costs nothing: a push block
already gets `impl GPUWrite` from the same `GeneratedStructDefinition::gpu_layout`
constructor every other layout struct goes through (build_tasks.rs:1062-1067). The
refinement keeps the layout invariant that §2's byte-copy soundness argument rests
on stated in one place instead of restated in two.

**Home is `gpu_write.rs`, not a new module.** The supertrait refinement above
is the reason: the trait belongs beside the invariant it refines. Generated
code does not constrain the choice — §2.2 emits the marker impl fully
qualified, so no import path is involved at all.

**Its doc comment must not copy `GPUWrite`'s.** That one says "must be
repr(C, align(16))", which is already stale generally and specifically wrong here:
a push block is `Alignment::Std430 { struct_alignment }`, so the emitted alignment
is *computed* — the fixture's nested `DrawInner` is `align(8)`. Say instead: std430
layout exactly as reflected, and every byte initialized because codegen emits
explicit `_padding_N` fields (the §2 argument above).

**Do not seal it,** and **do not hand-implement it for primitives.** A bare `f32`
is not a push block; the set is meant to be exactly "structs codegen emitted from a
`[[vk::push_constant]]` global". But the round-trip test at the end of §6 has to
`impl PushConstantBlock` for a local struct — the renderer crate has no generated
shaders to borrow — so a sealed trait would make that test unwritable.

### 2.2 Codegen: three files, and the one that is easy to miss

1. **crates/cli/templates/shader_atlas_entry.rs.askama** — inside the existing
   `match shader_impl.push_constant_type_name` arm (:96-101), emit
   `impl {{ import_root }}::renderer::gpu_write::PushConstantBlock for {{ push_constant_type_name }} {}`
   beside the ≤128 B assert. Fully qualified, so no `use` is needed at all —
   which dissolves the conditional-import question (a `use` under the match
   guard, or a dead import behind `#[allow(unused_imports)]`) and keeps the
   snapshot delta to a single line.
2. **crates/cli/fixtures/check_crate/src/renderer/gpu_write.rs** — a
   hand-maintained 4-line stub of the real trait (`pub trait GPUWrite {}` at :4).
   It must gain `pub trait PushConstantBlock: GPUWrite {}` or `alignment_tests`'
   `cargo check` of the generated `push_constants.rs` fails on an unresolved path.
   **This is the easiest thing in the whole phase to miss** — the stub is nowhere
   near the code being changed and nothing points at it.
3. **The other two templates need no change, deliberately.**
   `shader_compute_entry.rs.askama`: compute push blocks are rejected
   (`a_compute_push_constant_block_is_rejected`, build_tasks.rs:3064).
   `shader_shared_module.rs.askama`: a top-level push block does not hoist into a
   shared module (the known gap at ../bindless_textures.md:805-809). Both emit
   `impl GPUWrite`, so the asymmetry is visible and will look like an oversight —
   it is not.
4. **One `build_tasks.rs` change: pass the ≤128 budget into the atlas-entry
   template context**, so the emitted assert's literal is generated from
   `MAX_PUSH_CONSTANT_BYTES` instead of hand-typed in the template (the third
   copy §2 names; rendered output is unchanged, so no snapshot moves). It rides
   the same path `push_constant_type_name` already takes to the template (:358,
   :371, :406, :530); `collect_push_constant_block` already returns the name,
   so nothing else changes.

## 3. The record loop

A helper modelled on `cmd_bind_bindless_heap` (renderer.rs:2418), holding the
asserts from §5:

```rust
fn cmd_push_constants(
    &self,
    command_buffer: vk::CommandBuffer,
    layout: &ShaderPipelineLayout,
    payload: Option<&PushConstantBytes>,
    shader_name: &str,   // for the assert messages
)
```

Called in the draw loop between `cmd_bind_bindless_heap` (:2151-2156) and the
`match draw_call` (:2158).

**Once per loop iteration is both necessary and sufficient.** Push constant state
survives descriptor-set binds — so the `cmd_bind_descriptor_sets` at :2141 and the
heap bind at :2151 do not disturb it — and is invalidated only by binding a
pipeline with an incompatible layout, which this loop does every iteration
(:2103). Neither hoisting it out of the loop nor pushing it a second time after
the binds is correct.

`shader_name` is already computed at :2092 for the debug label; reuse it rather
than calling `source_file_name()` twice.

The helper also carries `debug_assert_eq!(range.offset, 0, ...)`. Pushing at a
hardcoded offset 0 while the range said otherwise would be
`VUID-vkCmdPushConstants-offset-01795`; Phase 7 measured offset 0 and the
single-range guards in §1 make anything else all but impossible, but the
invariant should be stated where it is relied on. Unlike §5's asserts this one
is an *internal* reflection invariant, not a caller-reachable state, which is
why it stays debug-only.

## 4. Queue API

Three new methods on `FrameRenderer`, each beside its existing twin:

- `queue_draw_indexed_with_push_constants<P: PushConstantBlock>(&mut self, pipeline: &PipelineHandle<DrawIndexed>, push: &P)` (beside :5699)
- `queue_draw_index_range_with_push_constants<P: PushConstantBlock>(&mut self, pipeline, first_index, index_count, push: &P)` (beside :5708)
- `queue_draw_vertex_count_with_push_constants<P: PushConstantBlock>(&mut self, pipeline: &PipelineHandle<DrawVertexCount>, vertex_count, push: &P)` (beside :5737)

**New methods rather than a fourth parameter on the three existing
`queue_draw_*`.** The ~14 existing callers declare no push block, so threading a
parameter through all of them buys nothing the §5 asserts don't already give. This
is a judgement call, and the opposite of what the rejected `firstInstance` design
needed — there every variant had to carry the value or silently ignore it.

Factor `queue_draw_index_range`'s debug bounds check (renderer.rs:5714-5725) into
a private helper both range methods call. It must not be duplicated, and must not
be silently absent from the new one.

**`P: PushConstantBlock` is the bound** — see §2.1 for the trait and for why
`GPUWrite` is too wide to use here.

**Known limitation, narrowed by §2.1 but not closed.** The marker gets the cheap
half: a vertex struct, a `u32`, a uniform element struct — anything that is not a
generated push block — no longer compiles at these call sites. What it cannot do is
tie `P` to *this pipeline's* block, because `PipelineHandle<T>` is parameterized on
the *draw-call marker* (`DrawIndexed` / `DrawVertexCount`, pipeline.rs:72-90), not
on the shader. So two different push blocks of the same size still both pass, and
the size assert in §5 is still the entire runtime check. Closing that means an
associated type on the generated `Shader` — which is what
../bindless_textures.md:796-799 intended and did not deliver (§7) — plus threading
the shader type through `PipelineHandle`, a much larger refactor than this phase
and one that deserves its own decision.

## 5. Asserts

Hard `assert!`s in `cmd_push_constants` — not `debug_assert!`s — covering both
directions:

| state | verdict |
|---|---|
| range + payload, `payload.len == range.size` | push it |
| range + payload, sizes differ | assert — otherwise `VUID-vkCmdPushConstants-offset-01795` |
| range, **no** payload | assert — **undefined data, no validation diagnostic at all** |
| payload, no range | assert — the bytes go nowhere |
| neither | early return, like `cmd_bind_bindless_heap`'s `None` arm (:2425) |

Row three is why this phase carries asserts rather than trusting validation: it is
the only one of the four failure modes with no other symptom. A length mismatch
validation would catch on its own; the assert's value there is failing early with
a shader name attached instead of deep in the record loop.

**Why hard rather than debug.** Validation only runs in debug builds, so in
release rows two and three are completely silent — and unlike the debug-only
precedent this renderer does have (`queue_draw_index_range`'s bounds check,
renderer.rs:5717, which robustBufferAccess semantics backstop into
garbage-but-defined rendering), nothing downstream catches these at all. Row
three's own rationale above — no other symptom, ever — is precisely the argument
for a check that survives into release. Row four goes hard too, purely for
uniformity; the total cost of all of them is an `Option` + `u32` compare once
per draw-loop iteration.

**Picking.** The picking path records its own hardcoded `cmd_draw(3, 1, 0, 0)`
(renderer.rs:1872) and has no channel that could carry bytes. Check at pipeline
*creation*, not in the record loop: in `create_picking_pipeline`
(renderer.rs:1262), after `create_from_atlas`, an
`anyhow::ensure!(layout.push_constant_range.is_none(), ...)` naming the shader
and saying picking has no push-constant channel. This is not an internal
invariant — the picking layout is built from a *user-supplied* atlas entry, and
a user picking shader declaring a push block passes reflection and codegen
without complaint — so it deserves an `Err` once, at introduction time, that
also works in release; ~~a `debug_assert!` beside `cmd_bind_bindless_heap` at
:1864~~ (the originally planned form) would fire per-frame, late, and only in
debug. Either way the check is stronger and simpler than the two-direction
form — nothing there could ever supply a payload, so the only correct state is
"no range".

**egui.** Checked: `renderer/egui.rs` declares no push constants. Nothing to do.

**Compute.** Reflection already rejects compute push blocks
(`a_compute_push_constant_block_is_rejected`, build_tasks.rs:3064), so
`ComputeShaderPipelineLayout` needs no field and the dispatch path needs no push
call. Phase 12.

**No device-suitability check.** `maxPushConstantsSize`'s 128 B guarantee is the
spec floor, so a gate in `undersized_limits` would be dead code — unlike the
bindless heap limits in Phase 3, which genuinely vary between devices. Phase 7's
compile-time assert is the real check.

## 6. Verification

`cargo check --workspace --all-targets`, `just test`, `just lint`, `cargo fmt`.

**Exactly one snapshot changes, purely additively.** 7c and 7d could claim zero;
§2.2's marker impl gives that up, so the tripwire takes the tighter form Phase 7
itself landed on (../bindless_textures.md:858-865):

- `mltrs_cli__build_tasks__tests__alignment_tests@src__generated__shader_atlas__push_constants.rs.snap`
  gains exactly one line, the fully-qualified
  `impl …::renderer::gpu_write::PushConstantBlock for DrawConstants {}` — no
  `use` line, per §2.2. Nothing else in it moves — not the derives, not the
  `repr`, not one offset assert.
- **Every other generated snapshot byte-identical.** That is where the leak
  detector now lives, and it is still sharp: `push_constants` is the only fixture
  with a push block, so a change anywhere else means the conditional emit in §2.2
  is not actually conditional.
- `just shaders` still changes nothing — no example declares a push block.
- Review, do not blind-accept: `just insta` (`cargo insta test --workspace
  --review`, justfile:152), **not** `cargo insta test --accept`. The whole claim
  here is *which* snapshot moved.
- The `mltrs-slang-reflection` constant move in §2 is still a pure `pub const`
  addition and must not change a snapshot at all — including §2.2 item 4's
  template-context threading, whose rendered output is the same `128` text.

`just sweep` 16 ok / 0 fail, plus `just sweep-self-test`.

### A green sweep proves nothing on its own

The same warning Phases 3, 4 and 7c carry, and it is sharper here: **no example
declares a push block**, so every line this phase adds is unreachable under `just
sweep`. A clean sweep would confirm only that nothing regressed. Force the path.

### The GPU proof: `multi_mesh`, and why it is the right example

`DRAWS` (examples/multi_mesh/src/main.rs:290-292) already queues **P_CUBE twice**,
over two index ranges — same pipeline, same descriptor set, same params uniform
buffer. Push constants then become the *only* thing in the frame that can make the
two halves of the cube differ, so a visible difference cannot be explained by
anything else. No other example has two draws sharing a pipeline: `toon_link`
(main.rs:1186) and the rest are one pipeline per batch, and `sprite_batch` is a
single draw.

Temporary scaffolding, reverted afterwards as in Phases 3-6 and 7c:

1. Add `struct MultiMeshDraw { float4 tint; }` +
   `[[vk::push_constant]] ConstantBuffer<MultiMeshDraw> draw;` to
   `examples/multi_mesh/shaders/source/multi_mesh.shader.slang`, and multiply the
   fragment output by `draw.tint`. Run `just shaders multi_mesh` — this is the
   first time §2.2's marker impl lands in an *example*, so expect
   `examples/multi_mesh/src/generated/shader_atlas/multi_mesh.rs` to gain the
   fully-qualified `impl … for MultiMeshDraw {}` line alongside the struct.
2. Give each `DRAWS` entry a tint and queue through
   `queue_draw_index_range_with_push_constants` (main.rs:399).
3. **Freeze `elapsed` to 0** before capturing. `orbit_angle(elapsed)` and
   `shape_models(elapsed)` both animate, and an animated A/B is not comparable —
   this is precisely the correction 7c had to make for `sprite_batch`, and 7d for
   `ray_marching`.
4. Capture under a real GPU: `SDL_VIDEODRIVER=x11`, `import -window` against the
   window id from `xwininfo -root -tree` — the route Phases 6, 7c and 7d used.

Checks, in ascending order of what they would catch:

- The two P_CUBE halves render in different colours; setting both tints equal
  makes the cube uniform again. That is the entire claim.
- The un-pushed pipelines in the same frame (P_PYRAMID, the panels) still render.
  A shader with no push block must not trip the "carried bytes but no range"
  direction.
- **Negative control, and the one that matters most:** route one of the two
  P_CUBE draws through the plain `queue_draw_index_range` — payload `None` while
  the range is `Some` — and confirm the range-without-payload assert fires
  *before* anything renders, rather than the cube drawing with stale or zero
  data. A missing push is undefined data with no validation output; nothing else
  in the toolchain catches it, which is the whole argument for row three of §5's
  table. ~~delete the `cmd_push_constants` call and confirm the new assert
  fires~~ — self-defeating as first written: §3 puts the asserts *inside*
  `cmd_push_constants`, so deleting the call deletes the assert with it, nothing
  fires, and the cube quietly draws the very undefined data the control was
  meant to prove impossible.
- **Length mismatch:** push a struct one field short and confirm the assert fires
  rather than the VUID. Since §5's asserts are hard, this and the negative
  control above hold in release builds too, where the VUID would never print.

Finally: revert every scaffolding line and re-run `just shaders multi_mesh`, and
confirm the committed artifacts come back byte-identical — including the marker
impl disappearing again, which is the only end-to-end check that §2.2's emit
really is gated on `push_constant_type_name`.

### The one thing testable without a device

A `PushConstantBytes::from_value` round-trip in `renderer.rs`'s test module — a
`repr(C, align(16))` struct with known field values copies to the expected bytes,
and `len == size_of::<P>()`. **The struct must be padding-free**: either its
fields exactly fill the `align(16)`-rounded size, or it carries explicit
`_padding: [u8; N]` fields the way codegen does. `align(16)` rounds the size up,
so a casually chosen field set leaves compiler-inserted padding that
`from_value` would then read — uninitialized bytes, so UB, and "the expected
bytes" would not even be well-defined. This is the same every-byte-initialized
invariant §2's soundness argument rests on; the test has to obey it, not
undermine it. The struct needs a local `impl GPUWrite` +
`impl PushConstantBlock`, which is the reason §2.1 says not to seal the trait. The
negative half — that `from_value(&1.0f32)` no longer compiles — is real but not
worth adding a `trybuild` dependency the workspace does not have; the bound is
visible in the signature.

Everything else here needs a device. `renderer.rs`'s test module is pure functions
and there is still no headless harness — the wall 7c hit and 7d hit again. Do not
re-litigate it, and do not write a vacuous test in its place: 7d's plan proposed a
stability test that reduced to `x == x`, and dropping it was the right call.

## 7. Corrections to `../bindless_textures.md` to make while here

- **§8's last bullet, "A push block cannot carry a BDA address" — now false.** It
  argues that `Gpu` is constructed after every `queue_draw_*`, so an address minted
  in the submit closure does not exist at queue time, and that the fix "belongs to
  that doc". 7c shipped exactly the fix it describes: `&self` minting on
  `FrameRenderer` (`current_immutable_addr_at`, renderer.rs:5629; `singleton_addr_at`,
  :5646). Left as written it reads as a live constraint and argues for a workaround
  nobody needs.
- **§8's first bullet, "the same four `create_from_atlas` sites"** → two for
  graphics; the other two are compute (Phase 12). See §1 above.
- **§7 planned to "surface the type on the generated `Shader` … so Phase 8's API
  can be typed rather than raw bytes" — that did not ship.** The `push_constants`
  fixture snapshot emits the block struct indistinguishably from any other
  GPU-layout struct (same derives, same `repr`, same `impl GPUWrite`) plus the
  ≤128 B assert; there is no `pub const`/`pub type` on `Shader` modelled on
  `WORKGROUP_SIZE`. §2.1's marker delivers that intent by a different mechanism,
  and §4 records what the `WORKGROUP_SIZE`-style version would additionally have
  bought (tying `P` to the pipeline's own block). Worth annotating in place rather
  than leaving §7 reading as done.

## Out of scope

- **Compute push constants — Phase 12**, including the hazard this phase cannot
  see: push constant state is one block per command buffer, not one per bind
  point, so interleaved draws and dispatches clobber each other — but only once
  *both* sides push, which is why it is invisible until Phase 12 lands.
- **Picking integration with the multi-draw queue** (`link_rendering` §4.5).
  Phase 8 only asserts that the picking pipeline declares no block.
- **`toon_link` — Phase 9.** 7c also freed that phase to adopt the
  `ImmutableAddr<Material>`-in-push-block shape `05` §4 specifies, instead of the
  bare `uint materialIndex` it currently plans. That is Phase 9's call, not this
  one's; both work over this phase's channel unchanged.
