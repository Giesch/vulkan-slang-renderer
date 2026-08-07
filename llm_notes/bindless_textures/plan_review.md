# Review of the bindless-textures plan

**Status: review, 2026-08-06 — addressed.** A code-verified review of
[../bindless_textures.md](../bindless_textures.md) and
[phase_0_spike.md](phase_0_spike.md), written before Phase 1 started. Every
claim below was checked against the code at the commit current on this date
(`6f48010`); line references are to that state, corrected where the plan's
had drifted.

**Later the same day, the plan was updated to incorporate every item below**
(items 1-11 and the doc nits); items 2-4 additionally carry inline resolution
notes recording decisions that went beyond the review's suggestion. This
document stays as the record of what the review found.

Overall the plan is sound — the spike measured the right things, the phase
ordering (reject → features → heap → binding → codegen) keeps every
intermediate state safe, and the big structural claims verified: the single
recursive `reflect_struct_fields` walker means the Phase 1 detection really
fires inside `ImmutableAddr` pointees and nested structs; "the heap never
enters `ReflectedPipelineLayout`" is consistent with how sets are actually
discovered (sub-object ranges, not `descriptor_set_count()`); and texture
image layouts are set once at creation and never mutated, so heap descriptors
can't go stale from layout churn. The items below are what implementation
needs to add, fix, or watch.

## Bugs / omissions

### 1. Phase 2 is missing `shader_sampled_image_array_dynamic_indexing`

The spike's own disassembly shows the heap index is loaded from a buffer
(`OpLoad` → `OpAccessChain` into the runtime array) — non-constant indexing
of a sampled-image array, which the spec only permits with this feature
enabled. The plan correctly omits the *non-uniform* variant, but the
dynamic-indexing one is required. It is a core-1.0 `VkPhysicalDeviceFeatures`
bit, so it goes on the base `features` builder (renderer.rs:3491), **not** on
`vulkan_12_features` — and it needs mirroring in the suitability gate and the
bail message like the others. Universally supported in practice, but
validation will flag its absence.

### 2. Phase 3's slot-allocation sites miss `storage_texture_as_sampled`

The plan says to allocate slots in `create_texture_with_options` and
`create_texture_with_mips` "so *every* texture gets one" — but
`storage_texture_as_sampled` (renderer.rs:794) constructs a `Texture` and
calls `self.textures.add()` directly, bypassing both. Watercolor uses it
heavily. (Also, `with_options` just delegates to `with_mips`, so listing both
is redundant.) The robust hook is wherever `textures.add` is called — two
sites — or inside `add` itself. Related: these aliases live in `GENERAL`
layout, not `SHADER_READ_ONLY_OPTIMAL`, so `insert_texture` must take the
layout from `texture.image_layout` (as `create_descriptor_sets` already does
at renderer.rs:4219) rather than hardcoding it.

**Resolution (2026-08-06):** the plan now allocates slots in a single private
renderer method (e.g. `register_texture`) wrapping both `textures.add` call
sites — not inside `TextureStorage::add` (it has no device access), and not by
changing watercolor (`storage_texture_as_sampled` is a public API, and
watercolor's use of it is the only sweep coverage of `GENERAL`-layout aliased
textures in the heap). `insert_texture` reads `texture.image_layout`.

### 3. "Release in the destroy path" targets a path that is dead and already unsafe

`drop_texture` (renderer.rs:729) has **zero callers** in the workspace, and
`destroy_texture` destroys the sampler/view/image **immediately** — no fence
wait, no `old_pipelines`-style deferral. The plan carefully defers heap *slot
reuse* but never says the underlying Vulkan object destruction must be
deferred too, and today it isn't. Once every texture is resident in an
always-bound heap, the first real caller of `drop_texture` gets a
use-after-free even with perfect slot bookkeeping. Phase 3 should either make
texture destruction itself deferred (the retirement pattern the plan already
cites), or explicitly state that slot release is being built against a
currently-uncalled path — in which case nothing in `just sweep` exercises the
deferral logic, and the plan's verification table overstates what Phase 3's
sweep proves.

