# Phase 12 — push constants in compute shaders

**Status: done, 2026-08-20. 12 pipelines → 10, and the workspace's last bound
texture descriptors became handles.** See [§10](#10-outcome) for the
measurements and for the five places this plan was measured wrong.
Detailed plan for Phase 12 of [../bindless_textures.md](../bindless_textures.md).
Written against `40828ff`; the line numbers below are that snapshot. The work
mirrors [phase_08.md](phase_08.md) one subsystem at a time, and the type encoding
it extends is [phase_08b.md](phase_08b.md)'s push slot.

## Why this phase exists

Watercolor runs 12 pipelines after Phase 11c: 11 compute plus the display
graphics pipeline. The two remaining duplicate pairs vary *per dispatch within
one frame*, which no once-per-frame uniform write can express:

- **`wc_pressure_jacobi` ×2.** `pressure_parity` flips between the two
  dispatches of one frame (main.rs:743-747). The parity is baked into each
  pipeline's descriptor set: `Resources { pressure_in:
  pressure.read_sampled(parity), pressure_out: pressure.write_storage(parity) }`
  (main.rs:494-507). Those two bound slots are the **last per-pipeline texture
  descriptors in the workspace** — every other slot became a heap handle in
  11/11b/11c.
- **`wc_gaussian_blur` ×2.** One shader dispatched twice, H then V. The two
  pipelines differ only in which of two uniform buffers is bound
  (main.rs:520-532); all three varying values — `inputTex`, `outputTex`,
  `direction` — are already handles or plain data in `Params`
  (wc_gaussian_blur.compute.slang:12-17). Two UBOs exist purely because one
  per-frame write cannot carry two values.

| step | pipelines |
|---|---|
| after 11c | 12 |
| jacobi collapses (§6.1) | 11 |
| blur collapses (§6.2) | 10 |

The channel that carries per-dispatch data is the push constant block, and
compute has none: Phase 7's reflection gate rejects it, because Phase 8 wired
the graphics record loop only and reflection accepting something no code path
writes is the failure shape this codegen exists to prevent. This phase wires
the dispatch path and removes the gate.

**There is no technical obstacle, and less remains than Phase 8 had to build:**

- `VK_SHADER_STAGE_COMPUTE_BIT` is a valid `VkPushConstantRange` stage, and
  `vkCmdPushConstants` takes a pipeline layout, not a bind point.
- `reflect_pipeline_layout` is already shared between the graphics and compute
  reflection paths. The moment the gate flips, a compute push block reflects as
  an `All`-stage range and lands in the created `VkPipelineLayout` with no
  builder change — the shared `vk_create` (renderer.rs:5380-5427) has passed
  `push_constant_ranges` through all along.
- `ReflectedStageFlags::Compute => vk::ShaderStageFlags::COMPUTE` already
  exists (renderer.rs:5535).
- `PushConstantBytes` (renderer.rs:5679-5711) and `collect_push_constant_block`
  (build_tasks.rs:1076-1103) are stage-agnostic and are reused verbatim.

What the phase builds: the gate flip (§1), the compute push slot in the type
system (§2), codegen (§3), a fixture (§4), the dispatch-path write (§5), and
the two watercolor collapses (§6).

## 1. Reflection — flip the gate, then delete it

`reflect_compute_entry_point`
(crates/slang-reflection/src/reflection/parameters.rs:856-905) passes
`PushConstantSupport::Rejected` at :859-860. The flip is one word; the honest
change is a deletion. `PushConstantSupport` (:129-133) has exactly two call
sites — `reflect_entry_points` at :31 and this one — and after the flip both
pass `Supported`. So the enum, the `push_constant_support` parameter (:139),
and the bail (:165-171) all go. Phase 7 built the parameter for exactly this
flip; a constant-valued parameter afterward is speculative generality, and
Phase 13 cannot want it back — picking rejection has been type-level since 8b
(a picking pipeline accepts only `NoPush` handles, docs/bindless.md:81).

What the compute path then reaches, all shared with graphics and unchanged:

- the one-block-only check (:173-183),
- `reject_descriptor_fields` (:202) — a descriptor field in a compute push
  block is rejected by the same code as graphics; §4 adds the test that pins
  the routing,
- `PushConstantGlobalParameter` with its `element_size` (:204-208).

One rejection stays, deliberately: `reject_non_varying_entry_point_parameter`
(:222-264). A `uniform` entry point parameter is an implicit second source of
push ranges that bypasses the annotated-global route codegen reads. Reword the
comment at :868-872 — its "the dispatch path never writes one" tail dies with
this phase, but the bypass rationale is the same one graphics has (Phase 7b).

Tests (crates/cli/src/build_tasks.rs):

- **Delete** `a_compute_push_constant_block_is_rejected` (:3195-3228), its
  comment included.
- **Keep, untouched:** `a_compute_entry_point_uniform_is_rejected` (:3385) and
  `a_compute_entry_point_descriptor_is_rejected` (:3426). Both must stay green
  with no edit; needing one is evidence the entry-point walk broke.
- **Add** `a_descriptor_in_a_compute_push_block_is_rejected`: the compute twin
  of the graphics test at :3153, via the existing
  `reflect_rejected_compute_shader` helper (:2767). It pins that the compute
  path routes through the shared `reject_descriptor_fields` once the enum is
  gone.

## 2. The push slot for compute

Phase 8b's encoding extended to the compute types. Nothing is designed here;
the graphics shapes are copied one-for-one. `pipeline.rs` is
crates/renderer/src/renderer/pipeline.rs.

- `ComputePipelineConfig<'t, P = NoPush>` (pipeline.rs:488-493): add the
  parameter and a private `push: PhantomData<P>`.
