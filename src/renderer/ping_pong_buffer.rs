use super::storage_buffer::StorageBufferHandle;

pub struct PingPongBufferHandle<T> {
    buffers: [StorageBufferHandle<T>; 2],
    current_read: usize,
}

impl<T> PingPongBufferHandle<T> {
    pub(super) fn new(buffers: [StorageBufferHandle<T>; 2]) -> Self {
        Self {
            buffers,
            current_read: 0,
        }
    }

    pub fn read_buffer(&self) -> &StorageBufferHandle<T> {
        &self.buffers[self.current_read]
    }

    pub fn write_buffer(&self) -> &StorageBufferHandle<T> {
        &self.buffers[1 - self.current_read]
    }

    pub(crate) fn read_buffer_mut(&mut self) -> &mut StorageBufferHandle<T> {
        &mut self.buffers[self.current_read]
    }

    pub(crate) fn write_buffer_mut(&mut self) -> &mut StorageBufferHandle<T> {
        &mut self.buffers[1 - self.current_read]
    }

    pub fn swap(&mut self) {
        self.current_read = 1 - self.current_read;
    }
}
