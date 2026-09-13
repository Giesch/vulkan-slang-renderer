//! Vulkan's opaque indexed-indirect argument record.

/// One initialized Vulkan indexed-indirect argument record.
///
/// The record is exactly 20 bytes with 4-byte alignment. Construct it with
/// [`Self::new`]; its private representation cannot be replaced by a different
/// implementation of an accessor interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct DrawIndexedIndirectCommand {
    index_count: u32,
    instance_count: u32,
    first_index: u32,
    vertex_offset: i32,
    first_instance: u32,
}

impl DrawIndexedIndirectCommand {
    pub const fn new(
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        vertex_offset: i32,
        first_instance: u32,
    ) -> Self {
        Self {
            index_count,
            instance_count,
            first_index,
            vertex_offset,
            first_instance,
        }
    }

    pub const fn index_count(self) -> u32 {
        self.index_count
    }
    pub const fn instance_count(self) -> u32 {
        self.instance_count
    }
    pub const fn first_index(self) -> u32 {
        self.first_index
    }
    pub const fn vertex_offset(self) -> i32 {
        self.vertex_offset
    }
    pub const fn first_instance(self) -> u32 {
        self.first_instance
    }

    pub fn set_index_count(&mut self, value: u32) {
        self.index_count = value;
    }
    pub fn set_instance_count(&mut self, value: u32) {
        self.instance_count = value;
    }
    pub fn set_first_index(&mut self, value: u32) {
        self.first_index = value;
    }
    pub fn set_vertex_offset(&mut self, value: i32) {
        self.vertex_offset = value;
    }
    pub fn set_first_instance(&mut self, value: u32) {
        self.first_instance = value;
    }
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

    #[test]
    fn indirect_command_accessors() {
        let mut command = DrawIndexedIndirectCommand::new(11, 12, 13, -14, 15);
        assert_eq!(
            (
                command.index_count(),
                command.instance_count(),
                command.first_index(),
                command.vertex_offset(),
                command.first_instance()
            ),
            (11, 12, 13, -14, 15)
        );
        command.set_index_count(21);
        command.set_instance_count(22);
        command.set_first_index(23);
        command.set_vertex_offset(-24);
        command.set_first_instance(25);
        assert_eq!(
            command,
            DrawIndexedIndirectCommand::new(21, 22, 23, -24, 25)
        );
    }
}
