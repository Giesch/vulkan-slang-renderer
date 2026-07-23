# BDA Renderer Plumbing: The Phase 5 Design

Companion note to [../vulkan_1_3_migration.md](../vulkan_1_3_migration.md). **Status: Phase 5 implemented as designed (2026-07-14).** The push-constant offset fix originally bundled into this phase landed separately, just before it (see below). For how this BDA direction relates to "bindless" and the broader reading list, see [bindless_vs_bda_terminology.md](bindless_vs_bda_terminology.md).

Phase 5 gives every storage buffer a queryable GPU pointer and exposes it to apps through the `Gpu` per-frame view. No shader, codegen, or descriptor changes — those are Phases 6 and 7. This phase is behaviorally invisible on its own.

## The concept

`vkGetBufferDeviceAddress` returns a `VkDeviceAddress` (a `u64`): the GPU-visible address of a buffer's first byte. A shader compiled with `PhysicalStorageBuffer64` addressing (what Slang emits for `T*` fields) can load through that address directly — no descriptor binding involved.

The key mental shift: **an address is data, not a resource.** A `T*` field in a Slang parameter block is just 8 bytes of uniform data. It consumes no descriptor slot, appears in no descriptor set layout, and needs no `update_descriptor_sets` call — the app writes it per-frame exactly like any other uniform field (`params.sprites = gpu.device_address(&sprite_buffer)`). That's why this phase touches only buffer creation and the `Gpu` view, and why uniform buffers themselves stay descriptor-bound: the addresses live *in* uniform data, so something descriptor-bound has to carry them.

## What's already in place (banked in Phase 0)

- `Vulkan12Features::buffer_device_address(true)` is enabled unconditionally (`create_logical_device`, renderer.rs:3096) and checked at physical-device selection (renderer.rs:2849–2850).
- VMA is created with `AllocatorCreateFlags::BUFFER_DEVICE_ADDRESS` unconditionally (renderer.rs:296) — required so VMA allocates memory with `VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT` when a buffer carries the usage flag.
- Nothing in src/ yet sets `BufferUsageFlags::SHADER_DEVICE_ADDRESS` or calls `get_buffer_device_address` — this phase is a greenfield addition.

BDA works on any memory type, including the persistently-mapped host-visible allocations storage buffers use today (no ReBAR or device-local requirement).

## The design

Five changes; (e) is already done.

### (a) Usage flag + address query at creation

`create_storage_buffer` (renderer.rs:803–833) loops `BUFFER_FRAME_COUNT` times through the shared `create_memory_buffer` helper (renderer.rs:3536–3552). Change the usage to `STORAGE_BUFFER | SHADER_DEVICE_ADDRESS` and, right after each copy is created, query its address:

```rust
let device_address = unsafe {
    self.device.get_buffer_device_address(
        &vk::BufferDeviceAddressInfo::default().buffer(buffer),
    )
};
debug_assert_ne!(device_address, 0);
```

Applied to **all** storage buffers, no opt-in flag — the cost is one usage bit and one query at creation. The other `create_memory_buffer` callers (uniform, staging, vertex, index buffers) are untouched.

### (b) Cache on `RawStorageBuffer`

`RawStorageBuffer` (src/renderer/storage_buffer.rs:22–27) gains `device_address: vk::DeviceAddress`, one per frame copy, set once at creation. Lifetime story: `StorageBufferStorage` slots are append-only (`take` leaves `None`; indices are never reused in place), so a cached address is valid for exactly the buffer's life — created in `create_storage_buffer`, dead after `destroy_storage_buffer` (renderer.rs:852–857) or renderer teardown. No invalidation logic exists or is needed.

### (c) Storage accessor

`StorageBufferStorage::get_device_address_for_frame<T>(&self, handle: &StorageBufferHandle<T>, frame: usize) -> vk::DeviceAddress`, mirroring `get_mapped_mem_for_frame` (storage_buffer.rs:61–68).

### (d) App-facing API

On `Gpu<'f>` (renderer.rs:5002–5044), which already resolves the current `buffer_frame` for `write_uniform`/`write_storage`:

