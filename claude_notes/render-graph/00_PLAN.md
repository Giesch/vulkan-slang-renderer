# Compute Shader Support Implementation Plan

## Overview

Add compute shader support to the Vulkan renderer, enabling GPU compute workloads alongside the existing graphics pipeline.

---

## Current Architecture Analysis

### What Already Exists (Reusable)

- **`ReflectedStageFlags::Compute`** - Stage flag enum already includes compute
- **Descriptor/binding system** - Generic, works for any shader stage
- **`PipelineHandle<T>` pattern** - Marker-based type safety, extensible
- **Per-frame resource management** - `MAX_FRAMES_IN_FLIGHT` double-buffering works identically
- **Slang compiler integration** - Supports `slang::Stage::Compute`

### Current Limitations

- Shader compilation hardcoded for vertex+fragment pairs
- `ReflectedShader` expects exactly 2 entry points
- Code generator assumes graphics pipeline structure
- No `vkCreateComputePipelines` path in renderer
- Templates generate vertex/index buffer fields unconditionally
- No support for RW resources (storage images, RW buffers)

---

## Phase 1: Resource Access Reflection

Slang provides `ResourceAccess` separately from `ResourceShape`:

```c
enum SlangResourceAccess {
    SLANG_RESOURCE_ACCESS_NONE,
    SLANG_RESOURCE_ACCESS_READ,
    SLANG_RESOURCE_ACCESS_READ_WRITE,
    SLANG_RESOURCE_ACCESS_RASTER_ORDERED,
}
```

### File: `src/shaders/json/parameters.rs`

Add access mode to resource reflection:

```rust
pub struct Resource {
    pub field_name: String,
    pub binding: Binding,
    pub resource_shape: ResourceShape,
    pub resource_access: ResourceAccess,  // NEW
    pub result_type: ResourceResultType,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResourceAccess {
    Read,       // StructuredBuffer<T>, Texture2D<T>
    ReadWrite,  // RWStructuredBuffer<T>, RWTexture2D<T>
}
```

### File: `src/shaders/reflection/parameters.rs`

Update reflection to extract access mode:

```rust
let resource_access = match field_type_layout.resource_access() {
    Some(slang::ResourceAccess::SlangResourceAccessRead) => ResourceAccess::Read,
    Some(slang::ResourceAccess::SlangResourceAccessReadWrite) => ResourceAccess::ReadWrite,
    other => todo!("unhandled resource access: {other:?}"),
};
```

### File: `src/shaders/json/pipeline_builders.rs`

Add storage image binding type:

```rust
pub enum ReflectedBindingType {
    Sampler,
    Texture,
    ConstantBuffer,
    CombinedTextureSampler,
    StorageBuffer,
    StorageImage,  // NEW - for RWTexture2D, etc.
}
```

### Resource Type Mapping

| Slang Type | Shape | Access | Vulkan Descriptor | Rust Handle |
|------------|-------|--------|-------------------|-------------|
| `Texture2D<T>` | Texture2D | Read | `COMBINED_IMAGE_SAMPLER` | `TextureHandle` |
| `RWTexture2D<T>` | Texture2D | ReadWrite | `STORAGE_IMAGE` | `StorageImageHandle` |
| `StructuredBuffer<T>` | StructuredBuffer | Read | `STORAGE_BUFFER` | `StorageBufferHandle<T>` |
| `RWStructuredBuffer<T>` | StructuredBuffer | ReadWrite | `STORAGE_BUFFER` | `StorageBufferHandle<T>` |

Note: Both RO and RW structured buffers use `STORAGE_BUFFER` descriptor type. The difference is the shader's access pattern. We use the same `StorageBufferHandle<T>` for both.

---

## Phase 2: Storage Image Support

### File: `src/renderer.rs`

Add storage image handle and creation:

