# Bindless Rendering: Vulkan and Metal

## Overview

Bindless rendering replaces per-draw resource binding with large descriptor arrays indexed dynamically at runtime. Both Vulkan and Metal support this pattern, with similar benefits and tradeoffs.

**Benefits:**
- Reduced CPU overhead (fewer bind calls)
- GPU-driven rendering (shaders select resources dynamically)
- Massive resource counts (hundreds of thousands)
- Draw call batching (one bind, many draws)

**Tradeoff:**
- Additional indirection at runtime (typically negligible)

---

## Vulkan: Descriptor Indexing

Vulkan's bindless support comes from `VK_EXT_descriptor_indexing`, promoted to core in Vulkan 1.2.

### Key Features

**1. Unsized descriptor arrays:**
```glsl
// Thousands of textures in one binding
layout(set = 0, binding = 0) uniform sampler2D textures[];
```

**2. Non-uniform dynamic indexing:**
```glsl
// Different invocations access different textures
uint materialId = materials[instanceId].textureIndex;
vec4 color = texture(textures[nonuniformEXT(materialId)], uv);
```

**3. Partially bound descriptors:**
You can have a 10,000 texture array but only populate 500 slots. Requires `VK_DESCRIPTOR_BINDING_PARTIALLY_BOUND_BIT`.

**4. Update-after-bind:**
Descriptors can be updated after binding without invalidating command buffers. See detailed section below.

### Availability

> "The extension has been promoted to Vulkan 1.2... Support is widespread on desktop-class GPUs, but even more modern mobile hardware supports this just fine."

Global limit for update-after-bind descriptors: min-spec 500K.

---

## Metal: Argument Buffer Tiers

Metal has two tiers with very different capabilities:

| Feature | Tier 1 | Tier 2 |
|---------|--------|--------|
| Max buffers | 64 | 500,000 |
| Max textures | 128 | 500,000 |
| Max samplers | 16 | 1,024 |
| Dynamic indexing | Limited/none | Full support |
| Nested argument buffers | No | Yes |
| GPU families | Apple2+, Mac2 | Apple6+ (iPhone 11+), Mac2 |

Tier 2 is essentially bindless - arrays of resources indexed dynamically at runtime.

### Performance Tradeoff

From Apple's documentation:

> "Use the automatic layout mechanism when you don't need to produce a resource hierarchy and your game doesn't use bindless resources. Because this mechanism avoids one level of indirection, it may provide a performance advantage compared to the explicit layout approach."

| Approach | Indirection | Best For |
|----------|-------------|----------|
| Direct binding (Tier 1 style) | None | Fixed resource sets, tight inner loops |
| Argument buffer (Tier 2) | One level | Many resources, dynamic indexing |
| Bindless with heaps | Two levels | Massive resource counts, GPU-driven |

### Practical Guidance

```
Scene complexity vs binding strategy:

Simple scene (< 64 textures, uniform access):
  → Direct binding, skip argument buffers entirely

Medium scene (64-500 textures, some dynamic indexing):
  → Argument buffers, but not fully bindless

Complex scene (1000s of materials, GPU-driven):
  → Full bindless with heaps
```

### Slang Reflection Support

Slang's reflection has special handling for Metal argument buffer tiers via `maybeChangeTypeLayoutToArgumentBufferTier2()`. This ensures correct binding indices and memory layout offsets are generated based on the target tier.

**Tier 1 layout:**
```
// Resources bound directly, strict limits
buffer[0]: params
texture[0]: albedo
texture[1]: normal
sampler[0]: linearSampler
```

**Tier 2 layout:**
```
// Resources packed into argument buffer with pointer/ID fields
argument_buffer[0]:
  offset 0: params (buffer pointer)
  offset 8: albedo (texture ID)
  offset 16: normal (texture ID)
  offset 24: linearSampler (sampler ID)
```

---

## Update-After-Bind (Vulkan)

### What It Does

Update-after-bind lets you update *which resource a descriptor points to* while a command buffer using that descriptor set is pending:

```
Without UPDATE_AFTER_BIND:
  Frame N: Bind descriptor set → Record commands → Submit
           ↓
           Cannot update any descriptors until GPU finishes

With UPDATE_AFTER_BIND:
  Frame N: Bind descriptor set → Record commands → Submit
           ↓
           Can update UNUSED descriptors immediately
           (e.g., add new textures to slots 500-600 while GPU uses slots 0-100)
```

### What It Does NOT Do

It does **not** let you write to a buffer while the GPU reads from it. That's still a data race:

```
// This is STILL illegal regardless of update-after-bind:
GPU: reading buffer A
CPU: writing to buffer A  // Race condition!
```

The descriptor update changes a pointer in a metadata table, not the underlying data.

### Setup Requirements

To use update-after-bind:

1. `VK_DESCRIPTOR_POOL_CREATE_UPDATE_AFTER_BIND_BIT` on the pool
2. `VK_DESCRIPTOR_SET_LAYOUT_CREATE_UPDATE_AFTER_BIND_POOL_BIT` on the layout
3. `VK_DESCRIPTOR_BINDING_UPDATE_AFTER_BIND_BIT` on each binding

