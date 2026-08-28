# Dynamic State: What It Is and the Phase 4 Design (Won't Do)

Companion note to [../vulkan_1_3_migration.md](../vulkan_1_3_migration.md). **Status: Phase 4 was implemented, fully verified, and then reverted before commit — marked won't-do (2026-07-14).** The phase was purely orthogonal to the remaining BDA work (Phases 5–7), and the renderer's only real static pipeline variation is `depth_test_enable`, so the payoff was judged speculative. This note survives as (a) a primer on the mechanism and (b) the preserved, proven-clean design should per-pipeline state variation ever materialize.

Phase 4 moved six pieces of pipeline state (cull mode, front face, topology, and the three depth states) from static pipeline creation to core-1.3 dynamic commands.

## The concept

A `vk::Pipeline` is Vulkan's biggest up-front commitment: shaders, vertex layout, rasterization, blending, depth/stencil — nearly every knob on the GPU — compiled together at creation into one immutable object. That design is deliberate: with everything known ahead of time, the driver can generate final hardware state (and often final shader machine code) once, so binding a pipeline at draw time is cheap and — unlike GL — never triggers a hidden mid-frame recompile.

The cost of that commitment is combinatorics. If any one knob needs two values, you need two pipelines. State that varies at runtime (viewport size, depth on/off, cull direction) multiplies the pipeline count, and each permutation is a separate expensive creation call.

**Dynamic state is the escape hatch.** Any state listed in `PipelineDynamicStateCreateInfo` at creation is *excluded from the baked object*: the value in the static create-info struct is ignored (the struct member must still exist), and the actual value is supplied while recording commands, via a `cmd_set_*` call that must happen before the first draw after each pipeline bind. The contract inverts per state: baked states can't change without a new pipeline; dynamic states *must* be set every time, because the pipeline no longer carries them.

Three generations of this mechanism:

- **Vulkan 1.0 core**: a small fixed menu — viewport, scissor, line width, depth bias, blend constants, stencil masks/refs. These were dynamic-capable from day one because hardware universally treats them as registers, not compiled state.
- **`VK_EXT_extended_dynamic_state` + `_state2`, promoted to core 1.3**: cull mode, front face, primitive topology, depth test/write/compare, stencil op, vertex stride, and more. On 1.3 these need **no feature bit and no extension** — this is the set Phase 4 adopted.
- **`VK_EXT_extended_dynamic_state3`**: still extension-only (not core even in 1.4) — blend enable/equation, polygon mode, MSAA sample count, etc. This boundary is why `blend_enable` stayed a static `create_graphics_pipeline` parameter: making it dynamic would reintroduce a hardware/driver support question that the core-1.3 set doesn't have. (The logical endpoint of this direction is `VK_EXT_shader_object` — no pipeline objects at all — but that's a different architecture, not an incremental step.)

## Benefits

- **Fewer pipeline permutations.** Pipeline creation is the expensive, potentially stutter-causing operation; a `cmd_set_*` is a trivial command-buffer write. Every state made dynamic deletes a ×N from the permutation space. In this renderer the win would be structural rather than numeric — `depth_test_enable` is the only static variation — which is precisely why the phase was cut: the benefit stays speculative until real per-pipeline state variation (a two-sided material, a line-topology debug view) exists.
- **Resize without rebuilds.** This one predates Phase 4 and is unaffected by the revert: viewport/scissor are dynamic, so window resizes and render-scale changes never touch pipelines — `recreate_swapchain` rebuilds images, not pipelines.
- **State-agnostic hot reload.** With the six states dynamic, `try_shader_recompile` would rebuild a pipeline from shader bytes alone. Today it reads `disable_depth_test` back off the stored `RendererPipeline` to reconstruct the same variant — the "remember what this pipeline was created with" plumbing the phase would have deleted.
- **Simpler creation paths.** `create_graphics_pipeline` would lose its `depth_test_enable` parameter; state choices would live in one struct instead of being threaded through creation call chains.

The honest cost: every declared-dynamic state **must** be set after **every** bind, even when it's irrelevant (validation enforces this, e.g. depth states on a pass with no depth attachment). That's six extra tiny commands per graphics bind — negligible on desktop.

## What this codebase actually uses today

Only the original Vulkan 1.0 pair. `create_graphics_pipeline` (src/renderer.rs) declares `VIEWPORT` and `SCISSOR` dynamic, set from `render_extent` inline at each graphics bind site. Everything else is baked: cull `BACK`, front face `COUNTER_CLOCKWISE`, `TRIANGLE_LIST`, and depth write-on/compare-`LESS`, with `depth_test_enable` as the one per-pipeline static parameter (fed by `PipelineConfig.disable_depth_test`, used by `sprite_batch` and `space_invaders`). Hot reload (`try_shader_recompile`) reads `disable_depth_test` off the stored `RendererPipeline` to rebuild the same variant.

