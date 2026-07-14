# Vulkan 1.3 Migration: Dynamic Rendering, Sync2, Timeline Semaphores, Extended Dynamic State, Opt-in BDA

Status: Phase 0 complete (2026-07-14); Phases 1–7 pending

## Context

The renderer already requests `vk::API_VERSION_1_3` at instance, device, and VMA creation — but uses none of the 1.3 features. This migration adopts them for quality-of-life wins: dynamic rendering deletes all render pass/framebuffer machinery (and framebuffer recreation on resize), synchronization2 gives clearer barrier semantics, timeline semaphores collapse the fence/binary-semaphore/bootstrap-flag frame sync, extended dynamic state shrinks pipeline permutations, and buffer device addresses (opt-in via Slang pointer types) open the door to simpler storage-buffer access and future GPU-driven rendering.

**BDA scope decision:** opt-in. Extend reflection/codegen to support `T*` pointer fields in parameter blocks; existing descriptor-bound shaders keep working unchanged. Migrate exactly two examples as proof (`sprite_batch` graphics + `particles` compute); remaining examples migrate later, one-by-one.

**Effort estimate:** ~7 phases; the runtime work is concentrated in `src/renderer.rs` (~20 change sites for dynamic rendering, ~14 barrier/submit sites for sync2), and the BDA codegen work threads a new pointer concept through 5 files. Each phase lands green independently.

**Hardware/market support:** Steam's hardware survey publishes no Vulkan version breakdown, but the 1.3 hardware floor (NVIDIA Maxwell+, AMD Polaris+ on Windows / all GCN on Linux RADV, Intel Gen9+) covers ~95%+ of surveyed systems; the excluded classes (Kepler, pre-Polaris AMD, pre-Skylake Intel) have fallen off the visible GPU list. The real-world gap is stale drivers reporting 1.2 on capable hardware — handled by a clean error at device selection (Phase 0). Within the 1.3 population there is no feature fragmentation: core 1.3 mandates `dynamicRendering`, `synchronization2`, and `bufferDeviceAddress`; 1.2 mandated `timelineSemaphore`.

**Retired risks (verified in code):**
- The pinned `slang-rs` fork (`fad6e14`) already exposes `SlangTypeKind::Pointer`, `SlangScalarType::Uint64`, `Type::element_type()`, and `CompilerOptions::capability()` — no fork changes needed.
- Compiled `.spv` files are SPIR-V 1.5 despite the `spirv_1_6` profile atom (slang emits the minimum version the module needs). Benign: `PhysicalStorageBuffer64` is core in SPIR-V 1.5.
- `egui-ash-renderer` 0.11 has a compile-time `dynamic-rendering` cargo feature; constructor takes `DynamicRendering { color_attachment_format, depth_attachment_format }` instead of a render pass.

## Phase ordering

```
0. Feature enablement restructure   (prerequisite for everything)
1. Synchronization2                 (so Phase 2's new explicit barriers are written once, in sync2 style)
2. Dynamic rendering                (removes passes/framebuffers; needs sync2-style transitions)
3. Timeline-semaphore frame sync    (builds on SubmitInfo2/SemaphoreSubmitInfo from Phase 1)
4. Extended dynamic state           (independent)
5. BDA renderer plumbing            (no shader changes yet)
6. Slang reflection/codegen pointers (build-time only, snapshot-tested)
7. Proof examples: sprite_batch + particles
```

Each phase ends green: `cargo check --all`, `just test`, `just shaders` where relevant, `timeout 3 just dev EXAMPLE` for a representative set (`basic_triangle`, `suzanne`, `sprite_batch`, particles, picking, watercolor) with zero validation errors.

## Phase 0 — Device feature enablement restructure ✅ (done 2026-07-14)

Implemented as planned, plus: `choose_physical_device` logs a per-device warning naming each missing feature before skipping it, and the no-device error names the 1.3 requirements. Verified: `cargo check --all`, `just lint` (release clippy), `just test` (70 passed), six examples clean under validation in debug, three in release.

`src/renderer.rs`: `create_logical_device` (~3007–3060), `choose_physical_device` (~2763+), allocator creation (288–298), `REQUIRED_DEVICE_EXTENSIONS` (2753–2761).

