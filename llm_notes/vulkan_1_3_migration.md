# Vulkan 1.3 Migration: Dynamic Rendering, Sync2, Timeline Semaphores, Extended Dynamic State, Opt-in BDA

Status: **Migration complete** — Phases 0–3, 5, 6, and 7 done (Phase 7 done 2026-07-18); Phase 4 won't do (decided 2026-07-14). Pointer shaders must use `LayoutPtr<T, Std430DataLayout>` (see Phase 6 companion note).

Related notes:
- [vulkan_1_3_migration/bindless_vs_bda_terminology.md](vulkan_1_3_migration/bindless_vs_bda_terminology.md) — how the BDA pointer-tree direction relates to "bindless", search terms, reading list.
- [vulkan_1_3_migration/timeline_semaphores.md](vulkan_1_3_migration/timeline_semaphores.md) — timeline semaphore primer, WSI limits, and the Phase 3 old→new sync-object mapping.
- [vulkan_1_3_migration/dynamic_state.md](vulkan_1_3_migration/dynamic_state.md) — dynamic state primer, benefits/costs, and the (reverted, won't-do) Phase 4 design for reference.
- [vulkan_1_3_migration/bda_renderer_plumbing.md](vulkan_1_3_migration/bda_renderer_plumbing.md) — BDA primer and the detailed Phase 5 design (address caching, `Gpu` API shape incl. the PingPong-mirroring `device_address_prev`).
- [vulkan_1_3_migration/slang_pointer_codegen.md](vulkan_1_3_migration/slang_pointer_codegen.md) — the Phase 6 design and implementation record (JSON `PointerStructField` shape, the pointee-layout findings incl. why `LayoutPtr<T, Std430DataLayout>` is required, per-field `offset_of!`/`size_of` asserts, steps A–F with results).

## Context

At the outset, the renderer already requested `vk::API_VERSION_1_3` at instance, device, and VMA creation — but used none of the 1.3 features. This migration adopts them for quality-of-life wins: dynamic rendering deletes all render pass/framebuffer machinery (and framebuffer recreation on resize), synchronization2 gives clearer barrier semantics, timeline semaphores collapse the fence/binary-semaphore/bootstrap-flag frame sync, extended dynamic state shrinks pipeline permutations, and buffer device addresses (opt-in via Slang pointer types) open the door to simpler storage-buffer access and future GPU-driven rendering.

**BDA scope decision:** opt-in. Extend reflection/codegen to support `T*` pointer fields in parameter blocks; existing descriptor-bound shaders keep working unchanged. Migrate exactly two examples as proof (`sprite_batch` graphics + `particles` compute); remaining examples migrate later, one-by-one.

**Effort estimate:** ~7 phases; the runtime work is concentrated in `src/renderer.rs` (~20 change sites for dynamic rendering, ~14 barrier/submit sites for sync2), and the BDA codegen work threads a new pointer concept through 5 files. Each phase lands green independently.

**Hardware/market support:** Steam's hardware survey publishes no Vulkan version breakdown, but the 1.3 hardware floor (NVIDIA Maxwell+, AMD Polaris+ on Windows / all GCN on Linux RADV, Intel Gen9+) covers ~95%+ of surveyed systems; the excluded classes (Kepler, pre-Polaris AMD, pre-Skylake Intel) have fallen off the visible GPU list. The real-world gap is stale drivers reporting 1.2 on capable hardware — handled by a clean error at device selection (Phase 0). Within the 1.3 population there is no feature fragmentation: core 1.3 mandates `dynamicRendering`, `synchronization2`, and `bufferDeviceAddress`; 1.2 mandated `timelineSemaphore`.

**Retired risks (verified in code or by landing):**
- The pinned `slang-rs` fork (`fad6e14`) already exposes `SlangTypeKind::Pointer`, `SlangScalarType::Uint64`, `Type::element_type()`, and `CompilerOptions::capability()` — no fork changes needed. (Phase 6 correction: `Type::element_type()` returns None on pointer types; the pointee is resolved via `element_type_layout().ty()` instead.)
- Compiled `.spv` files are SPIR-V 1.5 despite the `spirv_1_6` profile atom (slang emits the minimum version the module needs). Benign: `PhysicalStorageBuffer64` is core in SPIR-V 1.5.
- `egui-ash-renderer` 0.11 has a compile-time `dynamic-rendering` cargo feature; constructor takes `DynamicRendering { color_attachment_format, depth_attachment_format }` instead of a render pass. (Landed cleanly in Phase 2.)
- Sync-validation complaints from replacing implicit subpass dependencies with explicit barriers: didn't materialize — Phase 2 landed with zero validation errors across all 14 examples.

## Phase ordering

```
0. Feature enablement restructure   (prerequisite for everything)
1. Synchronization2                 (so Phase 2's new explicit barriers are written once, in sync2 style)
2. Dynamic rendering                (removes passes/framebuffers; needs sync2-style transitions)
3. Timeline-semaphore frame sync    (builds on SubmitInfo2/SemaphoreSubmitInfo from Phase 1)
4. Extended dynamic state           (independent — won't do)
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

## Phase 1 — Synchronization2 ✅ (done 2026-07-14)

Implemented: all 9 `cmd_pipeline_barrier` sites → `ImageMemoryBarrier2`/`MemoryBarrier2` via a new `cmd_barrier2` helper (next to `end_single_time_commands`); all 4 submits → `queue_submit2` with `SemaphoreSubmitInfo`/`CommandBufferSubmitInfo`; `FrameRenderer::memory_barrier` + `PendingComputeCommand::Barrier` retyped to `Flags2` (callers in `examples/particles.rs` and `examples/watercolor.rs` updated). Precise stages used: `COPY` for buffer→image uploads, `BLIT` for the render-scale blit and mipmap chain (src stays `ALL_TRANSFER` where the producer is mixed copy/blit). The compute→graphics wait mask was widened from `FRAGMENT_SHADER` to `VERTEX|FRAGMENT|COMPUTE_SHADER` at both submit sites (fixes the latent vertex-stage under-sync, originally slated for Phase 3). Render-pass `SubpassDependency`s intentionally still sync1 — deleted in Phase 2. Verified: check/lint/test green; 8 examples clean under validation in debug; release spot-check clean.

`src/renderer.rs`: barriers at 729–807, 1342–1360, 1664–1760, `transition_image_layout` 4408+, `FrameRenderer::memory_barrier` ~5305; submits at 1998–2008, 2022–2032, 2087–2097, 4390–4400.

- Add private helpers wrapping `vk::DependencyInfo` + `cmd_pipeline_barrier2`; convert every `ImageMemoryBarrier`/`MemoryBarrier` to the `2` variants with stage/access masks on the barrier (use precise `PipelineStageFlags2::BLIT`/`COPY` where it's a blit/copy).
- Convert the 4 submit sites to `queue_submit2` with `CommandBufferSubmitInfo` + `SemaphoreSubmitInfo` (`.stage_mask()` replaces `wait_dst_stage_mask`).
- Fences/binary semaphores untouched this phase — only submit structs change.

## Phase 2 — Dynamic rendering ✅ (done 2026-07-14)

Implemented as planned. Notes beyond the plan: `create_graphics_pipeline` takes `(color_format, depth_format: Option<vk::Format>)` (None for picking → `Format::UNDEFINED` in `PipelineRenderingCreateInfo`); a shared `COLOR_SUBRESOURCE_RANGE` const was added; the swapchain-to-present transition uses `dst_stage NONE/NONE` (present orders via the render_finished semaphore); the egui-active blit post-barrier dst access is `COLOR_ATTACHMENT_READ|WRITE` since egui LOADs. `EguiIntegration::new` now takes the swapchain format; `set_render_pass` deleted (it was never called on resize anyway — formats don't change there). Depth aspect derives from the new `Renderer::depth_format` field via `has_stencil_component`. All render pass/framebuffer functions, fields, and destroy calls deleted — zero references to RenderPass/Framebuffer/sync1 flags remain in src/. Verified: check/lint/test green; all 14 examples run clean under validation in debug (incl. 4 egui-using ones and gpu_picking); release spot-checks clean. Not covered by automation: visual parity and resize stress — worth a quick manual look.

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

## Phase 3 — Timeline-semaphore frame sync ✅ (done 2026-07-14)

Implemented as planned, with two deliberate deviations. (1) `compute_timeline` values come from a dedicated `compute_frames: u64` counter (incremented per compute-signaling submit), not the frame number — `has_compute_pipelines` can flip true mid-run, and frame-number values would make the first compute submit's wait `≥ N−1` unsatisfiable (deadlock; timeline signal values may jump but waits must still be reached). (2) `use_pipelined` gained a derived `compute_frames > 0` term replacing `compute_bootstrapped`, preserving the first-compute-frame-goes-combined behavior with no managed flag and nothing to reset on recreate. The pipelined compute-CB-reuse fence became a CPU wait `compute_timeline ≥ compute_value − MAX_FRAMES_IN_FLIGHT` (exact because compute_frames advances every frame once compute is active, and the CB slots alternate with frame parity). All fences deleted (`queue_submit2` now takes `Fence::null()` everywhere); one timeline signal per compute submit serves both the next compute wait (@COMPUTE_SHADER) and next graphics wait (@V|F|C). One behavior change: the first post-resize frame goes straight to pipelined (previously re-bootstrapped) — safe since `recreate_swapchain`'s `device_wait_idle` guarantees all signaled values are reached. Verified: check/fmt/lint/test green; basic_triangle, particles (non-pipelined), watercolor (pipelined), suzanne, gpu_picking clean under validation in debug; watercolor release spot-check clean; no frame-1/2 hang (CPU-usage check). Not covered by automation: resize storms and clean-shutdown-mid-flight — worth a quick manual look.

`src/renderer.rs` (anchors current as of end of Phase 2): sync fields 135–160, `create_sync_objects` ~3393, `Renderer::draw_frame` ~1967 (plus the delegating `FrameRenderer::draw_frame` ~5251), `recreate_swapchain` ~2270.

Two timeline semaphores (`SemaphoreTypeCreateInfo::semaphore_type(TIMELINE)`) replace two fence arrays and two binary-semaphore arrays:
- `frame_timeline`: graphics submit for frame N signals value N; the CPU fence-wait becomes `wait_semaphores(frame_timeline ≥ N − MAX_FRAMES_IN_FLIGHT)` (wait on ≤0 returns immediately — replaces pre-signaled fences; `reset_fences` disappears). Deletes `frames_in_flight` fences.
- `compute_timeline`: compute submit N waits ≥N−1 / signals N; graphics submit N waits ≥N−1 with the `VERTEX_SHADER|FRAGMENT_SHADER|COMPUTE_SHADER` stage mask (already widened in Phase 1 — carry it over). Deletes `compute_finished`, `compute_to_graphics_sem`, `compute_fences`, and the `compute_bootstrapped` three-way branch (~2166) (waiting on 0 succeeds trivially on frame 1).
- **Also deleted**: the `recreate_swapchain` block (~2273) that destroys/recreates `compute_finished` + `compute_to_graphics_sem` and resets `compute_bootstrapped` — it exists only because signaled binary semaphores can't be reset; timeline semaphores don't have that problem and must NOT be recreated there (values stay monotonic across recreation).
- **Must stay binary (WSI constraint):** `image_available` (acquire can't signal timeline) and per-swapchain-image `render_finished` (present can't wait timeline). They mix freely into `SubmitInfo2` — timeline entries just set `.value()`.
- Frame counter: ensure exactly one increment per submitted frame (audit early-return recreate paths); the "advance counters BEFORE present" comment in `draw_frame` marks a spot that has bitten before.

Verify: pipelined compute (particles/watercolor), resize storms, clean shutdown mid-flight.

## Phase 4 — Extended dynamic state ❌ (won't do, decided 2026-07-14)

Implemented, fully verified green (check/lint/test with zero snapshot churn; gpu_picking, suzanne, sprite_batch, space_invaders, viking_room clean under validation; release and hot-reload spot-checks clean), then **reverted by decision before commit**. Rationale: the phase is purely orthogonal — Phases 5–7 (BDA) have zero dependency on it, and the renderer's only actual static variation today is `depth_test_enable`, so the practical payoff (fewer pipeline permutations, state-agnostic hot reload) is speculative until real per-pipeline state variation exists. The resize benefit often attributed to dynamic state was already banked pre-migration via dynamic VIEWPORT/SCISSOR.

The full design and verified implementation shape are preserved in [vulkan_1_3_migration/dynamic_state.md](vulkan_1_3_migration/dynamic_state.md) — if per-pipeline state variation ever materializes (two-sided materials, line-topology debug views), that note is the restart point; the change is small (two files) and was proven clean.

`src/renderer.rs` (anchors current as of end of Phase 2): `create_graphics_pipeline` ~3250 (dynamic_states list inside it), main draw recording inside `record_command_buffer` ~1348+ (bind at the `cmd_bind_pipeline` after `cmd_begin_rendering`), picking draw in the same fn (~1400s), `RendererPipeline` in `src/renderer/pipeline.rs`.

- Extend `dynamic_states` with core-1.3 states: `CULL_MODE`, `FRONT_FACE`, `PRIMITIVE_TOPOLOGY`, `DEPTH_TEST_ENABLE`, `DEPTH_WRITE_ENABLE`, `DEPTH_COMPARE_OP` (no feature bit needed; do NOT include blend enable — that's EDS3, not core).
- Store choices in a small `DynamicPipelineState` struct on `RendererPipeline`; populate from existing `PipelineOptions`/`depth_test_enable` inputs (drop that param from `create_graphics_pipeline`; keep the static create-info structs as ignored placeholders).
- After every `cmd_bind_pipeline` (graphics + picking): `cmd_set_cull_mode`, `cmd_set_front_face`, `cmd_set_primitive_topology`, `cmd_set_depth_test_enable`, `cmd_set_depth_write_enable`, `cmd_set_depth_compare_op`.
- Hot-reload rebuilds no longer vary by these states.

## Phase 5 — BDA renderer plumbing ✅ (done 2026-07-14)

Implemented as designed in [vulkan_1_3_migration/bda_renderer_plumbing.md](vulkan_1_3_migration/bda_renderer_plumbing.md) (BDA primer, verified anchors, `Gpu` API shape, PingPong frame-offset semantics): `SHADER_DEVICE_ADDRESS` usage + per-copy address query/cache in `create_storage_buffer`, `device_address` field on `RawStorageBuffer`, `get_device_address_for_frame` accessor, `Gpu::device_address`/`Gpu::device_address_prev` (the latter mirroring PingPong's −1 frame offset) and `Renderer::device_addresses` for init paths. Verified: check/fmt/lint/test green with zero snapshot churn; basic_triangle, sprite_batch, particles, watercolor, suzanne, gpu_picking clean under validation in debug; particles release spot-check clean. Behaviorally invisible by design — the API's first real exercise is Phase 7.

`src/renderer.rs` (verified anchors 2026-07-14: `create_storage_buffer` 803–833, `Gpu` 5002–5044), `src/renderer/storage_buffer.rs`.

- Enable on **all storage buffers** (no opt-in flag — cost is one usage bit + one query at creation): add `BufferUsageFlags::SHADER_DEVICE_ADDRESS`; after creation call `get_buffer_device_address` and cache `device_address: vk::DeviceAddress` on `RawStorageBuffer` (one per `BUFFER_FRAME_COUNT` copy).
- Expose via the `Gpu` view (the buffer_frame-aware handle apps write uniforms through): `gpu.device_address(&storage_handle) -> u64` for the current buffer_frame, plus an all-frames variant for init paths.
- Uniform buffers stay descriptor-bound (addresses live *in* uniform data, not vice versa).
- ~~Fix the latent push-constant bug while here~~ ✅ done 2026-07-14, landed ahead of the phase: `impl ReflectedPushConstantRange::to_vk` (renderer.rs:4981–4988) now uses `.offset(self.offset)` instead of the old `.offset(self.size)`.

## Phase 6 — Slang reflection/codegen pointer support ✅ (done 2026-07-17)

Implemented per the companion note [vulkan_1_3_migration/slang_pointer_codegen.md](vulkan_1_3_migration/slang_pointer_codegen.md), which records the step-by-step results; the summary below is superseded by that note where they differ. Headline deviation from the sketch below: the Step A experiment found default `T*` pointees use slang's *natural* layout (not std430), so shaders must declare `LayoutPtr<T, Std430DataLayout>` (bare `T*` is a reflection hard error) and pointee offsets are reflected via `type_layout(_, DefaultStructuredBuffer)`. Also landed en route: per-field `offset_of!`/`size_of` asserts on all generated GPU structs, the glam align guard, the shared-module layout-compatibility check, the field-size tripwire test, and the matrix hard error (the "dormant" Mat3 bug was live in std430_matrices).

`src/shaders/reflection/parameters.rs`, `src/shaders/json/parameters.rs`, `src/shaders/build_tasks.rs`, `templates/shader_atlas_entry.rs.askama` + compute/shared-module templates, `shaders/test/`, insta snapshots.

- **Reflection** (`parameters.rs`): add `Uint64` to `scalar_from_slang` (339–345, currently `todo!()`s); add a `slang::TypeKind::Pointer` arm to the struct-field walk (the `todo!()` at ~322) → new `StructField::Pointer` variant carrying field name, pointee type name (`element_type_layout`), and the existing `Binding::Uniform` offset/size (a pointer is 8 bytes of uniform data in the parameter block — it consumes no descriptor slot, so it naturally stays out of `binding_ranges` and `Resources`).
- **JSON** (`json/parameters.rs`): `Uint64` in `ScalarType` (235–240); `Pointer` variant in `StructField` (84–90), serde-tagged consistently.
- **Codegen** (`build_tasks.rs` + templates): scalar maps (~909, ~1037–1045) map `Uint64 => "u64"`; a pointer field emits `pub name: u64` in the generated param-block struct at the reflected offset. **No `Resources<'a>`/`pipeline_config()` change**: the address is uniform data written per-frame, not a resource handle. Verify `create_descriptor_sets` ignores pointer fields (no descriptor binding exists for them).
- **Reflection-driven struct layout** (decided 2026-07-14): generated struct padding/alignment must come from Slang's *reflected field offsets*, not the codegen's own std140/std430 alignment table (`build_tasks.rs:~1176`). Rationale: a `T*` pointee is laid out by a separate compiler path (`PhysicalStorageBuffer64`) whose rules are not guaranteed to be std430 (may be scalar/natural layout, where `float3` aligns to 4 not 16); keeping a local re-implementation of layout rules creates a second source of truth that silently diverges. Consuming reflected offsets makes Rust match whatever Slang decided, by construction — and hardens the existing descriptor path too. Additionally emit **per-field `core::mem::offset_of!` asserts** (not just total-size asserts) so any future mismatch fails at `cargo check`, since layout bugs through raw device addresses produce no validation errors — just silently wrong data.
- **glam type handling under reflection-driven layout**: reflected offsets say where a field must land; where it actually lands is governed by the *Rust* size/align of glam types, which the generator must model. Sizes are stable (`Vec2`=8, `Vec3`=12, `Vec4`=16, `Mat4`=64) but alignments are feature-dependent — default glam features enable SIMD, making `Vec4`/`Quat`/`Mat2`/`Mat4` align-16 while `Vec3` stays align-4 (that align-4 is what lets fields tail-pack after a `float3`, e.g. `Sprite.rotation` at offset 12 — never substitute `Vec3A`). Rules: (a) padding fields are `[u8; N]` (align 1); (b) if a reflected offset isn't a multiple of the Rust type's alignment (possible only under scalar pointer layout, e.g. `float4` at offset 4), fall back to `[f32; 4]` for that field or error clearly; (c) emit an `align_of::<glam::Vec4>() == 16` assert so a transitively-enabled glam `scalar-math` feature is caught at compile time. **Latent bug to resolve while here** (decided: hard error): `build_tasks.rs:993-995` maps `float2x2/3x3 → glam::Mat2/Mat3`, but GPU matrix layouts have interior row/column stride padding (std140 mat3 = 48 bytes vs glam::Mat3's 36 contiguous; std140 mat2 = 32 vs Mat2's 16) that field-level padding cannot express. Not dormant after all (found 2026-07-16): `shaders/test/std430_matrices.shader.slang` uses float2x2/float3x3 in a StructuredBuffer element and its snapshot emits `glam::Mat3` with wrong interior column stride — the size assert passes only by coincidence, and per-field offset asserts can't catch interior stride. No *source* shader is affected (only `Mat4` appears in real parameter blocks); the test shader gets fixed alongside the hard error. Resolution: remove the Mat2/Mat3 arms and emit a clear codegen error if a float2x2/float3x3 appears in a parameter block, with the workaround in the message (use float4x4 or padded vec rows); add padded-array support only when a shader actually needs it.
- **Compiler options** (`src/shaders.rs`): expect no change — slang auto-enables `PhysicalStorageBuffer64` + `SPV_KHR_physical_storage_buffer` when pointers appear (legal in SPIR-V 1.5). Fallback if needed: `session_options.capability(find_capability(...))` (verified available in the fork). Optionally factor the 3 duplicated option blocks (L48–122/129–198/207–253) into one helper.
- **Tests first**: add a pointer-using test shader in `shaders/test/` with a deliberately layout-hostile struct (unpadded `float3`s, a lone `float` before a `float4`); `spirv-dis` the output and compare `OpMemberDecorate Offset`/`ArrayStride` on the PhysicalStorageBuffer-class struct vs the same struct descriptor-bound — this empirically settles what layout Slang uses for pointer pointees before any Rust depends on it. Then `just test` + review new snapshots; confirm **zero changes to existing snapshots** (proves descriptor path untouched — note: switching padding to reflection-driven offsets should be byte-identical for existing std140/std430 structs; if snapshots churn, the reflected offsets disagree with the old table and each diff needs review).

## Phase 7 — Proof examples ✅ (done 2026-07-18)

Implemented as planned — three one-line shader edits (`StructuredBuffer<T>`/`RWStructuredBuffer<T>` → `LayoutPtr<T, Std430DataLayout>`; reads *and* writes through pointers use plain subscript, no other shader changes) plus the app-side address writes. Zero renderer/codegen changes were needed, confirming the Phase 6 claim. Notes beyond the plan: (1) the generated `pub use super::particle::Particle` re-exports disappeared from `particles_compute.rs`/`particle_render.rs` (the pointee is no longer named in the entry's Rust types), so `examples/particles.rs` now imports `shader_atlas::particle::Particle` directly — cosmetic, the shared module is still `pub`; (2) particles' `StorageBufferFrameStrategy::PingPong` line is deleted entirely — the −1/0 frame offsets moved into `SimParams { particles_in: gpu.device_address_prev(..), particles_out: gpu.device_address(..) }` exactly as `bda_renderer_plumbing.md` specified; (3) sprite_batch's `SpriteBatchParams` construction moved inside the draw closure (the address comes from `Gpu`). Snapshot churn was exactly the predicted 6 (the three shaders' .json/.rs), zero elsewhere.

Verified: check/fmt/lint/test green; `just shaders` idempotent; both examples clean under validation in debug and release; regression sweep (basic_triangle, suzanne, watercolor incl. pipelined compute, gpu_picking) clean; `spirv-val --target-env vulkan1.3` passes on all five regenerated binaries; **visual confirmation via window screenshots** (the real gate — BDA layout bugs are invisible to validation): sprite_batch renders 8192 rotated/textured/tinted sprites correctly, particles shows the rainbow spiral evolving and wrapping across frames, proving the compute ping-pong address path. Hot reload of a migrated shader (comment-level edit) recompiles clean mid-run. One observed limitation, believed pre-existing and not pointer-specific: hot-reloading an edit that *changes the descriptor interface* (e.g. swapping `LayoutPtr` back to `StructuredBuffer` mid-run) produces pipeline-layout-incompatibility validation errors — descriptor sets aren't reallocated on reload; any resource-count-changing edit would do the same.

- **sprite_batch** (graphics): `shaders/source/sprite_batch.shader.slang` — replace `StructuredBuffer<Sprite> sprites` with `LayoutPtr<Sprite, Std430DataLayout> sprites` in the ParameterBlock; app sets `params.sprites = gpu.device_address(&sprite_buffer)` in the per-frame update. Storage buffer created as before, just never descriptor-bound. Rust `Sprite` repr matching the pointer-load layout is enforced automatically by Phase 6 (per-field `offset_of!`/`size_of` asserts, the `pointee_size` cross-check, and the dual-context compatibility panic) — no manual verification step needed.
- **particles** (compute): `particles.compute.slang` — `LayoutPtr<Particle, Std430DataLayout> particlesIn/particlesOut` in the params struct; `particle_render.shader.slang` — same for the vertex-stage read. App supplies addresses respecting the existing ping-pong buffer_frame rotation (the address written into frame N's uniform copy must target the same prev/current buffer copies the descriptor `PingPong` strategy used). Keep the pipelined-compute path exercised.
- Regenerate everything (`just shaders`).

## Verification (overall gate)

- `cargo check --all`, `cargo fmt`, `just lint`, `just test` green.
- All examples run clean under validation (`timeout 3 just dev EXAMPLE`), debug **and** release.
- Feature-specific: MSAA resolve visuals, render-scale blit, egui editor overlay, picking clicks, resize stress, shader hot-reload (edit a `.slang` while `just dev` runs), pipelined compute.
- `spirv-val`/`spirv-dis` on the two migrated shaders.

## Remaining risks

- **Slang pointer struct layout** — ~~may differ from std430~~ **settled empirically 2026-07-16** (Phase 6 Step A, see [vulkan_1_3_migration/slang_pointer_codegen.md](vulkan_1_3_migration/slang_pointer_codegen.md) D2): default `T*` pointee layout is *natural* (C-like, `float4` can land at offset 20), NOT std430, and the pointer's reflected `element_type_layout()` reports natural offsets even when the pointer declares another layout. Resolution: shaders must use `Std430DataLayout` pointers (the builtin `LayoutPtr<T, Std430DataLayout>` alias); reflection hard-errors on bare `T*` (detected via `Type::full_name()`); pointee offsets come from `program_layout.type_layout(pointee_ty, LayoutRules::DefaultStructuredBuffer)`, which matches the emitted SPIR-V byte-for-byte. Under required-std430, dual-context types (`LayoutPtr<Particle, Std430DataLayout>` in one shader, `StructuredBuffer<Particle>` in another) are byte-identical by construction, and Phase 7's proof structs remain trivially safe. Residual watch-item: slang-upgrade drift (default layout, `full_name` format, `DefaultStructuredBuffer` semantics) — pinned by the permanent rspirv offset test.
- **Timeline + swapchain-recreate edge cases** (frame-counter increments on early-return paths; the "advance counters BEFORE present" comment in `draw_frame` shows this area has bitten before): test resize storms.
- **`shader_draw_parameters`** feature bit newly actually-enabled in Phase 0 (previously extension-only): strictly more correct, but watch draw-parameter-using examples.