- **A new `ComputePipelineConfigBuilder<'t>`** with today's four pub fields and
  `pub fn build<P>(self) -> ComputePipelineConfig<'t, P>`. Generated code
  constructs the builder literal and calls `.build()`, with `P` inferred from
  the generated `pipeline_config()`'s declared return type — exactly how
  graphics does it (shader_atlas_entry.rs.askama:171-177: builder literal,
  `.{{ build_method }}()`, no turbofish). The alternative — a pub
  `_push: PhantomData<P>` field in the user-visible literal — leaks plumbing
  into generated code and diverges the two templates for nothing. The :487
  comment ("fields are pub because generated compute atlas entries construct
  this directly") moves to the builder.
- `ComputePipelineStorage::add<P>` (pipeline.rs:453-462): returns
  `PipelineHandle<Compute, P>`. `P` is erased at the storage boundary, same as
  `PipelineStorage::add<T, P>` (:137-146). Generify the `#[cfg(debug_assertions)]`
  `get_mut` (:464-467) with it.
- `create_compute_pipeline<P>` (renderer.rs:1400-1403): takes
  `ComputePipelineConfig<P>`, returns `PipelineHandle<Compute, P>`.
- **The check_crate stub moves in lockstep**
  (crates/cli/fixtures/check_crate/src/renderer/mod.rs:46-51): the concrete
  `ComputePipelineConfig<'a>` becomes the parameterized form plus a
  `ComputePipelineConfigBuilder`, mirroring the graphics stubs at :39-44 and
  :53-61. phase_08.md §2.2 calls the stub the easiest thing in the phase to
  miss; it recurs here, again. Drift is an `alignment_tests` cargo-check
  failure, not a runtime failure.

The `P = NoPush` default keeps the blast radius at zero: particles' one
pipeline and all ten of watercolor's `PipelineHandle<Compute>` fields compile
untouched when this section lands (§6 then retypes two of them) — the same
default-parameter trick that let 8b land across ~14 graphics call sites
without an edit.

## 3. Codegen

crates/cli/src/build_tasks.rs plus one template.

- `collect_compute_shader_data` (:601-670): the `unreachable!()` arm
  (:614-617) becomes the mirror of the graphics arm (:363-371) — call
  `collect_push_constant_block` (:1076-1103) and record the returned name. The
  helper needs no edit: std430 fields, the size assert against slang's
  `element_size`, and the ≤128 gate are all stage-agnostic.
- `GeneratedComputeShaderImpl` (:541-549): add
  `push_constant_type_name: Option<String>` and a `config_return_type()` — the
  graphics one (:567-582) minus the vertex-axis branch:

  ```rust
  let push_slot = match &self.push_constant_type_name {
      Some(block) => format!("PushBlock<{block}>"),
      None => "NoPush".to_string(),
  };
  format!("ComputePipelineConfig<'a, {push_slot}>")
  ```

  **The slot is always printed, `NoPush` included.** That is where the graphics
  template landed after 8b (its `config_return_type` never leans on the
  default); compute lands there in one step rather than 8b's two. The cost is
  one-time churn in every compute `pipeline_config` — §4 counts it exactly.
- `ShaderComputeEntryModule` (:511-520): add `push_constant_budget: usize`,
  populated with `MAX_PUSH_CONSTANT_BYTES`
  (crates/slang-reflection/src/json/pipeline_builders.rs:53) in
  `render_compute_shader_file` (:673-707), mirroring :447.
- `templates/shader_compute_entry.rs.askama`, three edits:
  1. After the struct-defs loop (~line 93), the marker block copied from
     shader_atlas_entry.rs.askama:96-102:
     `impl {{ import_root }}::renderer::gpu_write::PushConstantBlock for {{ X }} {}`
     plus `const _: () = assert!(std::mem::size_of::<{{ X }}>() <= {{ push_constant_budget }});`.
  2. `pipeline_config`'s return type (line 113) becomes
     `{{ shader_impl.config_return_type() }}`.
  3. The config literal (lines 137-142) becomes
     `ComputePipelineConfigBuilder { … }.build()`.

For the record: **codegen emits no `pub type PushConstants` alias, for graphics
either.** The parent doc's Phase 12 section describes a Phase 7-era shape; 8b
shipped the marker impl instead, and the parent doc's "silently never written"
argument survives with the marker substituted (§9).

## 4. The fixture and the snapshot tripwire

New: `crates/cli/fixtures/alignment/push_constants.compute.slang`, declaring
`module push_constants_compute;` — the `handle_params` pair already establishes
the naming convention for a fixture stem shared across both suffixes.

```slang
#language slang 2026

module push_constants_compute;

// Test: a [[vk::push_constant]] block in a compute shader.
//
// Mirrors push_constants.shader.slang's pins for the compute reflection and
// codegen path: the nested struct + `tail` prove std430 over std140, and the
// handles pin that a handle living *only* in a push block still sets
// bindlessHeapSet — Params carries no handle, so the push block is the only
// possible source. `target` is a storage-image handle in a push block: the
// write target selected per dispatch, which is the shape this phase exists
// for. SampleLevel, not Sample: implicit-LOD sampling is illegal in compute.

struct DispatchInner {
    float2 v;   // 0
}               // std430: align 8, size 8 | std140: align 16, size 16

struct DispatchConstants {
    float scale;                        // 0
    Sampler2D.Handle tex;               // 8   (4-byte gap after the scalar)
    RWTexture2D<float4>.Handle target;  // 16
    DispatchInner inner;                // std430: 24 | std140: 32
    float tail;                         // std430: 32 | std140: 48
    float4x4 model;                     // std430: 48 | std140: 64
}                                       // std430: 112 | std140: 128

struct Params {
    float4 tint;    // 0
}                   // size 16

ParameterBlock<Params> params;
[[vk::push_constant]] ConstantBuffer<DispatchConstants> dispatchConstants;

// reads every member of the block: the compute push_constant_spirv_layout
// twin asserts against this module, and an unread member would be free for
// the optimizer to drop, shifting the member indices the test pins
[numthreads(8, 8, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let uv = dispatchConstants.inner.v * dispatchConstants.scale + dispatchConstants.tail;
    let color = mul(dispatchConstants.model, dispatchConstants.tex.SampleLevel(uv, 0.0));
    dispatchConstants.target[int2(dispatchThreadID.xy)] = color * params.tint;
}
```

`inner` sits after the two handles so the std430/std140 discriminator stays
observable — `inner` at 24 vs 32, `tail` at 32 vs 48 — and the total 112 needs
no tail-rounding judgment call.

**Add** the compute twin of `push_constant_spirv_layout` (:2386-2436): the same
`member_offsets` machinery, reading `reflected.compute_shader` instead of the
fragment module, asserting `[(0,0), (1,8), (2,16), (3,24), (4,32), (5,48)]` and
`inner`'s `[(0,0)]`. It is ~20 lines and pins the compute compilation path's
std430 lowering, which the graphics test cannot.

**The tripwire.** Exactly six snapshot files move; a diff anywhere else is a
bug:

- **New (2):**
  `…alignment_tests@src__generated__shader_atlas__push_constants_compute.rs.snap`
  and `…alignment_tests@shaders__compiled__push_constants.comp.json.snap`.
- **Updated (4):** `…@src__generated__shader_atlas.rs.snap` (the new module's
  atlas entry) and the three existing compute `.rs` snaps —
  `handle_params_compute`, `pointer_params_compute`,
  `storage_handle_params_compute` — whose `pipeline_config` gains `, NoPush`
  and the builder literal.
- **Byte-identical, and that is itself evidence:** every graphics `.rs` and
  `.json` snap, and the three existing `.comp.json` snaps. The reflection JSON
  format does not change — `push_constant_ranges` already exists on
  `ReflectedPipelineLayout` (pipeline_builders.rs:12).

`just insta` to review; do not blind-accept. `just shaders` also regenerates
the examples: watercolor's and particles' committed compute modules change
return types — a working-tree diff, not a snapshot.

## 5. The dispatch path

crates/renderer/src/renderer.rs, mirroring Phase 8's record-loop shape.

- **Retain the range.** `ComputeShaderPipelineLayout` (:5235-5245) gains
  `push_constant_range: Option<vk::PushConstantRange>` with a doc pointer to
  `ShaderPipelineLayout::push_constant_range` (:5160-5161), populated in both
  constructors (debug :5280-5285, release :5300-5305) via
  `single_push_constant_range` (:5505-5514), exactly as graphics does at
  :5206-5208 and :5230. The created `VkPipelineLayout` needs nothing — the
  struct simply never kept a copy of the range it already created.
- **Payload on the pending command.** `PendingComputeCommand::Dispatch`
  (:5656-5660) gains `push_constants: Option<PushConstantBytes>` with the same
  doc comment as `PendingDrawCommand::Draw`'s (:5673). `PushConstantBytes`
  (:5679-5711) is reused verbatim.
- **Generalize `cmd_push_constants`** (:2480-2504) from `&ShaderPipelineLayout`
  to raw parts:

  ```rust
  fn cmd_push_constants(
      &self,
      command_buffer: vk::CommandBuffer,
      pipeline_layout: vk::PipelineLayout,
      push_constant_range: Option<vk::PushConstantRange>,
      payload: Option<&PushConstantBytes>,
      shader_name: &str,
  )
  ```

  The two-direction `match`/`unreachable!` body is unchanged — its message is
  already bind-point-agnostic. `cmd_bind_texture_heap` (:2454-2476) is the
  precedent: the sibling helper takes raw parts precisely so both record loops
  can call it. The graphics call site (:2187) passes
  `layout.pipeline_layout, layout.push_constant_range` in place. No size check
  is added: `PushConstantBytes::from_value`'s const assert (:5687-5692) plus
  8b's "`P` *is* the pipeline's block" covers it, same as graphics.
- **The record loop.** In `record_compute_commands` (:1647-1731), call the
  helper between `cmd_bind_texture_heap` (:1697-1702) and `cmd_dispatch`
  (:1705-1710), passing the layout's pair and the command's
  `push_constants.as_ref()`. Once per dispatch iteration is necessary and
  sufficient, for the invalidation-rule reasons in phase_08.md §3.
- **The API.** `dispatch` (:5794-5806) refactors over a private
  `push_dispatch(&mut self, pipeline_index, group_count, push_constants)`
  carrying the existing consecutive-`Dispatch` auto-barrier (:5795-5801), and
  gains a sibling — `push_vertex_count_draw` (:5942-5953) is the structural
  model:

  ```rust
  pub fn dispatch(&mut self, pipeline: &PipelineHandle<Compute, NoPush>, x: u32, y: u32, z: u32)

  /// [`Self::dispatch`], with a per-dispatch push constant block
  pub fn dispatch_with_push_constants<P: PushConstantBlock>(
      &mut self,
      pipeline: &PipelineHandle<Compute, PushBlock<P>>,
      x: u32, y: u32, z: u32,
      push: &P,
  )
  ```

  `dispatch` spells `NoPush` explicitly, matching `queue_draw_indexed`
  (:5835-5837). The payload is captured at call time — queue-time bytes, Phase
  8's decision — and §6 leans on that.
- **Barriers do not change.** The auto-barrier keys on consecutive `Dispatch`
  commands, not pipeline identity, so two dispatches of one collapsed pipeline
  get the same barrier the two old pipelines did.
- **Hot reload needs no code.** `try_compute_shader_recompile` (:2923-2996)
  swaps in a whole fresh `ComputeShaderPipelineLayout`, carrying the new field;
  `assert_shader_interface_unchanged` (:5129-5145) compares the full serialized
  reflection, so a mid-run push-block edit trips the interface panic exactly as
  for graphics. §7 spot-checks both directions.
- **No new runtime asserts.** The parent doc asks for "the same two debug
  asserts, both directions"; that request predates 8b. The push slot makes the
  mixed states unrepresentable, and the `unreachable!` inside
  `cmd_push_constants` is the only residue — carried to compute unchanged
  (§9).

### 5.1 The interleaving hazard, measured

The parent doc records one hazard graphics alone does not expose: push
constant state is a single block per command buffer, not partitioned by bind
point, and it calls the draw/dispatch clobber "reachable here rather than
theoretical". Measured against the tree, it is structurally unreachable:

- Payloads travel inside the pending commands, and `cmd_push_constants` writes
  them immediately before their own `vkCmdDispatch` / `vkCmdDraw` (:1697-1710,
  :2180-2194). No command reads push state it did not push itself.
- Within a frame the compute pass fully precedes the render pass —
  `record_compute_commands` runs at :1788, before the picking and main passes
  begin — so "a draw between two dispatches" cannot even be recorded.
- Command buffers are re-recorded every frame; no push state survives a frame
  boundary.

What *is* real: two consecutive dispatches of one pipeline with different
payloads (jacobi's exact shape, every frame — §7's poison (a) is the direct
probe), and a command buffer carrying both compute and graphics pushes (no
example does this even after §6, since watercolor's display declares no block —
§7.3's scaffold forces it once, deliberately). The parent-doc sentence gets a
correction rather than a confirmation (§9).

## 6. Watercolor

The split rule: **the push block carries what varies between dispatches of one
frame; frame-constant data stays in `Params`.** The Out-of-scope section
records why whole-`Params`-to-push is not on the table (empty `Resources` is
unsupported), but the split is the principled layout regardless —
docs/bindless.md:66 calls the push block the per-draw channel, and `gridSize`
and jacobi's `divergence` are per-frame facts. Handles are 8-byte `uint2`
values (docs/bindless.md:33), so both blocks below sit far under the 128-byte
floor.

### 6.1 wc_pressure_jacobi, 2 → 1

Shader (wc_pressure_jacobi.compute.slang):

```slang
ParameterBlock<Params> params;

struct Params {
    Sampler2D<float>.Handle divergence;
    float2 gridSize;
}

[[vk::push_constant]] ConstantBuffer<JacobiDispatch> jacobiDispatch;

struct JacobiDispatch {
    Sampler2D<float>.Handle pressureIn;    // 0
    RWTexture2D<float>.Handle pressureOut; // 8
}                                          // 16 bytes
```

The two bound declarations become handles, and the five body references
(:27-34) change prefix from `params.` to `jacobiDispatch.` — nothing else.
Indexed access is identical through a handle (the Phase 11 measurement).

main.rs:

- The pipeline pair (:490-510) collapses to one `create_compute_pipeline` with
  `Resources { params_buffer: &pressure_jacobi_params_buffer }`.
- Field `pressure_jacobi_pipelines: [PipelineHandle<Compute>; 2]` (:147)
  becomes `pressure_jacobi_pipeline:
  PipelineHandle<Compute, PushBlock<wc_pressure_jacobi_compute::JacobiDispatch>>`.
- The loop (:740-748):

  ```rust
  for _ in 0..JACOBI_ITERATIONS {
      let parity = self.pressure_parity;
      renderer.dispatch_with_push_constants(
          &self.pressure_jacobi_pipeline,
          wx, wy, 1,
          &wc_pressure_jacobi_compute::JacobiDispatch {
              pressure_in: self.pressure.read_sampled(parity).bindless_handle(),
              pressure_out: self.pressure.write_storage(parity).bindless_handle(),
          },
      );
      self.pressure_parity = !self.pressure_parity;
  }
  ```

- The `Params` write (:964-970) already writes exactly
  `{ divergence, grid_size }` — the bound slots were descriptors, not uniform
  bytes — so it does not change at all.
- The `JACOBI_ITERATIONS` even-parity assert (:50-53) stays; display-side
  expectations are unchanged.

**No parity-ordering footgun — dissolved, not dodged.** Phase 11's footgun
(phase_11.md §4) is a frame-end uniform write racing the parity flip. A push
payload is captured at the dispatch call, before the flip on the next line, so
the value each dispatch reads is the value in hand when it was queued. There is
no ordering to get wrong.

### 6.2 wc_gaussian_blur, 2 → 1 pipelines and 2 → 1 UBOs

Shader (wc_gaussian_blur.compute.slang): `Params` keeps only `gridSize`; the
per-dispatch values move.

```slang
struct Params {
    float2 gridSize;
}

[[vk::push_constant]] ConstantBuffer<BlurDispatch> blurDispatch;

struct BlurDispatch {
    Sampler2D<float>.Handle inputTex;    // 0
    RWTexture2D<float>.Handle outputTex; // 8
    float2 direction;                    // 16
}                                        // 24 bytes
```

Body references at :37, :45, :49 change prefix; nothing else.

main.rs:

- `blur_h_params_buffer` / `blur_v_params_buffer` (:456-459) become one
  `blur_params_buffer`; the pipeline pair (:520-532) becomes one.
- The dispatches (:760-770):

  ```rust
  // 6. Gaussian blur H (wet_mask → blur_temp)
  renderer.dispatch_with_push_constants(
      &self.blur_pipeline, wx, wy, 1,
      &wc_gaussian_blur_compute::BlurDispatch {
          input_tex: self.wet_mask.read_sampled(self.sim_parity).bindless_handle(),
          output_tex: self.blur_temp.bindless_handle(),
          direction: Vec2::new(1.0, 0.0),
      },
  );

  // 7. Gaussian blur V (blur_temp → blurred_mask)
  renderer.dispatch_with_push_constants(
      &self.blur_pipeline, wx, wy, 1,
      &wc_gaussian_blur_compute::BlurDispatch {
          input_tex: self.blur_temp_sampled.bindless_handle(),
          output_tex: self.blurred_mask.bindless_handle(),
          direction: Vec2::new(0.0, 1.0),
      },
  );
  ```

  `sim_parity` at the dispatch site is pre-flip (:791 flips it later in the
  same `draw`), which is the side the old H write (:993-1004) read — the same
  value, captured earlier.
- The two closure writes (:993-1014) collapse to one `Params { grid_size }`
  write.

### 6.3 Leftovers

- Delete the stale `// [sim_parity * 2 + deposit_parity]` comment on the
  (single) `advect_and_transfer_pipeline` field (~:152) — a Phase 11b
  leftover.
- End state: **10 pipelines** (9 compute + 1 graphics) and **zero bound
  texture descriptors in the workspace**. Every `texture_handles` /
  `storage_texture_handles` vec codegen emits is now empty, and the image-write
  arms in `create_descriptor_sets` are dead — retirement is the follow-up (Out
  of scope), not this phase.

## 7. Verification

Evidentiary weight, strongest first, per phase_11.md §7 — a green sweep proves
nearly nothing here:

1. **Per-collapse A/B against a three-checkpoint baseline, target
   `compare -metric AE` = 0.** Rebuild the phase_11 §3 scaffolding: scripted
   stroke as a pure function of frame index, frozen FPS label, fixed frame
   count, `SDL_VIDEODRIVER=x11` capture with the double-grab and the
   stray-quit retry (`dt` is already a `const`). 11c declined to rebuild it,
   and that reasoning does not transfer: 11c moved where constant handles are
   *stored*; this phase changes *which values each dispatch computes with* —
   parity moves from baked-at-creation descriptor sets to bytes assembled at
   queue time every frame, and blur's H/V from two UBOs to two payloads. A
   wrong parity or a swapped direction is a plausible-looking wrong image with
   no validation output, the exact failure class the scaffolding exists to
   catch. Baseline from unconverted `40828ff` with the scaffolding applied;
   A/B after the jacobi collapse and again after the blur collapse:
   **0 / 0 / 0 both times**.
2. **Poison controls, one per failure this migration could actually cause** —
   each must move thousands of pixels at the checkpoints, then return to
   0 / 0 / 0 when reverted:
   - (a) jacobi: do not flip `pressure_parity` between the two loop
     iterations — both dispatches push the same parity and the ping-pong
     collapses into self-feedback. This is also the direct probe that the
     second of two consecutive dispatches sees *its own* bytes rather than its
     predecessor's.
   - (b) jacobi: swap `pressure_in` / `pressure_out` in the second iteration's
     payload.
   - (c) blur: push `direction: (1.0, 0.0)` for both dispatches.
3. **The interleaving probe the parent doc asks for.** §5.1 records why the
   draw/dispatch clobber is structurally unreachable; verify the coexistence
   claim anyway, once, with a scaffold: give the display shader a temporary
   `[[vk::push_constant]]` block (`float exposure`, multiplied into the final
   fragment color) and swap `draw_vertex_count` for
   `queue_draw_vertex_count_with_push_constants(…, &DisplayPush { exposure: 1.0 })`
   plus the submit. Every frame then records 4 compute pushes and 1 graphics
   push across two bind points and incompatible layouts. Pass: validation-clean
   under `just sweep`, and the A/B stays 0 (×1.0 is bit-exact). Revert.
4. **Suite and the snapshot tripwire.** `just test`: the §4 six-file delta and
   nothing else; the new compute SPIR-V offset test green; the new compute
   rejection test green; the deleted rejection gone; the two surviving
   entry-point rejections green without edits; `alignment_tests`' check_crate
   `cargo check` green (the stub lockstep); `push_constant_bytes_round_trip`
   (renderer.rs:6155) untouched. Plus `just lint`,
   `cargo check --workspace --all-targets`, `cargo fmt`.
5. **`just sweep` (16 ok / 0 skip / 0 fail) and hot reload.** The
   `PushConstantsNotSet` layer heuristic the parent doc weighs now covers the
   compute path for free — still a heuristic, not a backstop; the type system
   and the tripwire are the reliable half. Hot reload (lavapipe +
   `SDL_VIDEODRIVER=offscreen`): a body edit to `wc_gaussian_blur.compute.slang`
   hot-swaps with the payload path live; an edit to the push block itself trips
   `assert_shader_interface_unchanged`, the same contract as graphics.

**Teardown:** revert the scaffolding and the poison edits, re-run
`just shaders watercolor`, confirm the committed artifacts are byte-identical
to the converted state, final `just sweep`.

## 8. Docs

docs/bindless.md:

- :66 — "the per-draw channel" becomes the per-draw / per-dispatch channel;
  the slang example needs no change (the syntax is identical in compute).
- :71-73 — the queue-method list gains `dispatch_with_push_constants`, and the
  `PipelineHandle<D, PushBlock<P>>` sentence generalizes to compute handles.
- :80-81 — "Graphics only. Reflection rejects a push block in a compute
  shader, and a picking pipeline accepts only `NoPush` handles." loses its
  first clause; the picking sentence stays (Phase 13's boundary).
- :57-60 — the watercolor reference paragraph ("a per-frame write target in
  the params uniform lets one compute pipeline write both textures of a
  ping-pong pair") becomes per-dispatch, citing jacobi as the reference for
  per-dispatch handles.

## 9. Corrections to `../bindless_textures.md`

In the house strikethrough style, as part of landing this plan:

1. :1867 and :1887 — ~~"codegen would emit `pub type PushConstants = X;`"~~ /
   ~~"the same `pub type PushConstants` + `<= 128` assert block"~~: no such
   alias is emitted anywhere, for graphics either. Phase 8b shipped the marker
   impl instead — `impl PushConstantBlock for X {}` plus the size assert
   (shader_atlas_entry.rs.askama:96-102) — and that is what the compute
   template copies. The "silently never written" argument survives with the
   marker substituted: the dead API is the pub struct plus its marker impl.
2. :1879 — ~~`PushConstants::{Supported, Rejected}`~~: the enum is
   `PushConstantSupport` (parameters.rs:129-133), and this phase deletes it
   rather than flipping it.
3. Stale anchors in the work list: renderer.rs:5082 → :5235, :5465 → :5656,
   :1618-1623 → :1697-1702, :1626 → :1705, :5529 → :5794, :1709 → :1788.
4. :1898-1900 — ~~"The same two debug asserts, both directions."~~: superseded
   by 8b before this phase was scheduled. The push slot makes the mixed states
   unrepresentable; the `unreachable!` in `cmd_push_constants` is the only
   residue.
5. :1908 — ~~"this is reachable here rather than theoretical"~~: measured
   structurally unreachable (§5.1) — payload-per-command recording, the
   compute pass preceding the render pass, per-frame re-record. The deliberate
   verification the paragraph asks for still runs (§7.3).

When the collapses land, phase_11b.md:386 and phase_11c.md:332 gain their
"done, see phase_12.md" notes.

## 10. Outcome

End state: **10 pipelines** (9 compute + 1 graphics) and **zero bound texture
descriptors in the workspace**. Every mechanism §"Why this phase exists" claims
is free was free.

### 10.1 Five places this plan was measured wrong

1. **§6.2's parity claim is wrong, and the A/B is what caught it.** The plan
   says `sim_parity` at the blur H dispatch site "is pre-flip, which is the side
   the old H write read — the same value, captured earlier". It is not. The old
   `blur_h` uniform write ran inside the `draw_vertex_count` closure, which
   executes *after* the `sim_parity` flip, so it read the **post-flip** value.
   The dispatch site is pre-flip, so the equivalent is `!self.sim_parity`.
   Written as planned, the A/B was 348 / 1627 / 2736 differing pixels; with the
   negation it is 0 / 0 / 0. Nothing else would have reported this — no
   validation message, no assert, and the image is plausible either way.
2. **§4's tripwire is seven files, not six.** It misses
   `generated_files@src__generated__shader_atlas__particles_compute.rs.snap`.
   The `generated_files` test snapshots particles' compute module, whose
   `pipeline_config` gains `, NoPush` and the builder literal like the three
   `alignment_tests` compute snaps. Measured delta: 2 new, 5 updated, every
   other snapshot byte-identical.
3. **§1 misses one prose edit.** `reject_non_varying_entry_point_parameter`'s
   bail message ended "(graphics shaders only)" (parameters.rs:250). That tail
   dies with this phase, same as the `reflect_compute_entry_point` comment.
4. **The reflected stage flag is `All`, not `Compute`.**
   `PipelineLayoutBuilder::current_stage_flags` starts at
   `ReflectedStageFlags::All` and narrows only inside the entry-point walk, so a
   compute push range serializes as `"stageFlags": "all"` and `to_vk` maps it to
   `vk::ShaderStageFlags::ALL`. The `Compute => COMPUTE` arm the opening section
   cites (renderer.rs:5535) is not on this path. `vkCmdPushConstants` matches
   anyway, because the payload is pushed with the range read back off the same
   layout.
5. **§5 conflates two call sites.** renderer.rs:2187 is the `cmd_push_constants`
   call, not a `cmd_bind_texture_heap` call. `cmd_bind_texture_heap` has three
   call sites: compute, picking, and the main draw loop. Picking records no push
   constants, and the generalized helper is not wired into it.

Line numbers throughout drift by up to eight lines from the `40828ff` snapshot.
Locate by symbol.

### 10.2 What the evidence showed

**The capture scaffolding.** Rebuilt per phase_11 §3 with all three of §9.4's
corrections: scripted stroke in canvas space as a pure function of frame index,
frozen FPS label, a wall-clock hold with the `CHECKPOINT <n>` marker printed
only after 15 held frames, and a capture script that kills strays, records
geometry, and requires two consecutive grabs to match. Checkpoints at frames 30
(mid-stroke), 60 (post-stroke) and 120 (late-sim).

Self-test: two runs of the unconverted build were **0 differing pixels** at all
three checkpoints, and the three checkpoints differ from each other by 28,105
and 28,593 pixels — distinct sim states, not a converged image compared with
itself.

**A/B.** 0 / 0 / 0 after the jacobi collapse, and 0 / 0 / 0 after the blur
collapse once §10.1's parity error was fixed.

**Poison controls**, each reverted to 0 / 0 / 0 afterwards:

| poison | c1 | c2 | c3 |
|---|---|---|---|
| (a) no `pressure_parity` flip between iterations | 7,031 | 12,389 | 16,990 |
| (b) swapped `pressure_in`/`pressure_out` on iteration 2 | 7,031 | 12,389 | 16,990 |
| (c) `direction: (1,0)` for both blur dispatches | 59 | 757 | 2,626 |

(a) is the direct probe §7.2 asks for: the second of two consecutive dispatches
reads *its own* bytes, not its predecessor's. If it read its predecessor's,
both dispatches would already carry the same parity and removing the flip would
change nothing. (a) and (b) coincide because both make the second iteration
read and write the side the first one just wrote.

(c) fires at every checkpoint but moves tens rather than thousands of pixels
early on, contrary to §7.2's "thousands of pixels at the checkpoints". The
blurred mask feeds `flow_outward`'s diffusion, so the error accumulates instead
of appearing at once.

**The interleaving probe (§7.3).** Ran as specified: a temporary `float
exposure` push block on `paint_display`, multiplied into the final fragment
color, queued through `queue_draw_vertex_count_with_push_constants`. Every frame
then recorded 4 compute pushes and 1 graphics push across two bind points and
incompatible layouts. Validation-clean under `just sweep`, and the A/B stayed
0 / 0 / 0 (×1.0 is bit-exact). Reverted.

**Hot reload** (lavapipe + `SDL_VIDEODRIVER=offscreen`), both directions. A body
edit to `wc_gaussian_blur.compute.slang` hot-swaps with the payload path live
(`finished recompiling compute shader: wc_gaussian_blur.compute.slang`, no
validation output). Adding a field to the push block panics with
`shader interface changed: 'wc_gaussian_blur.compute.slang'`, the same contract
graphics has. Note lavapipe runs watercolor at roughly two frames per second, so
a recompile takes tens of seconds to reach every compute shader.

**Suite.** `just test` green with exactly the §10.1 seven-file delta;
`push_constant_compute_spirv_layout` green, pinning the compute path's std430
emission at `[(0,0), (1,8), (2,16), (3,24), (4,32), (5,48)]`;
`a_descriptor_in_a_compute_push_block_is_rejected` green;
`a_compute_push_constant_block_is_rejected` deleted;
`a_compute_entry_point_uniform_is_rejected` and
`a_compute_entry_point_descriptor_is_rejected` green **with no edit**.
`just sweep` 16 ok / 0 skip / 0 fail with the injected-fault self-test still
firing.

**The reflection JSON format did not change.** `just shaders` regenerated every
example with no `.json` or `.spv` diff outside the two converted shaders; only
the ten compute `.rs` modules changed, and only in `pipeline_config`.

## Out of scope

- **Retiring the per-pipeline descriptor path.** `texture_handles` /
  `storage_texture_handles` on `PipelineConfig` / `ComputePipelineConfig`, the
  image-write arms in `create_descriptor_sets`, and the codegen that fills the
  vecs — dead workspace-wide once §6 lands, and promised as a follow-up by
  phase_11b.md:386 and phase_11c.md:332. The same phase-splitting convention
  that kept retirement out of 11c keeps it out of here.
- **Empty `Resources` / zero-descriptor-set pipelines.** Whole-`Params`-to-push
  would leave a shader with no `ParameterBlock`, and three places break today:
  codegen emits `pub struct Resources<'a>` unconditionally (build_tasks.rs:1136
  — zero fields is an E0392 unused-lifetime error), `record_compute_commands`
  chunks by `descriptor_sets_per_frame` (renderer.rs:1674-1678 — `chunks(0)`
  panics), and `create_descriptor_sets` allocates unconditionally
  (renderer.rs:4483-4486 — `descriptorSetCount = 0` violates a VUID). §6's
  split layout needs none of it; support belongs to the retirement follow-up
  if that phase wants it.
- **Phase 13** — push constants in the picking path. Unchanged by this phase.
