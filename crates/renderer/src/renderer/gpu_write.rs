/// Marker type for shaders that don't use vertex input buffers.
/// Used as the vertex type parameter for fullscreen quads, procedural geometry, etc.
pub enum NoVertex {}

/// A marker for types that get written to GPU memory.
///
/// This is the renderer module's version of `renderer::render_graph::GPUWrite`.
pub(crate) trait GPUWrite {}

/// A generated `[[vk::push_constant]]` block
///
/// A shader's generated instance of this will be 128 initialized std430 bytes.
///
/// This is the renderer module's version of `render_graph::PushConstantBlock`.
pub(crate) trait PushConstantBlock: GPUWrite {}

impl GPUWrite for NoVertex {}

// One-way bridges from the graph's construction vocabulary to the renderer's.
// The primitives and generated shader blocks implement the graph traits.
impl<T: super::render_graph::GPUWrite> GPUWrite for T {}
impl<T: super::render_graph::PushConstantBlock> PushConstantBlock for T {}

pub(super) unsafe fn write_to_gpu_buffer<T: GPUWrite>(
    allocator: &vk_mem::Allocator,
    allocation: &mut vk_mem::Allocation,
    elements: &[T],
) -> anyhow::Result<()> {
    unsafe {
        let mapped_dst = allocator.map_memory(allocation)? as *mut T;
        std::ptr::copy_nonoverlapping(elements.as_ptr(), mapped_dst, elements.len());
        allocator.unmap_memory(allocation);
    };

    Ok(())
}
