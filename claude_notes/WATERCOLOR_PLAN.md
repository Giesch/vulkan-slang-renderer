# Watercolor Painting Application — Implementation Plan

## Context

Implement an interactive watercolor painting application based on Curtis et al. "Computer-Generated Watercolor." The user paints brushstrokes with the mouse; a shallow-water fluid simulation drives pigment transport across the canvas; Kubelka-Munk compositing renders the final image.

The renderer currently lacks RWTexture2D support, so Phase 1 adds that infrastructure before building the application.

## Phase 1: Add RWTexture2D (Storage Image) Support

~140 lines across ~10 files, following existing patterns for Texture2D and StructuredBuffer.

### 1.1 Reflection — recognize RWTexture2D from Slang

**`src/shaders/reflection/parameters.rs`** (~5 lines)
- In the `slang_base_shape` match (line 264), when base shape is `SlangTexture2d`, check resource access — if `ReadWrite`, emit `ResourceShape::RWTexture2D` instead of `Texture2D`

**`src/shaders/json/parameters.rs`** (1 line)
- Add `RWTexture2D` variant to `ResourceShape` enum

### 1.2 Pipeline layout — map MutableTexture to StorageImage

**`src/shaders/reflection/pipeline_layout.rs`** (1 line)
- Change `MutableTeture => todo!()` to `MutableTeture => Self::StorageImage`

**`src/shaders/json/pipeline_builders.rs`** (1 line)
- Add `StorageImage` variant to `ReflectedBindingType` enum

### 1.3 Layout bindings — route StorageImage through to Vulkan

**`src/shaders/json.rs`** (~5 lines)
- In `layout_bindings_from_pipeline_layout`, add arm for `StorageImage` → `LayoutDescription::StorageImage`

**`src/renderer.rs`** (~10 lines)
- Add `LayoutDescription::StorageImage(StorageImageDescription)` variant
- Add `StorageImageDescription` struct (layout, binding, descriptor_count)
- Add `StorageImage => vk::DescriptorType::STORAGE_IMAGE` in `vk_descriptor_type()`

### 1.4 Code generation — thread StorageTexture2D through templates

**`src/shaders/build_tasks.rs`** (~25 lines)
- Add `StorageTexture2D` to `RequiredResourceType` enum
- Handle `ResourceShape::RWTexture2D` in `required_resource()` → map to `&'a StorageTextureHandle`
- Add `resources_storage_texture_fields` vec, thread through `GeneratedComputeShaderImpl` (and graphics equivalent)
- Handle `RWTexture2D` in `gather_struct_defs()` (return `None`, no uniform data)

**`templates/shader_compute_entry.rs.askama`** (~6 lines)
- Add `storage_texture_handles` vec in `pipeline_config()`, pass to `ComputePipelineConfig`

**`templates/shader_atlas_entry.rs.askama`** (~6 lines)
- Same for graphics pipeline config builder

### 1.5 Renderer — StorageTexture types, creation, descriptor binding

**`src/renderer.rs`** (or new `src/renderer/storage_texture.rs`) (~80 lines)
- `StorageTextureHandle` — index-based handle type (like `TextureHandle`)
- `StorageTexture` — holds `vk::Image`, `vk::DeviceMemory`, `vk::ImageView` (no sampler)
- `Renderer::create_storage_texture(width, height, format) -> StorageTextureHandle`
  - Usage flags: `STORAGE | SAMPLED` (SAMPLED so it can also be read as regular texture for display)
  - Layout: transition to `GENERAL` via one-shot command buffer
  - Format: parameterized (`R32_SFLOAT`, `R32G32_SFLOAT`, `R32G32B32A32_SFLOAT`)
- `Renderer::storage_texture_as_sampled(&StorageTextureHandle) -> TextureHandle`
  - Creates a read-only `TextureHandle` aliasing the same image (adds sampler + image view for `COMBINED_IMAGE_SAMPLER`)
  - Image stays in `GENERAL` layout (valid for both storage and sampled access)