## The reverted Phase 4 design (preserved for a possible restart)

Implemented and verified clean on 2026-07-14, then reverted by decision. Two files, no app-facing or codegen changes, zero snapshot churn:

1. **`DynamicPipelineState`** (src/renderer/pipeline.rs, next to `RendererPipeline`): six fields — cull mode, front face, topology, depth test/write/compare — with a `Default` reproducing the hard-coded statics above. It replaced `disable_depth_test` on `RendererPipeline` (and the `cfg_attr(expect(unused))` hot-reload hack on that field). `init_pipeline` set `depth_test_enable` from the config; picking hardcoded depth test *and* write off.
2. **`record(&device, command_buffer)`** on that struct issued all six `cmd_set_*` calls, invoked immediately after both graphics `cmd_bind_pipeline` sites in `record_command_buffer` (main pass + picking pass; the compute bind point has no dynamic state). One helper so the two sites can't drift.
3. **`create_graphics_pipeline`** gained the six `DynamicState` entries and lost the `depth_test_enable` param; the static create-info structs stayed populated as ignored placeholders (Vulkan requires their presence).

Details settled during implementation, worth keeping:

- **The picking pass must set depth state it can't use.** Picking renders with no depth attachment, but declared-dynamic states must still be set before draw — validation checks set-ness, not attachment presence. test=false / write=false is correct (write=false is technically redundant since writes are gated on the test, but states the intent).
- **Hot reload interplay**: the rebuilt `vk::Pipeline` is state-agnostic; `dynamic_state` persists on `RendererPipeline` across the swap and is re-recorded next frame.
- Verification was fully green: check/lint/test with zero snapshot diffs; gpu_picking, suzanne, sprite_batch, space_invaders, viking_room clean under validation in debug; release and hot-reload spot-checks clean.

## What stayed static, and why

- **Blend enable / blend factors** — EDS3, not core 1.3; would cost a feature/extension dependency. Remains the `blend_enable` parameter (picking's uint target needs blending off).
- **Primitive restart, polygon mode, depth bounds test, stencil test** — restart-enable and the stencil *ops* are core-1.3 dynamic-capable, but nothing here varies them; polygon mode is EDS3. Left static under the working rule: **make a state dynamic when it's core and removes real (or plausible) per-pipeline variation — not speculatively.**
- **MSAA sample count** — not dynamic-capable in any core version (EDS3 only, and awkward even there); sample count genuinely is compiled state.

## Rules of thumb (if the design is ever revived)

- Every declared-dynamic state must be set after **every** bind, before the first draw. Route all `cmd_set_*`s through one helper (`DynamicPipelineState::record()` in the reverted design) so the bind sites can't drift. Adding a state to the `dynamic_states` list, the struct, and the helper must be one change.
- Per-pipeline state then lives on `RendererPipeline`, not in the `vk::Pipeline` — vary a state by setting the field at construction, not by adding a `create_graphics_pipeline` parameter.
- The values in the static `PipelineInputAssemblyStateCreateInfo` / `PipelineRasterizationStateCreateInfo` / `PipelineDepthStencilStateCreateInfo` structs become dead for dynamic slots; changing them does nothing. The live fields in those same structs (`primitive_restart_enable`, `polygon_mode`, `depth_bounds_test_enable`, `stencil_test_enable`) still matter.
- A forgotten `cmd_set_*` is loud (a "dynamic state not set" VUID under validation), but *stale* state is quiet: dynamic state persists across binds within a command buffer, so a missing set after a second bind can silently inherit the previous pipeline's values, and validation flags it only in some cases. Helper-after-every-bind sidesteps both.

## Search terms and reading

- `VK_EXT_extended_dynamic_state`, `VK_EXT_extended_dynamic_state2`/`3`, `vkCmdSetCullMode`, `vkCmdSetDepthTestEnable`, `VkPipelineDynamicStateCreateInfo`
- **Vulkan spec §10.11 "Dynamic State"** — the normative list of which states are dynamic-capable in which version, and the set-before-draw rules.
- **Khronos blog: "Reducing Draw Time Hitching with VK_EXT_graphics_pipeline_library"** — good background on why pipeline permutations hurt, even though this repo took the dynamic-state route instead.
- **`VK_EXT_shader_object` extension page** — the "no pipelines at all" endpoint of this design direction; useful for perspective, not adoption.
- The **dynamic rendering** work (Phase 2) pairs well conceptually: dynamic rendering removed the render-pass dependency from pipeline objects; extended dynamic state would have removed most of the remaining mutable state.