### Restrictions

From the spec:

> "The contents may be consumed when the command buffer is submitted to a queue, or during shader execution of the resulting draws and dispatches, or any time in between."

This means:
- Once submitted, the GPU might read the descriptor at any point
- You can only safely update descriptors the GPU won't access
- Use `VK_DESCRIPTOR_BINDING_UPDATE_UNUSED_WHILE_PENDING_BIT` for slots not used by pending work

### Where It Actually Helps

| Use Case | Benefit |
|----------|---------|
| Streaming textures | Add to descriptor set without waiting for GPU |
| Dynamic resource management | Add/remove resources without rebuilding sets |
| Reducing frame latency | Update descriptors immediately after submit |

---

## Ping-Pong Buffers: Still Required

For simulation data that's read in frame N and written for frame N+1, **you still need double-buffering**:

```rust
struct SimulationState {
    // Two copies of actual data - still required
    agent_buffers: [StorageBuffer<Agent>; 2],
    current: usize,
}
```

Update-after-bind helps with *descriptor management*, not with the fundamental data hazard:

| Resource Type | Ping-Pong Needed? | Update-After-Bind Helps? |
|---------------|-------------------|-------------------------|
| Simulation data (read/write each frame) | **Yes** | No |
| Streaming textures (load async) | No | **Yes** |
| Material library (add/remove) | No | **Yes** |

The cross-frame simulation/rendering pattern:

```rust
fn frame(&mut self, frame_index: usize) {
    let read_idx = frame_index % 2;
    let write_idx = (frame_index + 1) % 2;

    // Update descriptors to swap read/write buffers
    // (changing pointers, not data)
    update_descriptor(slot: 0, &self.agent_buffers[read_idx]);   // Render reads
    update_descriptor(slot: 1, &self.agent_buffers[write_idx]);  // Sim writes
}
```

---

## Cross-Platform Design Implications

### Unified Mental Model

Both Vulkan and Metal converge on the same pattern:
- Bind a large resource set once
- Index dynamically in shaders
- Update unused slots without stalls

### Backend Abstraction

```rust
enum BindingMode {
    // Small resource counts, no dynamic indexing
    Direct,
    // Many resources or dynamic indexing
    Bindless,
}

impl Backend {
    fn bind_resources(&self, resources: &Resources) {
        match self.binding_mode {
            Direct => {
                // Per-resource binding calls
            }
            Bindless => {
                // Single global descriptor set/argument buffer
                // Shader indexes by resource ID
            }
        }
    }
}
```

### For FLAME-Style Simulation

Bindless aligns well with GPU-driven simulation:

```rust
// Traditional: bind per agent type
for agent_type in agent_types {
    bind_descriptor_set(agent_type.resources);
    dispatch(agent_type.count);
}

// Bindless: single bind, index by agent type
bind_global_descriptor_set();  // Contains ALL resources
dispatch(total_agents);        // Shader indexes by agent.type_id
```

---

## References

### Vulkan

- [Vulkan Descriptor Indexing Sample](https://docs.vulkan.org/samples/latest/samples/extensions/descriptor_indexing/README.html)
- [VK_EXT_descriptor_indexing Guide](https://docs.vulkan.org/guide/latest/extensions/VK_EXT_descriptor_indexing.html)
- [VkDescriptorBindingFlagBits (Vulkan Spec)](https://registry.khronos.org/vulkan/specs/1.3-extensions/man/html/VkDescriptorBindingFlagBits.html)
- [Bindless Descriptor Sets (Vincent Parizet)](https://www.vincentparizet.com/blog/posts/vulkan_bindless_descriptors/)
- [A Note on Descriptor Indexing (Chunk Stories)](https://chunkstories.xyz/blog/a-note-on-descriptor-indexing/)
- [Managing Bindless Descriptors in Vulkan](https://dev.to/gasim/implementing-bindless-design-in-vulkan-34no)
- [VK_EXT_descriptor_buffer (Khronos Blog)](https://www.khronos.org/blog/vk-ext-descriptor-buffer)

### Metal

- [Metal Feature Set Tables (Apple)](https://developer.apple.com/metal/Metal-Feature-Set-Tables.pdf)
- [MTLArgumentBuffersTier.tier2 Documentation](https://developer.apple.com/documentation/metal/mtlargumentbufferstier/tier2)
- [Improving CPU Performance by Using Argument Buffers (Apple)](https://developer.apple.com/documentation/metal/buffers/improving_cpu_performance_by_using_argument_buffers)
- [Explore Bindless Rendering in Metal - WWDC21](https://developer.apple.com/videos/play/wwdc2021/10286/)
- [Go Bindless with Metal 3 - WWDC22](https://developer.apple.com/videos/play/wwdc2022/10101/)
- [Encoding Argument Buffers on the GPU (Apple)](https://developer.apple.com/documentation/metal/buffers/encoding_argument_buffers_on_the_gpu)
