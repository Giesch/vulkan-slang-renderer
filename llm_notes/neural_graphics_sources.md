# Neural graphics sources

Reference links for four topics: Vulkan cooperative vectors, neural radiance
caching on constrained hardware, radiance cascades, and SVGF denoising.

All links verified 2026-08-23. Three return HTTP 403 to command-line tools: the
ACM, Wiley, and Path of Exile entries. All three open normally in a browser. The
block is bot protection, not a dead link.

---

## 1. Vulkan cooperative vectors and neural shading

Cooperative vectors give a shader invocation a matrix-vector multiply on
tensor-core hardware. This is the mechanism for evaluating a small MLP inside
shader code.

- [VK_NV_cooperative_vector — proposal](https://docs.vulkan.org/features/latest/features/proposals/VK_NV_cooperative_vector.html) — The design document. States that supported component types are device-dependent and must be queried at runtime.
- [VK_NV_cooperative_vector — reference pages](https://registry.khronos.org/vulkan/specs/latest/man/html/VK_NV_cooperative_vector.html) — The Khronos registry entry with the API surface.
- [GLSL_NV_cooperative_vector](https://github.com/KhronosGroup/GLSL/blob/main/extensions/nv/GLSL_NV_cooperative_vector.txt) — The raw GLSL language binding for the extension.
- [VK_KHR_cooperative_matrix — proposal](https://docs.vulkan.org/features/latest/features/proposals/VK_KHR_cooperative_matrix.html) — The cross-vendor matrix extension. Uses a subgroup-cooperative model that needs uniform control flow, unlike cooperative vectors.
- [RTXNS — NVIDIA Neural Shading SDK](https://github.com/NVIDIA-RTX/RTXNS) — Sample code and framework built on Slang. Requires an RTX 20-series GPU or newer and Vulkan driver 572.16 or newer.
- [Neural Shading Course, Part 7](https://www.youtube.com/watch?v=o-m5hP_9yLE) — Talk on reaching the hardware fast path with cooperative vectors.
- [Machine Learning Acceleration in Vulkan with Cooperative Matrices](https://developer.nvidia.com/blog/machine-learning-acceleration-vulkan-cooperative-matrices/) — NVIDIA's introduction to the matrix path.
- [Hardware Accelerated Neural Block Texture Compression with Cooperative Vectors](https://arxiv.org/pdf/2506.06040) — A worked application of cooperative vectors to texture decompression.

> **Verify per device:** `VK_KHR_cooperative_matrix` is cross-vendor on paper.
> [Phoronix reports AMD and Intel drivers advertising the extension without the
> hardware to back it](https://www.phoronix.com/news/Vulkan-1.3.300-Released).
> Query the device rather than trusting the flag.

---

## 2. Neural radiance caching on constrained hardware

A radiance cache stores incoming light at chosen points and reuses it instead of
tracing further bounces. The cache does not depend on how the query points were
found, so ray marching can feed it in place of hardware ray tracing.

- [Neural Radiance Cache Implementation on Mobile GPU](https://dl.acm.org/doi/10.1145/3757376.3771399) — SIGGRAPH Asia 2025. A fused MLP written entirely in compute shaders, with training decoupled from inference. Removes the tensor-core dependency.
- [Locality-aware Training for Online Radiance Caching in Path Tracing on Mobile Platforms](https://onlinelibrary.wiley.com/doi/10.1111/cgf.70537) — Computer Graphics Forum. Covers the online-training half of the problem.
- [How Arm Is Bringing Neural Graphics to Mobile](https://blog.siggraph.org/2026/06/how-arm-is-bringing-neural-graphics-to-mobile-at-siggraph-2026.html/) — SIGGRAPH 2026. Vendor perspective on neural rendering under a mobile power budget.

> Both papers still assume some ray tracing. Pairing a neural radiance cache with
> SDF ray marching on hardware without ray tracing appears unexplored.

---

## 3. Radiance cascades

A global illumination method by Alexander Sannikov at Grinding Gear Games, first
presented at ExileCon 2023 and shipped in Path of Exile 2. Cost is independent of
scene complexity and light count. The 2D form produces noiseless output without
temporal accumulation.

The core idea is the penumbra hypothesis: nearby light needs high spatial
resolution, distant light needs high angular resolution. Each cascade level
doubles the ray count and halves the probe density.

- [radiance.wiki](https://radiance.wiki/) — The community hub. Indexes the original paper, tutorials, and implementations in GLSL, HLSL, WGSL, Rust, C++, and CUDA. Start here.
- [radiance-wiki source](https://github.com/OrionReed/radiance-wiki) — The wiki content and contribution guide.
- [Radiance Cascades Discord](https://discord.gg/CdYqehej2a) — Active implementation discussion and accumulated fixes.
- [80.lv — Radiance Cascades](https://80.lv/articles/radiance-cascades-new-approach-to-calculating-global-illumination) — The accessible overview. Reports the 2D setup at 0.3 ms on a GTX 970.
- [Path of Exile forum thread](https://www.pathofexile.com/forum/view-thread/3448733) — Grinding Gear Games' own writeup.
- [GM Shaders: Radiance Cascades](https://mini.gmshaders.com/p/radiance-cascades) — Tutorial by Xor and Sannikov.
- [tmpvar interactive demo](https://tmpvar.com/poc/radiance-cascades/) — Browser demo for building intuition.
- [Holographic Radiance Cascades for 2D Global Illumination](https://arxiv.org/pdf/2505.02041) — A research extension of the original method.
- [radiance-cascades-urp](https://github.com/n01r1r/radiance-cascades-urp) — Screen-space implementation for Unity URP.

> **Search trap:** [arXiv 2408.14425, "Radiance Cascades: A Novel High-Resolution
> Formal Solution for Multidimensional Non-LTE Radiative Transfer"](https://arxiv.org/abs/2408.14425)
> is astrophysics, not graphics. It covers solar radiative transfer and ranks
> high in searches for this technique.

---

## 4. SVGF and denoising

Spatiotemporal Variance-Guided Filtering, by Schied et al., HPG 2017. Temporal
accumulation supplies history. Luminance variance estimates drive a hierarchical
image-space wavelet filter. The hierarchy separates noise from detail across
scales.

The reported result sets the baseline any denoiser must beat: 1920x1080
reconstructed in 10 ms from 1 sample per pixel, against a 2048 spp reference.

- [SVGF — NVIDIA Research](https://research.nvidia.com/publication/2017-07_spatiotemporal-variance-guided-filtering-real-time-reconstruction-path-traced) — The canonical paper page.
- [SVGF — KIT project page](https://cg.ivd.kit.edu/english/svgf.php) — Academic mirror with supplementary material.
- [CUDA-Path-Tracer-Denoising](https://github.com/ZheyuanXie/CUDA-Path-Tracer-Denoising) — Well-documented implementation with diagrams.
- [jacquespillet/SVGF](https://github.com/jacquespillet/SVGF) — A second reference implementation.
- [vkdt SVGF module](https://jo.dreggn.org/vkdt/src/pipe/modules/svgf/readme.html) — Vulkan implementation inside a working renderer.

> Follow-ups worth knowing: A-SVGF adds adaptive temporal reprojection for fast
> motion. NVIDIA NRD (Real-Time Denoisers) is the production library that
> superseded raw SVGF in most engines.

---

## 5. Local hardware capability

Measured on this machine with `vulkaninfo` from Vulkan SDK 1.4.328.0.

| Property | Value |
|---|---|
| Discrete GPU | NVIDIA GeForce RTX 3070 Ti Laptop (GA104, Ampere) |
| VRAM | 8 GB |
| Driver | 580.173.02 |
| Vulkan API | 1.4.312 |
| `VK_NV_cooperative_vector` | Present, revision 4 |

RTXNS requirements are met: the GPU exceeds the RTX 20-series minimum, the driver
exceeds 572.16, the SDK exceeds 1.3.296.0, and `slangc` ships with the SDK.

Supporting extensions present on the NVIDIA device:

- `VK_KHR_buffer_device_address`
- `VK_KHR_shader_float16_int8`
- `VK_KHR_shader_integer_dot_product`
- `VK_KHR_cooperative_matrix`, `VK_NV_cooperative_matrix`, `VK_NV_cooperative_matrix2`
- `VK_KHR_ray_query`, `VK_KHR_ray_tracing_pipeline`, `VK_KHR_ray_tracing_position_fetch`, `VK_KHR_ray_tracing_maintenance1`, `VK_NV_ray_tracing_invocation_reorder`

### Open question

`vulkaninfo` does not enumerate `VkPhysicalDeviceCooperativeVectorPropertiesNV`.
Which component types get hardware acceleration is unconfirmed. Call
`vkGetPhysicalDeviceCooperativeVectorPropertiesNV` to find out.

Ampere tensor cores handle FP16, BF16, and INT8. FP8 arrived with Ada. Expect
FP8 cooperative-vector paths to be unsupported or emulated on this GPU.

### Device selection

Vulkan enumerates three devices here, and the Intel iGPU comes first:

```
[0] Intel Iris Xe Graphics (ADL GT2)     <- no VK_NV_* extensions
[1] NVIDIA GeForce RTX 3070 Ti Laptop    <- the target
[2] llvmpipe (software rasterizer)
```

Sample code that takes `physicalDevices[0]` lands on the Intel part and fails the
cooperative-vector check. Check device selection before believing a report that
the extension is missing.