```rust
pub struct StorageImageHandle {
    index: usize,
}

impl Renderer {
    pub fn create_storage_image(
        &mut self,
        width: u32,
        height: u32,
        format: vk::Format,
    ) -> Result<StorageImageHandle> {
        // Create image with VK_IMAGE_USAGE_STORAGE_BIT
        // Create image view
        // Transition to VK_IMAGE_LAYOUT_GENERAL
    }
}
```

### File: `src/shaders/build_tasks.rs`

Update code generation to emit correct handle types:

```rust
RequiredResourceType::Texture(access) => match access {
    ResourceAccess::Read => format!("&'a TextureHandle"),
    ResourceAccess::ReadWrite => format!("&'a StorageImageHandle"),
}
```

---

## Phase 3: Pipeline Type System

### File: `src/renderer/pipeline.rs`

Add compute marker type:

```rust
pub struct Compute;
impl DrawCall for Compute {}
```

Note: `DrawCall` trait name becomes slightly misleading. Consider renaming to `PipelineType` in a follow-up refactor.

---

## Phase 4: Shader Compilation

### File: `src/shaders.rs`

1. Update `ReflectedShader` to support compute:

```rust
pub struct ReflectedShader {
    // Existing graphics fields (make optional)
    pub vertex_shader: Option<CompiledShader>,
    pub fragment_shader: Option<CompiledShader>,

    // New compute field
    pub compute_shader: Option<CompiledShader>,

    pub reflection_json: String,
}
```

2. Add shader type detection:

```rust
pub enum ShaderType {
    Graphics,  // vertex + fragment
    Compute,   // compute only
}
```

3. Create `prepare_compute_shader()` or modify `prepare_reflected_shader()` to:
   - Detect compute entry point (`[shader("compute")]` in Slang)
   - Compile single compute stage
   - Generate reflection for compute parameters

**File naming convention:**
- `*.shader.slang` - Graphics shaders (vertex + fragment)
- `*.compute.slang` - Compute shaders

---

## Phase 5: Reflection & Code Generation

### File: `src/shaders/reflection.rs`

Update `ReflectionJson` structure:

```rust
pub struct ReflectionJson {
    pub shader_type: ShaderType,

    // Graphics (optional)
    pub vertex_entry_point: Option<EntryPoint>,
    pub fragment_entry_point: Option<EntryPoint>,

    // Compute (optional)
    pub compute_entry_point: Option<EntryPoint>,
    pub workgroup_size: Option<[u32; 3]>,  // From [numthreads(X,Y,Z)]

    pub pipeline_layout: ReflectedPipelineLayout,
}
```

### File: `src/shaders/build_tasks.rs`

1. Detect shader type from file extension or entry points
2. Route to appropriate code generation path
3. Extract `[numthreads]` workgroup size from reflection

---

## Phase 6: Template Updates

### New file: `templates/shader_compute.rs.askama`

Compute-specific template generating:

```rust
pub struct Resources<'a> {
    // Only uniform/storage buffers and textures
    // No vertices or indices
    pub params: &'a UniformBufferHandle<ComputeParams>,
    pub input: &'a TextureHandle,
    pub output: &'a StorageImageHandle,
}

impl Shader {
    pub const WORKGROUP_SIZE: [u32; 3] = [16, 16, 1];

    pub fn pipeline_config(resources: Resources<'_>) -> ComputePipelineConfig<'_> {
        // ...
    }
}
```

### File: `templates/shader_atlas_entry.rs.askama`

Add conditional rendering:

```jinja
{% if shader_type == ShaderType::Compute %}
    {% include "shader_compute_impl.askama" %}
{% else %}
    {% include "shader_graphics_impl.askama" %}
{% endif %}
```

### File: `templates/shader_atlas.rs.askama`

Update `PrecompiledShaders` or create `PrecompiledComputeShader`:

```rust
pub enum PrecompiledShaders {
    Graphics { vert: &'static [u8], frag: &'static [u8] },
    Compute { comp: &'static [u8] },
}
```

---

## Phase 7: Renderer Pipeline Creation

