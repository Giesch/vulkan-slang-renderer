# Phase 0 spike results — `DescriptorHandle` reflection and SPIR-V lowering

**Status: done, 2026-08-05.** These are the measured answers to the Phase 0
questions in [../bindless_textures.md](../bindless_textures.md). Everything below
was observed, not reasoned about; the few places where I reason past the
measurement are marked **(not measured)**.

Toolchain: the slang-rs fork at `v0.1.1+slang-2026.13.1` (vendored slang
2026.13.1), pinned in the root `Cargo.toml`. Disassembly with `spirv-dis` from
the Vulkan SDK at `~/Tools/vulkan/1.4.328.0`.

> The system `slangc` on this machine is **2025.17.2**, older than the vendored
> library. Don't spike through it — go through the fork.

## How this was run

A throwaway `#[cfg(test)] mod bindless_spike` appended to
`crates/renderer/src/shaders.rs` (deleted afterward; nothing was committed). It
built its own session mirroring `prepare_reflected_shader_with_optimization`
(`shaders.rs:62-92`), walked the reflection tree, and wrote each entry point's
SPIR-V to `/tmp/bindless_spike/`.

The one thing that differs from the production setup:

```rust
// BindlessSpaceIndex is a TARGET option, not a session option.
let target_options = slang::CompilerOptions::default().bindless_space_index(4);
let target_desc = slang::TargetDesc::default()
    .format(slang::CompileTarget::Spirv)
    .profile(global_session.find_profile("glsl_450+spirv_1_6"))
    .options(&target_options);          // <-- this line is what's missing today
```

The parent doc's Phase 0 step 1 says to add the option "alongside
`emit_spirv_directly`", which is the *session* options struct at `shaders.rs:70`.
That's the wrong struct — see the fork's own test at `src/tests.rs:138-144`.

Eight variants were compiled: handle directly in a param block; handle inside a
`Std430DataLayout` pointee; the same with `NonUniformResourceIndex` on the index
and on the handle; a handle-free control; separate `Texture2D.Handle` +
`SamplerState.Handle`; nested `ParameterBlock`s; and a deliberate space
collision.

---

## Q1 — What does `slang::reflection` report for a handle field?

**A handle reflects as `TypeKind::Vector` — a `uint2`. Its identity survives only
on the declared type.**

```
tex: kind=Vector tl.name=Some("vector") declared_full_name="DescriptorHandle<Sampler2D<vector<float,4>>>"
     categories=[Uniform@0(size 8)] uniform(size=8, stride=8, align=8)
```

- `type_layout().kind()` → `TypeKind::Vector`; `type_layout().name()` → `"vector"`.
- `variable_layout.ty().full_name()` → `DescriptorHandle<Sampler2D<vector<float,4>>>`.
  For the separate forms: `DescriptorHandle<Texture2D<vector<float,4>>>` and
  `DescriptorHandle<SamplerState>`.
- Parameter category is **`Uniform`**, size 8, stride 8, align 8. It is ordinary
  data, exactly as the design assumed.
- There is no dedicated `TypeKind`: `SlangTypeKind` has 21 variants ending at
  `Enum = 20` (`slang-sys/src/bindings.rs:1594-1616`). `Vector` is what we get.

**Nested inside a `Std430DataLayout` pointee: identical.** The `Material` case the
design actually wants lays out correctly.

```
materials: kind=Pointer declared_full_name="Ptr<Material, Access.Immutable, AddressSpace.Device, Std430DataLayout>"
  -> std430 pointee Some("Material") size=32
    albedo: kind=Vector  declared_full_name="DescriptorHandle<Sampler2D<vector<float,4>>>"  Uniform@0 (size 8)
    tint:   kind=Vector  declared_full_name="vector<float,4>"                               Uniform@16 (size 16)
```

`albedo` at offset 0 size 8, `tint` at 16 (std430 pushes the `float4` to a 16-byte
boundary), struct size 32. The existing pointee machinery
(`program_layout.type_layout(ty, LayoutRules::DefaultStructuredBuffer)`) handles it
with no changes.

