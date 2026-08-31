# Coordinates

## Convention

- World and view space are right-handed, Y-up.
- CPU projections come from `glam::camera::{rh,lh}::proj::directx`.
  Clip-space Y is up. NDC depth is `[0, 1]`.
- Shaders apply the projection unchanged. `mltrs::MVPMatrices` and
  `mltrs::Projection` contain no Y flip.
- The renderer records every viewport with negative height
  (`Renderer::flipped_viewport`). The rasterizer maps clip-space +Y to the top
  row of the framebuffer.
- Front faces are counter-clockwise in Y-up space. `RasterState` defaults to
  `CullMode::Back`. Meshes from OBJ and glTF render as authored. GX meshes are
  clockwise; `convert_link` reverses their triangles.

## Screen-space 2D

Use Y-up pixel coordinates with the origin at the bottom-left:

```rust
directx::orthographic(0.0, width, 0.0, height, 0.0, -1.0)
```

## Fullscreen shaders

`mltrs::fullscreenPosition` returns two coordinates:

| field        | range     | Y    | use                                 |
|--------------|-----------|------|-------------------------------------|
| `svPosition` | `[-1, 1]` | up   | `SV_Position`                       |
| `texCoord`   | `[0, 1]`  | down | sampling an image; row 0 is the top |

`svPosition.xy` is NDC. A fragment shader that needs NDC must receive
`svPosition.xy` as a varying; the fragment-stage `SV_Position` input is
framebuffer coordinates, not NDC. Multiply NDC by the inverse of `proj * view`
to reconstruct a ray.

## Geometry emitted in clip space

Shaders that write clip coordinates directly (no projection) must emit
counter-clockwise triangles in Y-up space.

## Framebuffer coordinates

`SV_Position` in a fragment shader, mouse positions, and picking readback are
all framebuffer coordinates: Y-down, row 0 at the top. The viewport flip does
not change them.
