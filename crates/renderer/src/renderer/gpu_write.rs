/// Marker type for shaders that don't use vertex input buffers.
/// Used as the vertex type parameter for fullscreen quads, procedural geometry, etc.
pub enum NoVertex {}

/// A marker for someday-generated types that get written to GPU memory
pub trait GPUWrite {}

/// A generated `[[vk::push_constant]]` block
///
/// A shader's generated instance of this will be 128 initialized std430 bytes.
pub trait PushConstantBlock: GPUWrite {}

impl GPUWrite for u8 {} // image bytes
impl GPUWrite for f32 {} // storage texture data
impl GPUWrite for u32 {} // index buffer
impl GPUWrite for NoVertex {}

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