### The important part: today this fails silently

Running the *real* `prepare_reflected_shader` on both variants **succeeds**. It
does not panic, and it does not reject anything. `reflect_struct_fields` falls into
its `TypeKind::Vector` arm and emits:

```json
{
  "kind": "vector",
  "fieldName": "albedo",
  "binding": { "kind": "uniform", "offset": 0, "size": 8 },
  "elementCount": 2,
  "elementType": { "kind": "scalar", "scalarType": "uint32" }
}
```

So `just shaders` would today generate a `UVec2` field, the `offset_of!`/`size_of`
assertions would all pass, and the app would write raw integers into what the
shader treats as a descriptor index — with no diagnostic anywhere. The pipeline
layout comes out with a single `constantBuffer` binding and no heap set.

This is the same failure shape as the enum-degradation case already handled at
`crates/renderer/src/shaders/reflection/parameters.rs:180-190` ("slang lays an enum
out as its tag type … the enum identity survives only on the *declared* type"), and
it wants the same fix: **check the declared type's `full_name()` for a
`DescriptorHandle<` prefix before matching on `field_type_layout.kind()`**, in the
same early-continue block as the enum check. It is not a new arm in the
`TypeKind::Resource` match, which is where the parent doc expected it to land.

## Q2 — What global arrays does Slang emit?

**One unbounded `UniformConstant` array per descriptor type used, all in the
bindless space. The documented binding map is confirmed.**

From the combined-sampler case (`Sampler2D.Handle`):

```
OpCapability RuntimeDescriptorArray
OpDecorate %params                Binding 0
OpDecorate %params                DescriptorSet 0
OpDecorate %__slang_resource_heap Binding 1
OpDecorate %__slang_resource_heap DescriptorSet 4
%18 = OpTypeImage %float 2D 2 0 0 1 Unknown
%19 = OpTypeSampledImage %18
%_runtimearr_19 = OpTypeRuntimeArray %19
%__slang_resource_heap = OpVariable %_ptr_UniformConstant__runtimearr_19 UniformConstant
```

From the separate `Texture2D.Handle` + `SamplerState.Handle` case, two arrays:

```
OpDecorate %__slang_resource_heap   Binding 2     // %19 = OpTypeImage   (SAMPLED_IMAGE)
OpDecorate %__slang_resource_heap_0 Binding 0     // %24 = OpTypeSampler (SAMPLER)
```

Confirmed map: **0 = sampler, 1 = combined image sampler, 2 = sampled image.**
(Binding 3 "unknown" was not exercised.)

- The arrays are **`OpTypeRuntimeArray`** — unbounded — which is why
  `OpCapability RuntimeDescriptorArray` appears. Phase 1's
  `runtime_descriptor_array(true)` is **required**, not conditional.
- **A shader with no handles emits nothing.** The control variant produced no
  `__slang_resource_heap` variable and no `RuntimeDescriptorArray` capability, even
  though it was compiled with `bindless_space_index(4)`.

### How the handle is consumed

```
%29 = OpAccessChain %_ptr_Uniform_v2uint %params %int_0
%30 = OpLoad %v2uint %29
%31 = OpCompositeExtract %uint %30 0                          // <-- component 0 only
%32 = OpAccessChain %_ptr_UniformConstant_19 %__slang_resource_heap %31
%34 = OpLoad %19 %32
%sampled = OpImageSampleImplicitLod %v4float %34 %33 None
```

Only **component 0** of the `uint2` is read. In the default lowering the upper 32
bits are unused. The Rust `BindlessHandle<T>` should still be 8 bytes to match the
layout, but only the low word carries the slot index.

## Q3 — Does it emit `NonUniformEXT`?

**No. Never — and there is no source-level way to ask for it.**

`NonUniformEXT` appears zero times across all 16 emitted SPIR-V modules. Three
separate attempts:

1. Handle loaded from a material buffer indexed by a fragment varying (genuinely
   non-uniform) — no decoration.
2. `NonUniformResourceIndex(uint(i.uv.x))` on the buffer index — no decoration.
   (Correct in itself: that index feeds an `OpPtrAccessChain` on a BDA pointer, not
   a descriptor array.)
3. `Sampler2D tex = NonUniformResourceIndex(m.albedo);` — **compiles cleanly and
   changes nothing.** No decoration, byte-identical structure.

The reason is visible in the disassembly above: the index that reaches the heap's
`OpAccessChain` is synthesized by Slang from the loaded handle (`%45`/`%31`), so
there is no expression in the shader source that corresponds to it.

**This is the one finding that needs a decision before Phase 5**, because the
Vulkan spec requires the `NonUniformEXT` decoration when a descriptor index is not
dynamically uniform within the invocation group; without it the result is undefined
on hardware that cares. Options, in the order I'd try them:

- Keep indexing dynamically uniform per draw — one material per draw call. Safe
  today, but it gives up exactly the multi-draw batching that motivated the work
  ([../render-graph/05_multi_draw_rendering.md](../render-graph/05_multi_draw_rendering.md)).
- Check whether a newer slang emits the decoration, or file it upstream. Not
  investigated here.
- Override `getDescriptorFromHandle` — currently a stated non-goal, but it is the
  documented escape hatch and it is exactly the seam where the decoration would go.
  The parent doc's claim that deferring it "costs nothing" holds: it needs no
  shader-source change.

## Q4 — Does the reported bindless space match what we asked for?

**It reports the space actually used, which is `max(requested, first free space)`.
And it does not tell you whether the shader uses the heap at all.**

| variant | param block spaces used | requested | `bindless_space_index()` | SPIR-V `DescriptorSet` |
|---|---|---|---|---|
| single block + handle | 0 | 4 | 4 | 4 |
| nested blocks + handle | 0, 1 | 4 | 4 | 4 |
| nested blocks + handle | 0, 1 | **1** | **2** | **2** |
| single block + handle | 0 | *(unset)* | **1** | 1 |
| control, no handles | 0 | 4 | **4** | *(no heap emitted)* |

Two consequences:

- **Slang resolves collisions for us.** Asking for space 1 when a nested
  `ParameterBlock` already owns space 1 silently moved the heap to 2, and reflection
  reported 2. So `bindless_space_index` behaves as a *floor*, and
  `reflection.bindless_space_index()` is authoritative. There is no need for the
  "runtime assert against reflection" the parent doc's Phase 0 step 4 imagined — but
  there is a need to actually *read* the reported value rather than hardcoding one.
- **The reported index is not a usage signal.** The handle-free control still
  reported 4. `IBindlessResourceMetadata::usesBindlessResourceHeap()` would be the
  real signal, but it is bound only in raw `slang-sys` (`bindings.rs:1041`) and is
  **not exposed by the fork's safe API**. The practical substitute costs nothing:
  the reflection walk in Phase 4 already has to find handle fields, so "did we see
  any field whose declared `full_name()` starts with `DescriptorHandle<`" is the
  usage flag, and it can be recorded in the reflection JSON.

## The space index decision

Pass a **floor of 4** as the target option, and record
`reflection.bindless_space_index()` in the reflection JSON as the authoritative
value.

The floor matters for a reason the parent doc didn't anticipate: with no option
set, the heap lands in the first free space, which varies per shader (1 for a
single param block, 2 for nested). A varying set index means the heap set layout
and the `vkCmdBindDescriptorSets` call would differ per pipeline. A floor of 4 —
above any current example's param block count — makes it a constant in practice
while staying correct if it ever isn't. Assert the reflected value equals the floor
at pipeline creation and fail loudly if a shader ever grows past it.

---

## What this changes in the parent plan

> **Superseded — folded into [../bindless_textures.md](../bindless_textures.md).**
> Kept for the record. The measurements above stand; this section's *recommendations*
> were revised on review, and the phase numbers below are the old ones (the parent
> plan renumbered when the rejection work moved to the front). Two reversals worth
> knowing about:
>
> - **The `BINDLESS_SPACE_INDEX` floor of 4 was dropped.** Its only justification was
>   keeping the heap's set index constant across shaders, so the bind call wouldn't
>   differ per pipeline. But pipeline-layout compatibility forces a per-pipeline
>   rebind anyway — set 0 is the per-shader param block, so every pipeline switch
>   disturbs a binding at a higher set. Since a `VkDescriptorSetLayout` doesn't
>   encode its own set number, one heap layout works at whatever index each shader
>   reports. No floor means no gap indices and no empty placeholder layouts.
> - **`NonUniformEXT` is not "the one blocking decision".** It's scoped to material
>   indices that vary *within* a single draw. `toon_link` keeps one draw per batch,
>   so the Phase 6 payoff is unblocked, and even `gl_DrawID`-indexed multi-draw stays
>   dynamically uniform.

- **Phase 0 step 1 is wrong about where the option goes.** `TargetDesc`, not the
  session `CompilerOptions`.
- **Phase 1: `runtime_descriptor_array(true)` is required**, not conditional — the
  emitted arrays are `OpTypeRuntimeArray`. `shader_sampled_image_array_non_uniform_indexing`
  is *not* required by anything the compiler emits today, since `NonUniformEXT`
  never appears; keep it if we intend to fix Q3 later, but don't believe it's what
  makes the shader valid. The `descriptor_binding_*_update_after_bind` bits are
  driven by Phase 2's pool choice, not by the SPIR-V **(not measured)**.
- **Phase 2: create bindings 0, 1 and 2 in the heap set layout**, even though only
  binding 1 (combined image sampler) is exercised by `Sampler2D.Handle`. The binding
  numbers are fixed by Slang, so a shader that ever uses a separate texture or
  sampler handle needs 0/2 to already exist. `PARTIALLY_BOUND` makes the unused ones
  free. The shader-side unbounded array is compatible with a fixed-count layout as
  long as nothing indexes past the count **(not measured)**.
- **Phase 3 gets much smaller.** The concern that `reflect_pipeline_layout` would
  allocate the bindless space per-pipeline does not arise: `descriptor_set_count()`
  on the global params type layout is **0**, the single reflected binding range is
  the `ParameterBlock`, and the heap never appears in `ReflectedPipelineLayout` at
  all. There is nothing to *skip*. The work is purely additive — append the heap set
  layout at the reported index in `vk_create`, with empty placeholder layouts for
  gap indices, and keep the per-frame chunk width tied to the reflected sets. The
  `descriptor_sets_for_frame` / `descriptor_pool_sizes` hazards the parent doc lists
  only materialize if we push the heap layout into `descriptor_set_layouts`, which
  we now don't have to.
- **Phase 4 hooks in somewhere else than expected.** Not a new arm in the
  `TypeKind::Resource` match — a declared-type check before the `kind()` match,
  next to the existing enum special case at `parameters.rs:180-190`. Size and
  alignment are 8/8, so the existing `Addr` entries in the codegen size/alignment
  tables are the right model. The field must reflect as its own `StructField`
  variant precisely because the fallback is silent rather than loud.
- **Phase 5 is unaffected** by anything measured here.

## Open questions

- **`NonUniformEXT` (Q3).** The one blocking decision. Everything else is
  mechanical.
- Nothing here was run against a real device. Update-after-bind, partially-bound
  slots, and the retire-after-N-frames slot reuse are all still unverified — that's
  Phase 2's `just sweep`.
- **Compute shaders were not spiked.** Only vertex/fragment. The compute path has
  its own `prepare_reflected_compute_shader_with_optimization` (`shaders.rs:158`)
  with a separately-built `TargetDesc`, so it needs the same option added.
- Arrays of handles, and handles in a struct nested more than one level inside a
  pointee, were not tested.
- Whether `spirv-opt`/`OptimizationLevel::High` ever hoists or coalesces the heap
  access chain in a way that matters — the spike ran at `High` throughout and the
  output looked stable, but nothing stresses it.
