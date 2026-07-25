use std::ffi::c_void;
use std::marker::PhantomData;

use ash::vk;

use super::PRE_WAIT_RING_LEN;

#[derive(Debug)]
pub struct StorageBufferHandle<T> {
    index: usize,
    len: u32,
    _phantom_data: PhantomData<T>,
}

#[expect(clippy::len_without_is_empty)] // vulkan does not allow allocating an empty buffer
impl<T> StorageBufferHandle<T> {
    pub fn len(&self) -> u32 {
        self.len
    }
}

/// A storage buffer that nothing on the GPU ever writes
///
/// It can only mint `ImmutableAddr<T>` (never a writable `Addr<T>`).
///
/// The CPU may still update it between frames via `Gpu::write_immutable`
#[derive(Debug)]
pub struct ImmutableBufferHandle<T> {
    index: usize,
    len: u32,
    _phantom_data: PhantomData<T>,
}

#[expect(clippy::len_without_is_empty)] // vulkan does not allow allocating an empty buffer
impl<T> ImmutableBufferHandle<T> {
    pub fn len(&self) -> u32 {
        self.len
    }
}

/// A storage buffer the CPU writes only at setup, never from `gpu_update`
///
/// During the frame loop only the GPU touches it, reading and writing via
/// `Addr`/`ReadAddr`. Because no CPU write can land before the frame_timeline
/// wait, this is the only handle that can mint a `Gpu::previous_addr` history
/// pointer: an in-flight frame's read of the previous slot cannot race a CPU
/// write that no longer exists.
///
/// Initialize with `Renderer::write_gpu_only_all_frames`.
#[derive(Debug)]
pub struct GpuOnlyBufferHandle<T> {
    index: usize,
    len: u32,
    _phantom_data: PhantomData<T>,
}

#[expect(clippy::len_without_is_empty)] // vulkan does not allow allocating an empty buffer
impl<T> GpuOnlyBufferHandle<T> {
    pub fn len(&self) -> u32 {
        self.len
    }
}

pub(super) struct RawStorageBuffer {
    pub(super) buffer: vk::Buffer,
    pub(super) allocation: vk_mem::Allocation,
    /// cached from the persistently-mapped allocation's info
    pub(super) mapped_mem: *mut c_void,
    /// cached at creation; stable for the buffer's whole life
    pub(super) device_address: vk::DeviceAddress,
}

// NOTE renderer has to enforce type safety
// ordered first by handle index, then by frame
pub(super) struct StorageBufferStorage(Vec<Option<[RawStorageBuffer; PRE_WAIT_RING_LEN]>>);

impl StorageBufferStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<T>(
        &mut self,
        buffers_per_frame: [RawStorageBuffer; PRE_WAIT_RING_LEN],
        len: u32,
    ) -> StorageBufferHandle<T> {
        let handle = StorageBufferHandle {
            index: self.0.len(),
            len,
            _phantom_data: PhantomData::<T>,
        };

        self.0.push(Some(buffers_per_frame));

        handle
    }

    pub(super) fn get_device_address_for_frame<T>(
        &self,
        handle: &StorageBufferHandle<T>,
        frame: usize,
    ) -> vk::DeviceAddress {
        self.0[handle.index].as_ref().unwrap()[frame].device_address
    }

    pub(super) fn get_mapped_mem_for_frame<T>(
        &mut self,
        handle: &mut StorageBufferHandle<T>,
        frame: usize,
    ) -> *mut T {
        let raw_storage_buffer = &mut self.0[handle.index].as_mut().unwrap()[frame];
        raw_storage_buffer.mapped_mem as *mut T
    }

    pub fn take<T>(
        &mut self,
        handle: StorageBufferHandle<T>,
    ) -> [RawStorageBuffer; PRE_WAIT_RING_LEN] {
        self.0[handle.index].take().unwrap()
    }

    // Immutable buffers share this storage; the distinct handle type (with no
    // Addr accessors) is what keeps them un-writable on the GPU.

    pub fn add_immutable<T>(
        &mut self,
        buffers_per_frame: [RawStorageBuffer; PRE_WAIT_RING_LEN],
        len: u32,
    ) -> ImmutableBufferHandle<T> {
        let handle = ImmutableBufferHandle {
            index: self.0.len(),
            len,
            _phantom_data: PhantomData::<T>,
        };

        self.0.push(Some(buffers_per_frame));

        handle
    }

    pub(super) fn get_device_address_for_frame_immutable<T>(
        &self,
        handle: &ImmutableBufferHandle<T>,
        frame: usize,
    ) -> vk::DeviceAddress {
        self.0[handle.index].as_ref().unwrap()[frame].device_address
    }

    pub(super) fn get_mapped_mem_for_frame_immutable<T>(
        &mut self,
        handle: &mut ImmutableBufferHandle<T>,
        frame: usize,
    ) -> *mut T {
        let raw_storage_buffer = &mut self.0[handle.index].as_mut().unwrap()[frame];
        raw_storage_buffer.mapped_mem as *mut T
    }

    pub fn take_immutable<T>(
        &mut self,
        handle: ImmutableBufferHandle<T>,
    ) -> [RawStorageBuffer; PRE_WAIT_RING_LEN] {
        self.0[handle.index].take().unwrap()
    }

    // GPU-only buffers also share this storage; their handle type is what
    // keeps them out of Gpu's per-frame CPU write methods.

    pub fn add_gpu_only<T>(
        &mut self,
        buffers_per_frame: [RawStorageBuffer; PRE_WAIT_RING_LEN],
        len: u32,
    ) -> GpuOnlyBufferHandle<T> {
        let handle = GpuOnlyBufferHandle {
            index: self.0.len(),
            len,
            _phantom_data: PhantomData::<T>,
        };

        self.0.push(Some(buffers_per_frame));

        handle
    }

    pub(super) fn get_device_address_for_frame_gpu_only<T>(
        &self,
        handle: &GpuOnlyBufferHandle<T>,
        frame: usize,
    ) -> vk::DeviceAddress {
        self.0[handle.index].as_ref().unwrap()[frame].device_address
    }

    pub(super) fn get_mapped_mem_for_frame_gpu_only<T>(
        &mut self,
        handle: &mut GpuOnlyBufferHandle<T>,
        frame: usize,
    ) -> *mut T {
        let raw_storage_buffer = &mut self.0[handle.index].as_mut().unwrap()[frame];
        raw_storage_buffer.mapped_mem as *mut T
    }

    pub fn take_gpu_only<T>(
        &mut self,
        handle: GpuOnlyBufferHandle<T>,
    ) -> [RawStorageBuffer; PRE_WAIT_RING_LEN] {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<[RawStorageBuffer; PRE_WAIT_RING_LEN]> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}