### File: `src/renderer.rs`

1. Add `ComputePipelineConfig` struct:

```rust
pub struct ComputePipelineConfig<'a> {
    pub shader: &'a dyn ComputeShaderAtlasEntry,
    pub resources: ComputeResources<'a>,
}
```

2. Add `create_compute_pipeline()`:

```rust
pub fn create_compute_pipeline<S: ComputeShaderAtlasEntry>(
    &mut self,
    config: ComputePipelineConfig,
) -> Result<PipelineHandle<Compute>> {
    // 1. Create shader module from SPIR-V
    // 2. Create pipeline layout (descriptor sets, push constants)
    // 3. Call vkCreateComputePipelines
    // 4. Store in pipeline storage
    // 5. Return typed handle
}
```

3. Add compute pipeline Vulkan creation:

```rust
fn create_vulkan_compute_pipeline(
    &self,
    shader_module: vk::ShaderModule,
    layout: vk::PipelineLayout,
) -> Result<vk::Pipeline> {
    let stage_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader_module)
        .name(c"main");

    let create_info = vk::ComputePipelineCreateInfo::default()
        .stage(stage_info)
        .layout(layout);

    unsafe {
        self.device.create_compute_pipelines(
            vk::PipelineCache::null(),
            &[create_info],
            None,
        )
    }
}
```

---

## Phase 8: Dispatch Commands

### File: `src/renderer.rs` (FrameRenderer impl)

Add dispatch method:

```rust
impl FrameRenderer<'_> {
    pub fn dispatch(
        &mut self,
        pipeline: PipelineHandle<Compute>,
        group_count_x: u32,
        group_count_y: u32,
        group_count_z: u32,
    ) {
        // 1. Bind compute pipeline
        // 2. Bind descriptor sets
        // 3. cmd_dispatch(group_count_x, group_count_y, group_count_z)
    }

    pub fn dispatch_indirect(
        &mut self,
        pipeline: PipelineHandle<Compute>,
        buffer: &StorageBufferHandle<DispatchIndirectCommand>,
        offset: u64,
    ) {
        // For GPU-driven dispatch counts
    }
}
```

### Render Pass Interaction

Compute dispatches cannot occur inside a render pass. Options:

1. **Require `dispatch()` before `begin_render_pass()`** (recommended)
2. Add a separate `ComputeEncoder` type
3. Auto-end render pass if active (implicit, potentially confusing)

Recommend option 1 with runtime validation in debug builds.

---

## Phase 9: Memory Barriers

Compute shaders need explicit synchronization:

```rust
impl FrameRenderer<'_> {
    pub fn memory_barrier(
        &mut self,
        src_stage: vk::PipelineStageFlags,
        dst_stage: vk::PipelineStageFlags,
        src_access: vk::AccessFlags,
        dst_access: vk::AccessFlags,
    ) {
        // vkCmdPipelineBarrier
    }

    pub fn image_barrier(
        &mut self,
        image: &StorageImageHandle,
        old_layout: vk::ImageLayout,
        new_layout: vk::ImageLayout,
    ) {
        // Image memory barrier for layout transitions
    }
}
```

---

## Phase 10: Hot Reload Support

### File: `src/shader_watcher.rs`

Extend to watch `*.compute.slang` files and reload compute pipelines.

---

## Example Target API

### Shader Definition (`shaders/source/blur.compute.slang`)

```slang
struct BlurParams {
    float radius;
    int2 imageSize;
};

[[vk::binding(0, 0)]] ConstantBuffer<BlurParams> params;
[[vk::binding(1, 0)]] Texture2D<float4> inputImage;
[[vk::binding(2, 0)]] RWTexture2D<float4> outputImage;

[shader("compute")]
[numthreads(16, 16, 1)]
void main(uint3 tid : SV_DispatchThreadID) {
    // Blur implementation
}
```

### Generated Rust (`src/generated/blur.rs`)