- `gpu.device_address<T>(&StorageBufferHandle<T>) -> u64` — the current frame's copy. Matches what the descriptor path binds under `StorageBufferFrameStrategy::Standard` (and PingPong's non-first buffers).
- `gpu.device_address_prev<T>(&StorageBufferHandle<T>) -> u64` — `(buffer_frame + BUFFER_FRAME_COUNT − 1) % BUFFER_FRAME_COUNT`. This exactly mirrors the descriptor `PingPong` strategy, which binds the **first storage buffer in layout order** at frame offset −1 with `rem_euclid` wraparound (renderer.rs:3741–3747). Phase 7's `particlesIn` pointer is this; `particlesOut` is plain `device_address`.

On `Renderer`, for init paths (companion to `write_storage_all_frames`, renderer.rs:835–843):

- `renderer.device_addresses<T>(&StorageBufferHandle<T>) -> [u64; BUFFER_FRAME_COUNT]`, indexed by buffer frame.

Return `u64` at the API boundary (`vk::DeviceAddress` is an alias for it, and Phase 6 codegen emits pointer fields as `pub name: u64`), keep `vk::DeviceAddress` internally.

### (e) Push-constant offset fix ✅ (done 2026-07-14, landed ahead of the phase)

`impl shaders::json::ReflectedPushConstantRange::to_vk()` (renderer.rs:4981–4988) set `.offset(self.size)` instead of `.offset(self.offset)` — the struct's `offset` field (src/shaders/json/pipeline_builders.rs:34–40) was ignored entirely. Dormant while no shader declares push constants and every range starts at 0, but it would have produced garbage layouts the moment a nonzero-offset or multi-range case appeared. Fixed by hand to `.offset(self.offset)`; nothing left to do in this phase.

## Verification

- `cargo check --all`, `cargo fmt`, `just lint`, `just test` — expect **zero snapshot churn** (no codegen is touched; any snapshot diff means something leaked out of scope).
- Representative examples clean under validation, debug **and** release: `basic_triangle`, `sprite_batch`, `particles`, `watercolor`, `suzanne`, `gpu_picking`.
- Since no shader consumes an address yet, there is nothing visual to check — validation-clean runs plus the `debug_assert_ne!(addr, 0)` are the whole gate. The API's first real exercise is Phase 7.

## Rules of thumb

- **Addresses are per-frame-copy.** A `StorageBufferHandle` names `BUFFER_FRAME_COUNT` distinct buffers with distinct addresses. The address written into frame N's uniform copy must target the same buffer copy the descriptor path would have bound for frame N — that's the entire reason `device_address_prev` exists rather than leaving ping-pong math to apps.
- **BDA failures are silent.** A wrong address or wrong pointee layout produces no validation error, just wrong data (or a device-lost at worst). This phase keeps that risk near zero by changing no shader-visible behavior; layout safety is Phase 6's job (reflection-driven offsets + per-field `offset_of!` asserts).
- **No invalidation, by construction.** Cache the address once at creation; never re-query. If storage slots ever become reusable, that assumption must be revisited — today they aren't.
- **Uniform buffers stay descriptor-bound.** Don't add the usage flag or an address accessor for them; a pointer-to-uniform is a design smell here (the parameter block *is* the uniform).

## Search terms and reading

- `VK_KHR_buffer_device_address`, `vkGetBufferDeviceAddress`, `VkBufferDeviceAddressInfo`, `SPV_KHR_physical_storage_buffer`, `PhysicalStorageBuffer64`
- **Vulkan spec: `VkBufferUsageFlagBits`** — `SHADER_DEVICE_ADDRESS_BIT` interaction with `VK_MEMORY_ALLOCATE_DEVICE_ADDRESS_BIT` (VMA handles the allocate-flag side when its allocator flag is set).
- **Slang user guide, "Pointers"** — how `T*` parameter-block fields lower to physical storage buffer loads.
- [bindless_vs_bda_terminology.md](bindless_vs_bda_terminology.md) — where this sits relative to "bindless", and the longer reading list.
