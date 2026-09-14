//! Vulkan's opaque indexed-indirect argument record.

/// One initialized Vulkan indexed-indirect argument record.
///
/// Matches the Vulkan C struct `VkDrawIndexedIndirectCommand`.
///
/// The record is exactly 20 bytes with 4-byte alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct DrawIndexedIndirectCommand {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub vertex_offset: i32,
    pub first_instance: u32,
}

impl super::render_graph::GPUWrite for DrawIndexedIndirectCommand {}

const _: () = {
    use std::mem::{align_of, offset_of, size_of};
    assert!(size_of::<DrawIndexedIndirectCommand>() == 20);
    assert!(
        size_of::<DrawIndexedIndirectCommand>() == size_of::<ash::vk::DrawIndexedIndirectCommand>()
    );
    assert!(
        align_of::<DrawIndexedIndirectCommand>()
            == align_of::<ash::vk::DrawIndexedIndirectCommand>()
    );
    assert!(offset_of!(DrawIndexedIndirectCommand, index_count) == 0);
    assert!(offset_of!(DrawIndexedIndirectCommand, instance_count) == 4);
    assert!(offset_of!(DrawIndexedIndirectCommand, first_index) == 8);
    assert!(offset_of!(DrawIndexedIndirectCommand, vertex_offset) == 12);
    assert!(offset_of!(DrawIndexedIndirectCommand, first_instance) == 16);
};

#[cfg(test)]
mod tests {
    use super::DrawIndexedIndirectCommand;

    #[test]
    fn indirect_command_vulkan_layout() {
        use std::mem::{align_of, offset_of, size_of};
        assert_eq!(size_of::<DrawIndexedIndirectCommand>(), 20);
        assert_eq!(align_of::<DrawIndexedIndirectCommand>(), 4);
        assert_eq!(
            size_of::<DrawIndexedIndirectCommand>(),
            size_of::<ash::vk::DrawIndexedIndirectCommand>()
        );
        assert_eq!(
            align_of::<DrawIndexedIndirectCommand>(),
            align_of::<ash::vk::DrawIndexedIndirectCommand>()
        );
        assert_eq!(offset_of!(DrawIndexedIndirectCommand, index_count), 0);
        assert_eq!(offset_of!(DrawIndexedIndirectCommand, instance_count), 4);
        assert_eq!(offset_of!(DrawIndexedIndirectCommand, first_index), 8);
        assert_eq!(offset_of!(DrawIndexedIndirectCommand, vertex_offset), 12);
        assert_eq!(offset_of!(DrawIndexedIndirectCommand, first_instance), 16);
    }
}
