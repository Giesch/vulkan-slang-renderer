# Phase 7b — reject implicit push constants from entry point uniforms

Detailed plan for Phase 7b of [../bindless_textures.md](../bindless_textures.md).
**Status: not started.** Line numbers verified during the Phase 7 session.

Phase 7 gated `[[vk::push_constant]]` **globals**, which is the only path
`reflect_global_parameters` can see. Slang has a second one, and it walks past
every guard Phase 7 added.

## Goal

After 7b, `pushConstantRanges` has exactly one source: an annotated global.
Every other route into that array is a reflection-time error naming the
offending parameter, and Phase 8's "assert at most one range" becomes enforced
rather than assumed.

## The measurement this rests on

Throwaway probes against `prepare_reflected_shader`, all `.shader.slang`,
compiled through the normal path (`CompileTarget::Spirv`, `-O high`):

| declaration | result |
|---|---|
| `fragMain(uniform Tint t)` | **accepted**; `globalParameters: []`, ranges `[{fragment, 0, 32}]` |
| `uniform` structs on *both* stages | **accepted**; ranges `[{vertex, 0, 16}, {fragment, 0, 16}]` |
| global push block **+** `fragMain(uniform Tint t)` | **accepted**; ranges `[{all, 0, 16}, {fragment, 0, 16}]` |
| `fragMain(uniform float4 a)` | `todo!()` at reflection/parameters.rs:81, "type kind reflection not implemented: Vector" |
| `vertMain(…, uniform A pa)` | `todo!()` at build_tasks.rs:340, "field without vk format in entry point parameter: glam::Vec4" |

The slang documentation confirms this is intended compiler behaviour rather
than an accident, at `docs/user-guide/a2-01-spirv-target-specific.md:206-208`
(vendored checkout, tag `v2026.13.1-static`):

> By default, a `uniform` parameter defined in the parameter list of an
> entrypoint function is translated to a push constant in SPIR-V, if the type of
> the parameter is ordinary data type (no resources/textures). … All push
> constants follow the std430 layout by default.

So this is a permanent feature of the language, not a version quirk to wait out.

## Why each row matters

**Row 1 — a range that nothing can write.** Codegen reads only the *vertex*
entry point's parameters (build_tasks.rs:306); the fragment ones are used for
nothing but the entry point name. The uniform struct therefore gets no generated
type, no `PushConstants` alias and no `Resources` entry, while a real
`vk::PushConstantRange` still reaches the pipeline layout via `ToVk`.

**Row 2 — Phase 8's assert is falsified.** That assert was justified on a
*global* block reflecting as a single `All` range, which is correct and
incomplete: two entry point uniforms produce two ranges with no global block
involved. Distinct stages may overlap, so this is legal Vulkan — only invisible,
not invalid.

**Row 3 — an invalid layout.** `all` includes `FRAGMENT` and the second range
*is* `FRAGMENT` at the same offset, which is
`VUID-VkPipelineLayoutCreateInfo-pPushConstantRanges-00292`: two ranges must not
include the same stage. This fails at `vkCreatePipelineLayout` with nothing
upstream explaining why.

**Rows 4-5 are accidents, not guards**, and row 5 is the worst finding here.
`collect_graphics_shader_data` treats *any* struct parameter on the vertex entry
point as the vertex input type and starts building
`VertexInputAttributeDescription`s from its fields — it panicked only because
`float4` has no vertex format. A `uniform` struct of `float3`/`float2`/`uint`
fields would sail through and generate an `impl VertexDescription` for a push
constant block. **This is a pre-existing bug, older than the bindless work and
reachable today with no push constants involved.** The information needed to
tell the two apart is already in the json and simply unused: a real vertex input
binds `varyingInput`, a promoted uniform binds `uniform`.

## Deliverables

1. `crates/slang-reflection/src/reflection/parameters.rs` — one guard in
   `reflect_entry_points`' parameter loop (:36-85), rejecting an entry point
   parameter whose binding occupies bytes rather than being a varying. Reuse
   `Binding::occupies_bytes` (json/parameters.rs), which already draws exactly
   this line and is the same predicate the enum/array/handle field guards use.
2. Rejection tests via `reflect_rejected_shader` (build_tasks.rs) for all three
   accepted rows, asserting the message names both the parameter and the entry
   point.
3. The compute twin via `reflect_rejected_compute_shader` on a `.compute.slang`
   fixture — the same promotion applies to `computeMain(uniform T)`.
4. Rewrite Phase 8's first bullet in the parent doc: the at-most-one-range
   assert stops being conditional once this lands.

## Deliberately not done

Supporting entry point uniforms as a second push constant channel.
`add_push_constatant_range_for_constant_buffer` hard-codes `offset = 0`
(reflection/pipeline_layout.rs:53-75) on the assumption of one range per shader,
and the annotated-global form is the one with codegen, a generated type and a
place in Phase 8's API. Two channels would need offset assignment, a merge rule
for stage flags, and a codegen story for the entry-point struct — all to support
a spelling nobody in this repo uses.

## Verification

`just test`, `just lint`, `cargo check --workspace --all-targets`, `cargo fmt`.

**The pass condition is zero snapshot churn.** No existing fixture or example
declares an entry point uniform, so a green `just test` with no accepted
snapshots is the evidence that the guard is tight rather than merely loud. A
guard that also rejected something legitimate would show up as a fixture
suddenly failing to reflect.

The vertex-input half deserves its own explicit check, since it is the
pre-existing bug: a fixture with `vertMain(uniform SmallStruct s)` whose fields
are all `float3`/`float2`/`uint` — the shape that would *silently* generate a
wrong `VertexDescription` today rather than hitting the `vk::Format` `todo!()`.
Confirm it is rejected by the new guard and not by the format match.