**Resolution (2026-08-06):** textures are immortal for now. Phase 3 deletes
`drop_texture` and the then-dead `TextureStorage::take` outright (keeping
`destroy_texture` for the post-`device_wait_idle` shutdown path), and builds no
slot-release machinery. Removal returns later as a bindless-specific heap
add/remove API with deferred slot reuse *and* deferred object destruction;
bound (classic-path) textures stay owned by their pipelines / immortal.

### 4. Phase 5's `Gpu::addr`-style accessor can't be written as described

`Gpu` (renderer.rs:5132-5136) holds only `flight_slot`, `uniform_buffers`,
and `storage_buffers` — it has no access to `TextureStorage`, so it can't
resolve a `TextureHandle` to a heap slot. Either `Gpu` gains a reference to
the texture slab (or a slot table), or the slot gets stored inside
`TextureHandle` at creation so the accessor needs no lookup. Relatedly:
`TextureStorage` never reuses slab indices (tombstones only), so the heap
slot allocator must be its own free-list — **slab index ≠ heap slot**. The
plan implies this (slot stored on `Texture`) but never says it.

**Resolution (2026-08-06):** with textures immortal (see item 3), the
free-list collapses to a monotonic counter; slab index ≠ heap slot still
holds. The accessor question is resolved by the second option: the slot is
stored **in the `TextureHandle`** at creation, so the `Gpu` accessor reads it
off the handle with no `TextureStorage` lookup.

### 5. The codegen-table pointer is slightly off

The "size/alignment tables" at build_tasks.rs:1339/:1378 are both
*alignment*-only (`field_alignment_by_name`, `rust_type_alignment`). There
is a size table, but it's the test-only `rust_size_of` (build_tasks.rs:2037)
feeding the `field_size_tripwire` test — and it has no `Addr<` arm today, so
pointer-width fields are silently skipped by the tripwire. If
`BindlessHandle` should get tripwire coverage (the plan's own verification
section argues these compile-time checks matter *more* here), that's a third
table to touch, not two.

## Risks / underspecified

### 6. Arrays of handles bypass the Phase 1 check

The `TypeKind::Array` arm (reflection/parameters.rs:433-471) never recurses
into `reflect_struct_fields`, so a check placed next to the enum block won't
see `Sampler2D.Handle handles[4]`. It *is* rejected — `validate_array_element`
only accepts 16-byte vec4-shaped elements — but with a generic array message,
not the loud handle-specific one Phase 1 promises. Same failure shape as the
existing `enum_arrays_are_rejected` case. Fine for safety, but worth a
fixture and a sentence in the plan (the spike lists arrays of handles as
untested; this closes that question for the reflection side).

### 7. Stale contradiction about the `bindless_space_index` target option

"What's already in place" still says both target descs "need it"
(shaders.rs:80-82 and :176-178), but Phase 4 explicitly drops passing any
floor — and the spike's unset-option row shows reflection reports the index
fine without it. As written, an implementer may add the option to two call
sites for nothing. And if the option ever *is* passed, there is a **third**
`TargetDesc` construction the "both" claim misses:
`reflect_shared_module_types` at shaders.rs:256-258. Pick one story and state
it.

### 8. egui silently clobbers descriptor bindings

`egui_ash_renderer` records its own pipeline and descriptor binds into our
command buffer (renderer.rs:~2256). The "bind heap after each
`cmd_bind_pipeline`" rule holds today only because egui is recorded last.
That's an implicit ordering invariant — worth a comment at the egui call site
so a future pass added after it doesn't hit a mysteriously-unbound heap.

### 9. Heap layout details left implicit

