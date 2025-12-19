use std::ffi::c_void;
use std::marker::PhantomData;

use ash::vk;

#[derive(Debug)]
pub struct StorageBufferHandle<T> {
    index: usize,
    _phantom_data: PhantomData<T>,
}

pub(super) struct RawStorageBuffer {
    pub(super) buffer: vk::Buffer,
    pub(super) device_mem: vk::DeviceMemory,
    pub(super) mapped_mem: *mut c_void,
}

// NOTE renderer has to enforce type safety
// ordered first by handle index, then by frame
pub(super) struct StorageBufferStorage(Vec<Option<Vec<RawStorageBuffer>>>);

impl StorageBufferStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<T>(&mut self, buffers_per_frame: Vec<RawStorageBuffer>) -> StorageBufferHandle<T> {
        let handle = StorageBufferHandle {
            index: self.0.len(),
            _phantom_data: PhantomData::<T>,
        };

        self.0.push(Some(buffers_per_frame));

        handle
    }

    pub fn get_raw(&self, handle: &RawStorageBufferHandle) -> &[RawStorageBuffer] {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn get_mapped_mem_for_frame<T>(
        &mut self,
        handle: &mut StorageBufferHandle<T>,
        frame: usize,
    ) -> &mut T {
        let raw_storage_buffer = &mut self.0[handle.index].as_mut().unwrap()[frame];
        let mut_ptr = raw_storage_buffer.mapped_mem as *mut T;
        unsafe { &mut *mut_ptr }
    }

    pub fn take<T>(&mut self, handle: StorageBufferHandle<T>) -> Vec<RawStorageBuffer> {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<Vec<RawStorageBuffer>> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

// NOTE find a way to limit this to generated code
//   would need to make PipelineConfig fields private
pub struct RawStorageBufferHandle {
    index: usize,
}

impl RawStorageBufferHandle {
    pub fn from_typed<T>(handle: &StorageBufferHandle<T>) -> Self {
        let index = handle.index;
        Self { index }
    }
}
