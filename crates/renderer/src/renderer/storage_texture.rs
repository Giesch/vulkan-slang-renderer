use ash::vk;

use super::bindless::{BindlessHandle, RwTexture2D};
use super::descriptor_heap::BindlessIndex;

#[derive(Debug)]
pub struct StorageTextureHandle {
    pub(super) index: usize,
    /// This texture's slot in the bindless heap, distinct from `index`.
    bindless_slot: BindlessIndex,
}

impl StorageTextureHandle {
    pub fn bindless_handle(&self) -> BindlessHandle<RwTexture2D> {
        BindlessHandle::from_slot(self.bindless_slot)
    }
}

pub(super) struct StorageTextureStorage(Vec<StorageTexture>);

impl StorageTextureStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add(
        &mut self,
        texture: StorageTexture,
        bindless_slot: BindlessIndex,
    ) -> StorageTextureHandle {
        let handle = StorageTextureHandle {
            index: self.0.len(),
            bindless_slot,
        };
        self.0.push(texture);

        handle
    }

    pub fn get(&self, handle: &StorageTextureHandle) -> &StorageTexture {
        &self.0[handle.index]
    }

    pub fn take_all(&mut self) -> Vec<StorageTexture> {
        std::mem::take(&mut self.0)
    }
}

pub(super) struct StorageTexture {
    pub(super) image: vk::Image,
    pub(super) image_memory: vk_mem::Allocation,
    pub(super) image_view: vk::ImageView,
    pub(super) format: vk::Format,
    pub(super) width: u32,
    pub(super) height: u32,
}