- Add `storage_images: u32` to `DescriptorCounts`, update pool size calculation
- Add `StorageImage` arm in `create_descriptor_sets()` — write descriptor with `STORAGE_IMAGE` type, `GENERAL` layout, no sampler
- Add `storage_texture_handles: Vec<StorageTextureHandle>` to `ComputePipelineConfig` (and graphics equivalent)
- Add `UNDEFINED → GENERAL` image layout transition case

### 1.6 Verification

- Create a test compute shader that writes to `RWTexture2D<float4>`
- Run `just shaders` — verify generated code includes `StorageTextureHandle`
- Run `just test` — accept new snapshots
- Minimal example: compute writes gradient pattern to storage texture, fullscreen quad displays it

---

## Phase 2: Basic Painting App (No Simulation)

Interactive painting with mouse-driven brush strokes. No fluid simulation — just stamp brush footprints onto a canvas.

### 2.1 Paper Height Map

Generate a static paper texture at setup time (once):
- Layered noise (Perlin/simplex, multiple octaves) → `R32F` storage texture, values in [0, 1]
- Peaks (h≈1) = paper fiber ridges, valleys (h≈0) = hollows between fibers
- Derive fluid capacity: `c = h * (c_max - c_min) + c_min` → separate `R32F` texture
- Can generate on CPU and upload, or via a one-shot compute shader
- Display shader uses this to show subtle paper texture behind paint from the start

### 2.2 Shaders

**`shaders/source/paint_brush.compute.slang`** — Stamps a circular brush at a given position
- Reads: brush position, radius, opacity, color (via uniform/parameter block)
- Writes: `RWTexture2D<float4>` canvas
- Gaussian/smoothstep falloff from center
- Alpha-blend over existing canvas content
- Workgroup: `[16, 16, 1]`

**`shaders/source/paint_display.shader.slang`** — Fullscreen triangle that displays the canvas
- Imports `fullscreen_triangle.slang` for vertex positions (3 vertices, single oversized triangle)
- Rendered via `draw_vertex_count(pipeline, 3, ...)`
- Reads canvas as `Texture2D<float4>` (sampled view of the storage texture)
- Composites over paper-white background using canvas alpha
- Could also overlay a subtle paper texture from the paper height field

### 2.3 Example app

**`examples/watercolor.rs`**

```
struct Watercolor {
    // Textures
    canvas: StorageTextureHandle,        // RGBA32F — paint accumulates here
    canvas_sampled: TextureHandle,       // sampled view of canvas for display
    paper_height: StorageTextureHandle,  // R32F — static paper texture

    // Pipelines
    brush_pipeline: ComputePipelineHandle,
    display_pipeline: PipelineHandle<DrawVertexCount>,

    // Buffers
    brush_params: UniformBufferHandle<BrushParams>,
    display_params: UniformBufferHandle<DisplayParams>,

    // Input state
    painting: bool,
    mouse_pos: Vec2,
    prev_mouse_pos: Vec2,
    stroke_points: Vec<Vec2>,     // interpolated points for this frame

    // Brush settings
    brush_color: Vec4,
    brush_radius: f32,
    brush_opacity: f32,
}
```

Key behaviors:
- **`input()`**: Track mouse position. On `MouseDown(Left)`, start painting. On `MouseMotion` while painting, interpolate between previous and current position (spacing ~1/3 brush radius) to avoid gaps. On `MouseUp(Left)`, stop painting.
- **`draw()`**: For each accumulated stroke point, dispatch brush compute shader. Memory barrier (compute→fragment). Draw fullscreen display triangle. Clear stroke points.

### 2.4 Verification

- `just shaders && cargo check --all`
- `just dev watercolor` — paint with mouse, verify smooth strokes, soft edges, alpha blending

---

## Phase 3: Watercolor Simulation

Incrementally add the Curtis et al. simulation. Each sub-phase is independently testable.

### Simulation textures (all `RWTexture2D`, staggered MAC grid)