Stage flags for the heap binding aren't specified — reflected global bindings
come out as `ShaderStageFlags::ALL`
(reflection/pipeline_layout.rs:40, :262), and the heap should match. And
`MAX_BINDLESS_TEXTURES = 4096` should be validated at startup against
`maxPerStageDescriptorUpdateAfterBindSampledImages` /
`maxDescriptorSetUpdateAfterBindSampledImages` rather than assumed, given the
suitability gate only warn-skips devices.

### 10. Hot reload: confirmed, with a sharper consequence than the plan implies

`assert_shader_interface_unchanged` (renderer.rs:4814) is a whole-JSON
`serde_json::Value` equality compare that **panics** ("run `just shaders` and
rebuild"). So Phase 4's "confirm that behaves sanely" resolves to: adding or
removing a handle in a shader during `just dev` is a hard stop, same as any
interface change — expected, but it means handle-bearing shaders can't be
iterated live across interface changes. Also note it's debug-only; release
builds trust the embedded JSON, so the per-shader "uses heap" flag genuinely
must live in that JSON (the plan does this — good).

### 11. The `NonUniformEXT` invariant is unenforced

The plan's analysis is correct and honest, but "material index must be
dynamically uniform within a draw" ends up as an invariant living only in an
llm_note. Nothing in the code, codegen, or docs will stop someone from
indexing materials by instance/vertex data later and getting silent UB on
hardware that cares. When Phase 7 updates `docs/`, this invariant belongs
there, stated as a hard rule.

## Doc nits

- `pipeline_layout.rs` paths are missing the `reflection/` segment throughout
  (`crates/renderer/src/shaders/reflection/pipeline_layout.rs`).
- The enum precedent at parameters.rs is actually lines 177-198, and it checks
  `field.ty().kind() == TypeKind::Enum` — it does **not** parse `full_name()`.
  The full-name-prefix technique Phase 1 proposes is borrowed from the
  *pointer* arm (:360-364), not the enum arm. Also don't inherit the enum
  arm's `Binding::Uniform` bail wholesale; a handle in a vertex-input position
  needs its own message.
- "In the style of the `StructuredBuffer` rejection" should cite only
  parameters.rs:298 (`bail!`, field-named); the pipeline_layout.rs:328 one is
  a `panic!` backstop documented as unreachable.
- The dependency is the `shader-slang` crate (the repo is *named* slang-rs);
  `create_texture` (renderer.rs:495) is a third public creation entry point
  the plan omits; minor line drift elsewhere (`old_pipelines` spans 116-121,
  the retirement loop 2619-2645, the gate query starts at 3209).

## Verified sound (no action)

- Phase 3's decision to create only binding 1 (diverging from the spike's
  "create 0, 1 and 2") is sound given the Phase 1 shape rejection.
- "One set, not `MAX_FRAMES_IN_FLIGHT` copies" is correct under
  update-after-bind semantics; fixed-count layout + `PARTIALLY_BOUND` vs. the
  shader's unbounded array is valid as long as nothing indexes past the count.
- The fork pin at `v0.1.1+slang-2026.13.1` really does contain both
  `bindless_space_index` API halves (`CompilerOptions` option and
  `ShaderReflection::bindless_space_index()`).
- The toon_link claims check out exactly: one pipeline + one uniform buffer
  per material (main.rs:780-826), one index-range draw per batch
  (main.rs:1178-1186).

## The one thing to settle before Phase 1

Item 3. Building slot release on top of an uncalled, immediate-destroy
`drop_texture` means the deferral logic ships untested by anything; a small
`drop_texture` fix (or an explicit "textures are immortal for now" statement
in Phase 3) would make the plan honest about what `just sweep` can actually
verify.

**Resolution (2026-08-06):** settled as immortal-for-now — the dead path gets
deleted in Phase 3 instead of built upon, so there is no untested deferral
logic and the Phase 3 sweep claim is honest. See the resolutions on items 2-4
above for the details.
