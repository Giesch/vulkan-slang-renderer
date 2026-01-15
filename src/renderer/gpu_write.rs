use ash::vk;

/// Marker type for shaders that don't use vertex input buffers.
/// Used as the vertex type parameter for fullscreen quads, procedural geometry, etc.
pub enum NoVertex {}

/// A marker for someday-generated types that get written to GPU memory
///
/// An implementing struct must be repr(C, align(16))
/// and have its fields in descending size/alignment order
pub trait GPUWrite {}

impl GPUWrite for u8 {} // image bytes
impl GPUWrite for u32 {} // index buffer
impl GPUWrite for NoVertex {}

pub(super) unsafe fn write_to_gpu_buffer<T: GPUWrite>(
    device: &ash::Device,
    buffer_memory: vk::DeviceMemory,
    elements: &[T],
) -> anyhow::Result<()> {
    let buffer_size = std::mem::size_of_val(elements) as vk::DeviceSize;

    unsafe {
        let mapped_dst =
            device.map_memory(buffer_memory, 0, buffer_size, Default::default())? as *mut T;
        std::ptr::copy_nonoverlapping(elements.as_ptr(), mapped_dst, elements.len());
        device.unmap_memory(buffer_memory);
    };

    Ok(())
}