```rust
#[repr(C)]
#[derive(Clone, Copy, Std140)]
pub struct BlurParams {
    pub radius: f32,
    pub image_size: IVec2,
}

pub struct Resources<'a> {
    pub params: &'a UniformBufferHandle<BlurParams>,
    pub input_image: &'a TextureHandle,
    pub output_image: &'a StorageImageHandle,
}

impl Shader {
    pub const WORKGROUP_SIZE: [u32; 3] = [16, 16, 1];

    pub fn pipeline_config(resources: Resources<'_>) -> ComputePipelineConfig<'_> {
        // ...
    }
}
```

### Usage in Game

```rust
fn setup(renderer: &mut Renderer) -> Result<Self> {
    let blur_pipeline = renderer.create_compute_pipeline(
        blur::Shader::pipeline_config(blur::Resources {
            params: &self.blur_params,
            input_image: &self.scene_texture,
            output_image: &self.blur_output,
        })
    )?;

    Ok(Self { blur_pipeline, /* ... */ })
}

fn draw(&mut self, mut frame: FrameRenderer) -> Result<(), DrawError> {
    // Compute pass (before render pass)
    frame.dispatch(
        self.blur_pipeline,
        self.width / blur::Shader::WORKGROUP_SIZE[0],
        self.height / blur::Shader::WORKGROUP_SIZE[1],
        1,
    );

    frame.memory_barrier(
        vk::PipelineStageFlags::COMPUTE_SHADER,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::AccessFlags::SHADER_WRITE,
        vk::AccessFlags::SHADER_READ,
    );

    // Graphics pass
    frame.begin_render_pass(/* ... */);
    // ...
}
```

---

## Open Questions

1. **Trait naming**: Rename `DrawCall` to `PipelineType`?
2. **Async compute**: Support separate compute queue? (Probably future work)
3. **Indirect dispatch**: Include `dispatch_indirect` in initial implementation?
4. **Shared memory**: Expose `groupshared` reflection for buffer sizing?
5. **Buffer access modes**: Use same `StorageBufferHandle<T>` for RO and RW, or separate types?

---

## Testing Strategy

1. **Unit tests**: Reflection parsing for compute shaders and RW resources
2. **Snapshot tests**: Generated Rust code for compute shaders
3. **Integration test**: Simple compute shader that writes to buffer, read back and verify
4. **Example**: Add `compute_blur` or `compute_particles` example

---

## File Change Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `src/renderer/pipeline.rs` | Modify | Add `Compute` marker type |
| `src/shaders.rs` | Modify | Add compute compilation path |
| `src/shaders/json/parameters.rs` | Modify | Add `ResourceAccess` enum |
| `src/shaders/reflection/parameters.rs` | Modify | Extract resource access mode |
| `src/shaders/json/pipeline_builders.rs` | Modify | Add `StorageImage` binding type |
| `src/shaders/reflection.rs` | Modify | Support compute entry points |
| `src/shaders/build_tasks.rs` | Modify | Detect and route compute shaders, handle RW resources |
| `src/renderer.rs` | Modify | Add `create_compute_pipeline()`, `dispatch()`, `StorageImageHandle` |
| `templates/shader_compute.rs.askama` | New | Compute shader template |
| `templates/shader_atlas_entry.rs.askama` | Modify | Conditional graphics/compute |
| `templates/shader_atlas.rs.askama` | Modify | Support compute in enum |
| `src/shader_watcher.rs` | Modify | Watch compute shaders |
| `shaders/source/*.compute.slang` | New | Test compute shaders |
| `examples/compute_*.rs` | New | Example compute usage |

---

## Implementation Order

1. **Phase 1-2**: Resource access reflection + storage images (enables RW resources for graphics too)
2. **Phase 3-5**: Pipeline type + shader compilation + reflection updates
3. **Phase 6-7**: Templates + pipeline creation
4. **Phase 8-9**: Dispatch commands + barriers
5. **Phase 10**: Hot reload
