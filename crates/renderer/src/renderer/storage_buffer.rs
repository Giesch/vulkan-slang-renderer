use std::ffi::c_void;
use std::marker::PhantomData;

use ash::vk;

use super::MAX_FRAMES_IN_FLIGHT;
use super::addr::ImmutableAddr;

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
/// `Addr`/`ReadAddr`. That is what lets it mint a `Gpu::previous_addr` history
/// pointer, which no other handle can: reading the previous slot is only
/// meaningful for a buffer whose slots hold GPU output rather than whatever
/// the CPU last wrote there.
///
/// The write-after-read on the slot being reused — frame N's compute reads
/// slot s via `previous_addr` while frame N+1's compute writes it — is the one
/// hazard the frame_timeline wait does not cover. It is ordered by the barrier
/// at the top of every command buffer instead; see `record_command_buffer`.
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
pub(super) struct StorageBufferStorage(Vec<Option<[RawStorageBuffer; MAX_FRAMES_IN_FLIGHT]>>);

impl StorageBufferStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<T>(
        &mut self,
        buffers_per_frame: [RawStorageBuffer; MAX_FRAMES_IN_FLIGHT],
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
    ) -> [RawStorageBuffer; MAX_FRAMES_IN_FLIGHT] {
        self.0[handle.index].take().unwrap()
    }

    // Immutable buffers share this storage; the distinct handle type (with no
    // Addr accessors) is what keeps them un-writable on the GPU.

    pub fn add_immutable<T>(
        &mut self,
        buffers_per_frame: [RawStorageBuffer; MAX_FRAMES_IN_FLIGHT],
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

    /// The address of a single element, for pointing a shader at one struct in
    /// the buffer rather than at its base.
    ///
    /// The stride is `size_of::<T>()`, which is the slang std430 array stride
    /// by an existing test rather than by assumption: codegen emits
    /// `assert!(size_of::<T>() == expected_size)` and
    /// `pointer_pointee_spirv_layout` checks that same number against the
    /// emitted SPIR-V `ArrayStride`. It is also what
    /// `create_storage_buffers_per_frame` sizes the allocation with.
    pub(super) fn get_element_device_address_for_frame_immutable<T>(
        &self,
        handle: &ImmutableBufferHandle<T>,
        frame: usize,
        index: u32,
    ) -> vk::DeviceAddress {
        self.get_device_address_for_frame_immutable(handle, frame)
            + element_byte_offset(index, handle.len, std::mem::size_of::<T>())
    }

    pub(super) fn immutable_addr_for_frame<T>(
        &self,
        handle: &ImmutableBufferHandle<T>,
        frame: usize,
    ) -> ImmutableAddr<T> {
        let address = self.get_device_address_for_frame_immutable(handle, frame);
        ImmutableAddr::from_raw(address)
    }

    /// # Panics
    ///
    /// If `index` is out of bounds, see `element_byte_offset`.
    pub(super) fn immutable_element_addr_for_frame<T>(
        &self,
        handle: &ImmutableBufferHandle<T>,
        frame: usize,
        index: u32,
    ) -> ImmutableAddr<T> {
        let address = self.get_element_device_address_for_frame_immutable(handle, frame, index);
        ImmutableAddr::from_raw(address)
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
    ) -> [RawStorageBuffer; MAX_FRAMES_IN_FLIGHT] {
        self.0[handle.index].take().unwrap()
    }

    // GPU-only buffers also share this storage; their handle type is what
    // keeps them out of Gpu's per-frame CPU write methods.

    pub fn add_gpu_only<T>(
        &mut self,
        buffers_per_frame: [RawStorageBuffer; MAX_FRAMES_IN_FLIGHT],
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
    ) -> [RawStorageBuffer; MAX_FRAMES_IN_FLIGHT] {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<[RawStorageBuffer; MAX_FRAMES_IN_FLIGHT]> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

/// byte offset of element `index` in a buffer of `len` elements of `stride` bytes
///
/// `assert!`, not `debug_assert!` — deliberately unlike the neighbouring bounds
/// check in `queue_draw_index_range`. That one is debug-only because a bad index
/// range renders garbage silently under `robustBufferAccess`; robust access
/// covers descriptor-bound buffers and does *not* cover buffer device addresses,
/// which bypass descriptor bounds checking entirely. An out-of-range element
/// address is undefined behaviour and a plausible device loss, not a clamped read.
fn element_byte_offset(index: u32, len: u32, stride: usize) -> u64 {
    assert!(
        index < len,
        "element index {index} out of bounds for buffer of {len} element(s)"
    );

    index as u64 * stride as u64
}

#[cfg(test)]
mod tests {
    use super::element_byte_offset;

    #[test]
    fn element_offsets_are_index_times_stride() {
        // a non-power-of-two stride, so a shift-vs-multiply mistake would show
        assert_eq!(element_byte_offset(0, 4, 24), 0);
        assert_eq!(element_byte_offset(1, 4, 24), 24);
        assert_eq!(element_byte_offset(3, 4, 24), 72);
    }

    /// not `#[should_panic]` behind `debug_assertions`: the bound must hold in
    /// release too, because robustBufferAccess does not cover BDA loads
    #[test]
    #[should_panic(expected = "element index 4 out of bounds for buffer of 4 element(s)")]
    fn one_past_the_end_panics() {
        element_byte_offset(4, 4, 24);
    }
}
