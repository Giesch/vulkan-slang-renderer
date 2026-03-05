use ash::vk;

#[derive(Debug)]
pub struct StorageTextureHandle {
    pub(super) index: usize,
}

pub(super) struct StorageTextureStorage(Vec<StorageTexture>);

impl StorageTextureStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add(&mut self, texture: StorageTexture) -> StorageTextureHandle {
        let handle = StorageTextureHandle {
            index: self.0.len(),
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
    pub(super) image_memory: vk::DeviceMemory,
    pub(super) image_view: vk::ImageView,
    pub(super) format: vk::Format,
    pub(super) width: u32,
    pub(super) height: u32,
}