- Rewrite to `vk::PhysicalDeviceFeatures2` chain, unified across debug/release (`DeviceCreateInfo::push_next(&mut features2)`, drop `.enabled_features()`):
  - `Vulkan11Features::shader_draw_parameters(true)` — then remove the `KHR_SHADER_DRAW_PARAMETERS` extension from `REQUIRED_DEVICE_EXTENSIONS` (core 1.1; today only the extension is requested and the feature bit is never enabled).
  - `Vulkan12Features`: `timeline_semaphore(true)`, `buffer_device_address(true)` (all builds now); debug-only extras set conditionally on the same struct: `vulkan_memory_model`, `vulkan_memory_model_device_scope`, `storage_buffer8_bit_access`.
  - `Vulkan13Features`: `dynamic_rendering(true)`, `synchronization2(true)`.
  - Legacy `PhysicalDeviceFeatures` (sampler_anisotropy, sample_rate_shading, debug shader_int64) live inside `features2.features`. This collapses the four debug-only pNext structs at 3036–3055.
- `choose_physical_device`: query via `get_physical_device_features2` with chained 12/13 structs; reject devices lacking the needed bits (mandatory on conformant 1.3 hardware, but gives a clean error).
- Remove `#[cfg(debug_assertions)]` around VMA `AllocatorCreateFlags::BUFFER_DEVICE_ADDRESS` (293–296).
- Verify in **release too** (`cargo run --release --example ...`) — release previously attached no feature chain at all.

## Phase 1 — Synchronization2

`src/renderer.rs`: barriers at 729–807, 1342–1360, 1664–1760, `transition_image_layout` 4408+, `FrameRenderer::memory_barrier` ~5305; submits at 1998–2008, 2022–2032, 2087–2097, 4390–4400.