| Texture | Format | Purpose | Ping-pong? |
|---------|--------|---------|------------|
| `velocity_u_a/b` | `R32F` | Horizontal velocity at (i+0.5, j) | Yes |
| `velocity_v_a/b` | `R32F` | Vertical velocity at (i, j+0.5) | Yes |
| `pressure_a/b` | `R32F` | Pressure at cell centers | Yes |
| `pigment_a/b` | `RGBA32F` | Pigment concentration in water (4 species) | Yes |
| `deposit` | `RGBA32F` | Pigment deposited on paper (4 species) | No |
| `saturation` | `R32F` | Capillary water saturation | No |
| `wet_mask` | `R32F` | Wet-area mask (1=wet, 0=dry) | No |
| `paper_height` | `R32F` | Paper texture height field (static, from Phase 2) | No |
| `capacity` | `R32F` | Fluid-holding capacity (static, derived from h) | No |
| `correction` | `R32F` | Divergence correction (temp, for RelaxDivergence) | No |

Ping-pong textures: create two pipeline objects per step with input/output swapped. Alternate which pipeline is dispatched each sub-step.

### 3a: Fluid flow (velocity + pressure)

**Shaders:**
- `wc_apply_slope.compute.slang` — Subtract paper height gradient from velocity
- `wc_update_velocity.compute.slang` — Forward Euler: advection + viscosity + pressure gradient + drag
- `wc_compute_divergence.compute.slang` — Compute per-cell divergence correction (Jacobi step 1)
- `wc_apply_divergence.compute.slang` — Apply correction gradient to velocities (Jacobi step 2)
- `wc_enforce_boundary.compute.slang` — Zero velocity outside wet mask
- `wc_flow_outward.compute.slang` — Edge darkening: reduce pressure near wet-area boundary
- `watercolor_common.slang` — Shared structs (SimParams, grid helpers, bounds checking)

**Brush changes:** Instead of writing color directly to canvas, brush now writes to wet_mask (M=1) and adds initial pressure (creates outward flow from brush stroke).

**Per-frame dispatch:**
```
dispatch apply_slope
barrier
for sub_step in 0..NUM_SUBSTEPS:
    dispatch update_velocity (ping-pong u,v)
    barrier
dispatch enforce_boundary
barrier
for iter in 0..NUM_JACOBI_ITERS:
    dispatch compute_divergence → correction texture
    barrier
    dispatch apply_divergence (ping-pong pressure, reads correction)
    barrier
dispatch enforce_boundary
barrier
dispatch flow_outward
barrier
```

**Verify:** Paint a blob. Observe fluid spreading outward. Debug-visualize pressure or velocity magnitude as grayscale.

### 3b: Pigment transport

