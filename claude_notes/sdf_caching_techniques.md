# SDF Caching Techniques: MSDF Fonts vs 3D SDF Grids

## The Core Comparison

Both MSDF font rendering and 3D SDF grid techniques share a conceptual foundation: caching SDF evaluations to a grid structure rather than computing them on-the-fly.

- **Font MSDF**: Pre-bake SDF to texture, add multi-channel trick because text legibility demands sharp corners
- **3D SDF grids**: Cache SDF evaluations per-frame with LOD, accept smoothed corners as an acceptable tradeoff for performance

## What the Papers Describe

The [JCGT paper "Ray Tracing of Signed Distance Function Grids"](https://jcgt.org/published/0011/03/06/) by Hansson-Söderlund, Evans, and Akenine-Möller focuses on efficiently ray-tracing against cached SDF grids using trilinear interpolation and multi-resolution hierarchies. The [Geometry Clipmaps paper](https://hhoppe.com/geomclipmap.pdf) by Losasso and Hoppe is about terrain heightmaps specifically, using nested grids with higher resolution near the viewer.

## The Corner-Rounding Limitation

The corner-rounding limitation is real and acknowledged in the literature:

> "The effects of the low resolution are seen on the accuracy of the approximation. You can see that the sharp corners of the original shape have disappeared and been replaced with rounded points."

And regarding trilinear interpolation:

> "It can lead to over-smoothing of high-frequency details in data with sharp variations, potentially blurring fine structures."

## Multi-Channel SDFs: Mentioned but Not Used in 3D

One source notes:

> "Sharp corners can be preserved by using multiple channels of SDFs, however, complex to implement and computationally expensive in the contact detection."

The 3D case prioritizes rendering speed over geometric fidelity. For organic shapes, painterly styles (like Dreams), or effects like ambient occlusion, rounded corners are often visually acceptable. Fonts don't have that luxury.

## Sources

- [NVIDIA Research - Ray Tracing of Signed Distance Function Grids](https://research.nvidia.com/publication/2022-09_ray-tracing-signed-distance-function-grids)
- [Geometry Clipmaps Project Page](https://hhoppe.com/proj/geomclipmap/)
- [NVIDIA GPU Gems 2 - Terrain Rendering Using GPU-Based Geometry Clipmaps](https://developer.nvidia.com/gpugems/gpugems2/part-i-geometric-complexity/chapter-2-terrain-rendering-using-gpu-based-geometry)
- [Alex Evans SIGGRAPH 2015 - Learning from Failure](https://www.mediamolecule.com/blog/article/siggraph_2015)
