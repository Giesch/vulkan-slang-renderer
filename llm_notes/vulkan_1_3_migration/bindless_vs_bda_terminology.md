# Bindless vs. BDA Pointer Trees: Terminology and Reading List

Companion note to [../vulkan_1_3_migration.md](../vulkan_1_3_migration.md). Context: the migration's BDA phases (5–7) lead toward the "root address in a push constant" pattern; this note pins down how that pattern relates to "bindless" and where to read more.

The two are **related but not the same** — and the distinction is worth being precise about, because the terms get blurred constantly.

## Strictly speaking

**"Bindless" originally and most precisely means descriptor indexing**: instead of binding a handful of specific resources before each draw, you bind one giant array of descriptors (thousands of textures, say) once per frame, and shaders pick from it with a runtime index — `textures[material.albedoIndex]`. The "binding" step is what disappears, hence the name. In Vulkan this is `VK_EXT_descriptor_indexing` (core in 1.2): runtime descriptor arrays, update-after-bind, `nonuniformEXT` indexing. In D3D12 land the same concept is descriptor heaps and, since Shader Model 6.6, `ResourceDescriptorHeap` ("dynamic resources").

**The push-constant pointer tree is a different mechanism**: buffer device address. There's no descriptor and no table — the shader dereferences a raw 64-bit GPU virtual address. It doesn't index into a bound array; it bypasses the descriptor system entirely.

So: same *goal* (decouple resource access from CPU-side bind calls, let data reference other data), different *machinery* (a descriptor table you index vs. raw pointers you chase).

## Why they're constantly conflated

A fully "bindless renderer" in practice uses **both, split by resource type**, because Vulkan forces the split: buffers can be raw pointers (BDA), but **textures and samplers cannot** — there is no "image device address"; sampled images fundamentally require descriptors. So the standard modern architecture is exactly:

- **Buffers** → BDA pointer trees, root address in a push constant,
- **Textures** → one big bindless descriptor array, bound once, indexed by material data — where the *indices themselves* typically live in BDA-reachable buffers.

People call that whole composite "bindless rendering," so BDA gets swept under the label. You'll also see "bindless buffers" used specifically for the BDA half. When reading, check which mechanism an article actually means — an article on "bindless textures" (descriptor indexing) won't tell you much about pointer layout, and vice versa.

For this renderer's roadmap, the two are also separable in time: Phases 5–7 are pure BDA and touch nothing about textures; a bindless texture array would be a separate future project (and would replace the per-pipeline combined-image-sampler bindings the same way BDA replaces buffer bindings).

## Search terms, organized by concept

- **The pointer mechanism:** `buffer device address`, `BDA`, `VK_KHR_buffer_device_address`, `GLSL_EXT_buffer_reference`, `PhysicalStorageBuffer64`, `Slang pointers`
- **Bindless proper:** `descriptor indexing`, `VK_EXT_descriptor_indexing`, `runtime descriptor array`, `update-after-bind`, `nonuniformEXT`, `bindless textures`; D3D12-side: `SM6.6 dynamic resources`, `ResourceDescriptorHeap`, `bindless root signature`
- **The push-constant-root idiom specifically:** rarely has its own name; it shows up inside articles as "push constant with buffer reference/address" (D3D12 calls the analogous slot "root constants")
- **Where it all leads:** `GPU-driven rendering`, `multi-draw indirect` (MDI), `vertex pulling` / `programmable vertex fetch`, `VK_EXT_descriptor_buffer` (the newer descriptor model that dissolves descriptor sets themselves)

## Reading list

- **vkguide.dev** — the GPU-driven rendering chapters; the modern edition uses BDA for mesh/draw data with push constants carrying addresses. The most directly relevant walkthrough to this renderer's situation.
- **Sascha Willems' Vulkan samples** (github.com/SaschaWillems/Vulkan) — small, focused samples named `bufferdeviceaddress`, `descriptorindexing`, `descriptorbuffer`; good for seeing each mechanism in isolation.
- **Hans-Kristian Arntzen (Themaister)** — themaister.net; several deep pieces on Vulkan descriptor models, why descriptors are shaped the way they are, and descriptor-buffer/bindless trade-offs. Best conceptual grounding of the lot.
- **Wicked Engine devblog (Turánszki János)** — a clear "Bindless Descriptors" writeup covering the texture-array half across Vulkan/DX12.
- **Arseny Kapoulkine (zeux)** — the *niagara* renderer YouTube series ("writing a Vulkan renderer from scratch") and his "Writing an efficient Vulkan renderer" essay; strong on GPU-driven rendering and descriptor strategy trade-offs.
- **Sebastian Aaltonen's REAC talks** (e.g. the HypeHype rendering architecture talk) — the clearest articulation of the "one root struct, everything reachable by pointer" philosophy in production.
- **id Tech / DOOM Eternal SIGGRAPH talks** — the canonical shipped example of a fully bindless engine, for the endgame.
- **Slang user guide, pointers section** (shader-slang.com) — the authoritative reference for what `T*` means in shaders, address spaces, and alignment rules — directly feeds Phase 6.

If you read just two: the vkguide GPU-driven chapters for the hands-on version of the push-constant-root pattern, and Themaister's descriptor-model posts for the conceptual map that makes the terminology click into place.
