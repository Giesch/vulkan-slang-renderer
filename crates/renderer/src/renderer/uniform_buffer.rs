use std::ffi::c_void;
use std::marker::PhantomData;

use ash::vk;

use super::MAX_FRAMES_IN_FLIGHT;

#[derive(Debug)]
pub struct UniformBufferHandle<T> {
    index: usize,
    _phantom_data: PhantomData<T>,
}

impl<T> UniformBufferHandle<T> {
    pub(super) fn index(&self) -> usize {
        self.index
    }
}

pub(super) struct RawUniformBuffer {
    pub(super) buffer: vk::Buffer,
    /// Logical per-flight payload size, not the allocator's padded size.
    pub(super) byte_size: u64,
    pub(super) allocation: vk_mem::Allocation,
    /// cached from the persistently-mapped allocation's info
    pub(super) mapped_mem: *mut c_void,
}

// NOTE renderer has to enforce type safety
// ordered first by handle index, then by frame
pub(super) struct UniformBufferStorage(Vec<Option<[RawUniformBuffer; MAX_FRAMES_IN_FLIGHT]>>);

impl UniformBufferStorage {
    pub(super) fn contains(&self, index: usize) -> bool {
        self.0.get(index).is_some_and(Option::is_some)
    }

    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<T>(
        &mut self,
        buffers_per_frame: [RawUniformBuffer; MAX_FRAMES_IN_FLIGHT],
    ) -> UniformBufferHandle<T> {
        let handle = UniformBufferHandle {
            index: self.0.len(),
            _phantom_data: PhantomData::<T>,
        };

        self.0.push(Some(buffers_per_frame));

        handle
    }

    pub fn get_raw(
        &self,
        handle: &RawUniformBufferHandle,
    ) -> &[RawUniformBuffer; MAX_FRAMES_IN_FLIGHT] {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn get_mapped_mem_for_frame<T>(
        &mut self,
        handle: &mut UniformBufferHandle<T>,
        frame: usize,
    ) -> &mut T {
        let raw_uniform_buffer = &mut self.0[handle.index].as_mut().unwrap()[frame];
        let mut_ptr = raw_uniform_buffer.mapped_mem as *mut T;
        unsafe { &mut *mut_ptr }
    }

    pub(super) fn upload_target(
        &self,
        index: usize,
        frame: usize,
    ) -> Option<super::graph_backend::UploadTarget> {
        let raw = self.0.get(index)?.as_ref()?.get(frame)?;
        Some(super::graph_backend::UploadTarget {
            byte_size: raw.byte_size,
            mapped_mem: raw.mapped_mem,
            kind: super::graph_backend::UploadKind::Uniform,
        })
    }

    #[expect(dead_code)]
    pub fn take<T>(
        &mut self,
        handle: UniformBufferHandle<T>,
    ) -> [RawUniformBuffer; MAX_FRAMES_IN_FLIGHT] {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<[RawUniformBuffer; MAX_FRAMES_IN_FLIGHT]> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

// NOTE find a way to limit this to generated code
//   would need to make PipelineConfig fields private
pub struct RawUniformBufferHandle {
    index: usize,
}

impl RawUniformBufferHandle {
    pub fn from_typed<T>(handle: &UniformBufferHandle<T>) -> Self {
        let index = handle.index;
        Self { index }
    }
}