**Pigment palette:** Define the full pigment data model upfront. Up to 4 pigment species (packed into RGBA channels). Each pigment has:
```
struct PigmentProperties {
    float density;        // ρ — heavier pigments settle faster (used in 3c)
    float stainingPower;  // ω — resistance to lifting once deposited (used in 3c)
    float granulation;    // γ — paper texture interaction (used in 3c)
    float3 K;             // absorption coefficients per RGB channel (used in 3e)
    float3 S;             // scattering coefficients per RGB channel (used in 3e)
}
```
Pass as a uniform buffer. The user selects a pigment from the palette rather than an arbitrary color. Start with a small preset palette (e.g., 4 pigments matching the paper's examples). K and S are derived from the pigment's appearance over white and black backgrounds per Section 5.1 of the paper.

**Shaders:**
- `wc_move_pigment.compute.slang` — Gather-based upwind advection by velocity field

**Brush changes:** Brush now deposits initial pigment concentration (g) into the selected pigment's channel, in addition to pressure and wet mask. Phase 2's direct-color canvas is replaced by pigment-based rendering.

**Per-frame dispatch** (after fluid flow):
```
for sub_step in 0..NUM_SUBSTEPS:
    dispatch move_pigment (ping-pong pigment)
    barrier
```

**Verify:** Paint a colored blob. Pigment should flow with the water, spreading and diluting.

### 3c: Pigment deposition

**Shaders:**
- `wc_transfer_pigment.compute.slang` — Local adsorption/desorption between water (g) and paper (d). Single dispatch, no ping-pong.

**Per-frame dispatch** (after pigment transport):
```
dispatch transfer_pigment
barrier
```

**Verify:** Pigment gradually deposits on paper. Areas where water sits longer accumulate more. Deposited pigment remains after water recedes. Granulation visible (pigment in paper valleys).

### 3d: Capillary flow / backruns

**Shaders:**
- `wc_capillary_flow.compute.slang` — Absorption + diffusion through paper pores + wet-area expansion

**Per-frame dispatch** (after transfer pigment):
```
dispatch capillary_flow
barrier
```

**Verify:** Water seeps beyond wet-area edges through paper pores. Backrun branching patterns appear at drying edges.

### 3e: Kubelka-Munk rendering

**Modify `paint_display.shader.slang`:**
- Read deposited pigment texture (d)
- Per-pigment K (absorption) and S (scattering) coefficients from a uniform table
- Composite pigment layers over paper-white substrate using K-M model:
  - `R = sinh(bSx)/c`, `T = b/c`, `c = a*sinh(bSx) + b*cosh(bSx)`
- Apply paper texture subtly from height field

**Verify:** Paint with multiple colors. Colors mix subtractively (yellow + blue → green). Thin washes are translucent. Paper texture visible through thin areas.

### Complete per-frame dispatch sequence

```
1. Brush input → stamp wet_mask, pressure, pigment (multiple dispatches for stroke points)
2. Apply paper slope to velocity
3. UpdateVelocities (N sub-steps, ping-pong u/v)
4. EnforceBoundary
5. RelaxDivergence (M Jacobi iterations, ping-pong pressure)
6. EnforceBoundary
7. FlowOutward
8. MovePigment (N sub-steps, ping-pong pigment)
9. TransferPigment
10. SimulateCapillaryFlow
11. Render (Kubelka-Munk fullscreen triangle)
```

### Performance notes

- Grid = window resolution (e.g., 1024x768), workgroups 16x16
- Velocity sub-steps: 3-4 (fixed), with velocity clamped to max safe speed
- Jacobi iterations: 20-30 (start with 20, tune visually)
- Total dispatches/frame: ~60-80. Well within GPU budget at 60fps.
- All inter-dispatch barriers are compute→compute (lightweight)

---

## Files to create/modify

### Phase 1 (renderer changes)
- `src/shaders/reflection/parameters.rs` — RWTexture2D detection
- `src/shaders/json/parameters.rs` — ResourceShape::RWTexture2D
- `src/shaders/reflection/pipeline_layout.rs` — MutableTexture → StorageImage
- `src/shaders/json/pipeline_builders.rs` — ReflectedBindingType::StorageImage
- `src/shaders/json.rs` — LayoutDescription::StorageImage routing
- `src/shaders/build_tasks.rs` — StorageTexture2D code gen
- `templates/shader_compute_entry.rs.askama` — storage_texture_handles
- `templates/shader_atlas_entry.rs.askama` — storage_texture_handles
- `src/renderer.rs` — StorageTexture types, creation, descriptor binding

### Phase 2 (basic painting)
- `shaders/source/paint_brush.compute.slang` — create
- `shaders/source/paint_display.shader.slang` — create
- `examples/watercolor.rs` — create

### Phase 3 (simulation, incremental)
- `shaders/source/watercolor_common.slang` — create (shared types + grid helpers)
- `shaders/source/wc_apply_slope.compute.slang` — create
- `shaders/source/wc_update_velocity.compute.slang` — create
- `shaders/source/wc_compute_divergence.compute.slang` — create
- `shaders/source/wc_apply_divergence.compute.slang` — create
- `shaders/source/wc_enforce_boundary.compute.slang` — create
- `shaders/source/wc_flow_outward.compute.slang` — create
- `shaders/source/wc_move_pigment.compute.slang` — create
- `shaders/source/wc_transfer_pigment.compute.slang` — create
- `shaders/source/wc_capillary_flow.compute.slang` — create
- `shaders/source/paint_display.shader.slang` — modify for K-M compositing
- `examples/watercolor.rs` — extend with simulation state + dispatch loop

## Verification

After each phase:
1. `just shaders` — regenerate bindings
2. `cargo check --all` — type check
3. `just test` — snapshot tests (accept new snapshots for generated code)
4. `just dev watercolor` — interactive testing