- Add private helpers wrapping `vk::DependencyInfo` + `cmd_pipeline_barrier2`; convert every `ImageMemoryBarrier`/`MemoryBarrier` to the `2` variants with stage/access masks on the barrier (use precise `PipelineStageFlags2::BLIT`/`COPY` where it's a blit/copy).
- Convert the 4 submit sites to `queue_submit2` with `CommandBufferSubmitInfo` + `SemaphoreSubmitInfo` (`.stage_mask()` replaces `wait_dst_stage_mask`).
- Fences/binary semaphores untouched this phase — only submit structs change.

## Phase 2 — Dynamic rendering

`src/renderer.rs`, `src/renderer/picking.rs`, `src/renderer/egui.rs`, `Cargo.toml`. Convert one pass at a time (main → picking → egui), running validation between each.

**Pipeline creation** — `create_graphics_pipeline` (3325): replace the `render_pass` param with attachment formats; `push_next` a `vk::PipelineRenderingCreateInfo` (`.color_attachment_formats`, `.depth_attachment_format`; `UNDEFINED` depth for picking); delete `.render_pass()/.subpass()` (3419–3420). Hoist `find_depth_format` result to a Renderer field. Update the 3 callers (944, 1162, 2383) and hot-reload rebuild.

**Main pass** (begin/end 1555–1566/1644): replace with `cmd_begin_rendering`/`cmd_end_rendering`.
- Pre-barriers (sync2, replacing the entry subpass dependency): MSAA color `UNDEFINED→COLOR_ATTACHMENT_OPTIMAL`; depth `UNDEFINED→DEPTH_ATTACHMENT_OPTIMAL`; resolve image `UNDEFINED→COLOR_ATTACHMENT_OPTIMAL`.
- Color `RenderingAttachmentInfo` with `.resolve_mode(AVERAGE)`, `.resolve_image_view(resolve_image_views[current_frame])`, `.load_op(CLEAR)`, `.store_op(DONT_CARE)` (only the resolve is consumed); depth CLEAR/DONT_CARE.
- Post-barrier: resolve image `COLOR_ATTACHMENT_OPTIMAL→TRANSFER_SRC_OPTIMAL` before the render-scale blit (replaces `final_layout` + exit dependency).
- Delete `create_render_pass` (3114–3223), `create_framebuffers` (3436–3460), fields at 108–109.

**Picking pass** (`picking.rs` 87–193; begin/end renderer.rs 1424–1434/1473): pre-barrier `UNDEFINED→COLOR_ATTACHMENT_OPTIMAL`, single CLEAR color attachment, no depth; post-barrier `→TRANSFER_SRC_OPTIMAL` before readback. Delete pass/framebuffer fns + fields + Drop cleanup (picking.rs:82).

**egui pass** (1786–1819, `egui.rs:22–51`): enable the `dynamic-rendering` cargo feature on `egui-ash-renderer`; construct with `DynamicRendering { color_attachment_format: swapchain_format, depth_attachment_format: None }`; delete `set_render_pass`. Layout change: dynamic rendering can't render into `PRESENT_SRC_KHR` — blit post-barrier goes `→COLOR_ATTACHMENT_OPTIMAL` when egui is active (else `→PRESENT_SRC`); egui renders LOAD/STORE on the swapchain view; final barrier `→PRESENT_SRC_KHR` after `cmd_end_rendering`. This sub-step lands atomically (the cargo feature is all-or-nothing); read the feature-gated API in `~/.cargo/registry/.../egui-ash-renderer-0.11.0/src/renderer/mod.rs` before starting.

**Swapchain recreation** (2153–2290): remove all framebuffer recreation — resize now only rebuilds swapchain, image views, and MSAA/depth/resolve images.

Verify: MSAA resolve (suzanne/viking_room), render-scale blit, egui editor overlay, picking clicks, resize stress.

## Phase 3 — Timeline-semaphore frame sync

`src/renderer.rs`: sync fields 130–148, `create_sync_objects` 3490–3540, `draw_frame` 1876–2145.

Two timeline semaphores (`SemaphoreTypeCreateInfo::semaphore_type(TIMELINE)`) replace two fence arrays and two binary-semaphore arrays:
- `frame_timeline`: graphics submit for frame N signals value N; the CPU fence-wait becomes `wait_semaphores(frame_timeline ≥ N − MAX_FRAMES_IN_FLIGHT)` (wait on ≤0 returns immediately — replaces pre-signaled fences; `reset_fences` disappears). Deletes `frames_in_flight` fences.
- `compute_timeline`: compute submit N waits ≥N−1 / signals N; graphics submit N waits ≥N−1 with stage `VERTEX_SHADER|FRAGMENT_SHADER|COMPUTE_SHADER` (also fixes a pre-existing latent under-sync: the old wait mask was `FRAGMENT_SHADER` only, but particle buffers are read in the vertex stage). Deletes `compute_finished`, `compute_to_graphics_sem`, `compute_fences`, and the `compute_bootstrapped` three-way branch at 2056–2085 (waiting on 0 succeeds trivially on frame 1).
- **Must stay binary (WSI constraint):** `image_available` (acquire can't signal timeline) and per-swapchain-image `render_finished` (present can't wait timeline). They mix freely into `SubmitInfo2` — timeline entries just set `.value()`.
- Frame counter: ensure exactly one increment per submitted frame (audit early-return recreate paths, ~1918); never recreate timeline semaphores on swapchain recreation (values must stay monotonic).

Verify: pipelined compute (particles/watercolor), resize storms, clean shutdown mid-flight.

## Phase 4 — Extended dynamic state

`src/renderer.rs`: `create_graphics_pipeline` 3354–3407, draw recording 1570–1641, picking draw 1424–1470, `RendererPipeline`.

- Extend `dynamic_states` with core-1.3 states: `CULL_MODE`, `FRONT_FACE`, `PRIMITIVE_TOPOLOGY`, `DEPTH_TEST_ENABLE`, `DEPTH_WRITE_ENABLE`, `DEPTH_COMPARE_OP` (no feature bit needed; do NOT include blend enable — that's EDS3, not core).
- Store choices in a small `DynamicPipelineState` struct on `RendererPipeline`; populate from existing `PipelineOptions`/`depth_test_enable` inputs (drop that param from `create_graphics_pipeline`; keep the static create-info structs as ignored placeholders).
- After every `cmd_bind_pipeline` (graphics + picking): `cmd_set_cull_mode`, `cmd_set_front_face`, `cmd_set_primitive_topology`, `cmd_set_depth_test_enable`, `cmd_set_depth_write_enable`, `cmd_set_depth_compare_op`.
- Hot-reload rebuilds no longer vary by these states.

## Phase 5 — BDA renderer plumbing

`src/renderer.rs` (`create_storage_buffer` 862–892, `Gpu` ~5191), `src/renderer/storage_buffer.rs`.

- Enable on **all storage buffers** (no opt-in flag — cost is one usage bit + one query at creation): add `BufferUsageFlags::SHADER_DEVICE_ADDRESS`; after creation call `get_buffer_device_address` and cache `device_address: vk::DeviceAddress` on `RawStorageBuffer` (one per `BUFFER_FRAME_COUNT` copy).
- Expose via the `Gpu` view (the buffer_frame-aware handle apps write uniforms through): `gpu.device_address(&storage_handle) -> u64` for the current buffer_frame, plus an all-frames variant for init paths.
- Uniform buffers stay descriptor-bound (addresses live *in* uniform data, not vice versa).
- Fix the latent push-constant bug while here: `ReflectedPushConstantRange::to_vk` at renderer.rs:5164–5168 sets `.offset(self.size)` — should be the range's offset. Dormant today (no `cmd_push_constants` anywhere) but becomes live if a shader declares one.

## Phase 6 — Slang reflection/codegen pointer support

`src/shaders/reflection/parameters.rs`, `src/shaders/json/parameters.rs`, `src/shaders/build_tasks.rs`, `templates/shader_atlas_entry.rs.askama` + compute/shared-module templates, `shaders/test/`, insta snapshots.

- **Reflection** (`parameters.rs`): add `Uint64` to `scalar_from_slang` (339–345, currently `todo!()`s); add a `slang::TypeKind::Pointer` arm to the struct-field walk (the `todo!()` at ~322) → new `StructField::Pointer` variant carrying field name, pointee type name (`element_type_layout`), and the existing `Binding::Uniform` offset/size (a pointer is 8 bytes of uniform data in the parameter block — it consumes no descriptor slot, so it naturally stays out of `binding_ranges` and `Resources`).
- **JSON** (`json/parameters.rs`): `Uint64` in `ScalarType` (235–240); `Pointer` variant in `StructField` (84–90), serde-tagged consistently.
- **Codegen** (`build_tasks.rs` + templates): scalar maps (~909, ~1037–1045) map `Uint64 => "u64"`; a pointer field emits `pub name: u64` in the generated param-block struct at the reflected offset. **No `Resources<'a>`/`pipeline_config()` change**: the address is uniform data written per-frame, not a resource handle. Verify `create_descriptor_sets` ignores pointer fields (no descriptor binding exists for them).
- **Reflection-driven struct layout** (decided 2026-07-14): generated struct padding/alignment must come from Slang's *reflected field offsets*, not the codegen's own std140/std430 alignment table (`build_tasks.rs:~1176`). Rationale: a `T*` pointee is laid out by a separate compiler path (`PhysicalStorageBuffer64`) whose rules are not guaranteed to be std430 (may be scalar/natural layout, where `float3` aligns to 4 not 16); keeping a local re-implementation of layout rules creates a second source of truth that silently diverges. Consuming reflected offsets makes Rust match whatever Slang decided, by construction — and hardens the existing descriptor path too. Additionally emit **per-field `core::mem::offset_of!` asserts** (not just total-size asserts) so any future mismatch fails at `cargo check`, since layout bugs through raw device addresses produce no validation errors — just silently wrong data.
- **glam type handling under reflection-driven layout**: reflected offsets say where a field must land; where it actually lands is governed by the *Rust* size/align of glam types, which the generator must model. Sizes are stable (`Vec2`=8, `Vec3`=12, `Vec4`=16, `Mat4`=64) but alignments are feature-dependent — default glam features enable SIMD, making `Vec4`/`Quat`/`Mat2`/`Mat4` align-16 while `Vec3` stays align-4 (that align-4 is what lets fields tail-pack after a `float3`, e.g. `Sprite.rotation` at offset 12 — never substitute `Vec3A`). Rules: (a) padding fields are `[u8; N]` (align 1); (b) if a reflected offset isn't a multiple of the Rust type's alignment (possible only under scalar pointer layout, e.g. `float4` at offset 4), fall back to `[f32; 4]` for that field or error clearly; (c) emit an `align_of::<glam::Vec4>() == 16` assert so a transitively-enabled glam `scalar-math` feature is caught at compile time. **Latent bug to resolve while here** (decided: hard error): `build_tasks.rs:993-995` maps `float2x2/3x3 → glam::Mat2/Mat3`, but GPU matrix layouts have interior row/column stride padding (std140 mat3 = 48 bytes vs glam::Mat3's 36 contiguous; std140 mat2 = 32 vs Mat2's 16) that field-level padding cannot express. Dormant today — only `Mat4` (layout-safe: 64 contiguous bytes under every rule set) appears in parameter blocks; all float2x2 uses are shader-local. Resolution: remove the Mat2/Mat3 arms and emit a clear codegen error if a float2x2/float3x3 appears in a parameter block, with the workaround in the message (use float4x4 or padded vec rows); add padded-array support only when a shader actually needs it.
- **Compiler options** (`src/shaders.rs`): expect no change — slang auto-enables `PhysicalStorageBuffer64` + `SPV_KHR_physical_storage_buffer` when pointers appear (legal in SPIR-V 1.5). Fallback if needed: `session_options.capability(find_capability(...))` (verified available in the fork). Optionally factor the 3 duplicated option blocks (L48–122/129–198/207–253) into one helper.
- **Tests first**: add a pointer-using test shader in `shaders/test/` with a deliberately layout-hostile struct (unpadded `float3`s, a lone `float` before a `float4`); `spirv-dis` the output and compare `OpMemberDecorate Offset`/`ArrayStride` on the PhysicalStorageBuffer-class struct vs the same struct descriptor-bound — this empirically settles what layout Slang uses for pointer pointees before any Rust depends on it. Then `just test` + review new snapshots; confirm **zero changes to existing snapshots** (proves descriptor path untouched — note: switching padding to reflection-driven offsets should be byte-identical for existing std140/std430 structs; if snapshots churn, the reflected offsets disagree with the old table and each diff needs review).

## Phase 7 — Proof examples

- **sprite_batch** (graphics): `shaders/source/sprite_batch.shader.slang` — replace `StructuredBuffer<Sprite> sprites` with `Sprite* sprites` in the ParameterBlock; app sets `params.sprites = gpu.device_address(&sprite_buffer)` in the per-frame update. Storage buffer created as before, just never descriptor-bound. Confirm the Rust `Sprite` repr matches slang's pointer-load layout (should already match std430; add size assert if not).
- **particles** (compute): `particles.compute.slang` — `Particle* particlesIn/particlesOut` in the params struct; `particle_render.shader.slang` — `Particle* particles` for the vertex-stage read. App supplies addresses respecting the existing ping-pong buffer_frame rotation (the address written into frame N's uniform copy must target the same prev/current buffer copies the descriptor `PingPong` strategy used). Keep the pipelined-compute path exercised.
- Regenerate everything (`just shaders`).

## Verification (overall gate)

- `cargo check --all`, `cargo fmt`, `just lint`, `just test` green.
- All examples run clean under validation (`timeout 3 just dev EXAMPLE`), debug **and** release.
- Feature-specific: MSAA resolve visuals, render-scale blit, egui editor overlay, picking clicks, resize stress, shader hot-reload (edit a `.slang` while `just dev` runs), pipelined compute.
- `spirv-val`/`spirv-dis` on the two migrated shaders.

## Remaining risks

- **Slang pointer struct layout** vs generated `#[repr]` structs: a `T*` pointee is laid out under `PhysicalStorageBuffer64` rules that may differ from the std430 used for `StructuredBuffer<T>` (e.g. scalar layout: `float3` at align 4). Failure mode is *silent* wrong data — no validation error. Mitigated three ways: (1) Phase 6's spirv-dis experiment settles the actual layout before any Rust depends on it; (2) reflection-driven offsets + per-field `offset_of!` asserts in codegen (see Phase 6) make divergence a compile error; (3) both proof structs are layout-invariant across std140/std430/scalar (`Particle {float2,float2,float4}` trivially; `Sprite` via its hand-inserted `float2 padding` field) so Phase 7 can't hit it. Residual watch-items: **dual-context types** — during incremental migration, a buffer may be accessed via `Particle*` in one shader and `StructuredBuffer<Particle>` in another; both GPU layouts must agree with each other (fine if pointer layout turns out to be std430; if not, migrate all shaders sharing a struct together), and shared-module codegen emits one Rust struct per type so a type genuinely can't have two layouts.
- **Sync-validation complaints** after replacing implicit subpass dependencies with explicit barriers: convert one pass at a time within Phase 2.
- **Timeline + swapchain-recreate edge cases** (frame-counter increments on early-return paths; the code comment at ~2108 shows this area has bitten before): test resize storms.
- **`shader_draw_parameters`** feature bit newly actually-enabled in Phase 0 (previously extension-only): strictly more correct, but watch draw-parameter-using examples.
