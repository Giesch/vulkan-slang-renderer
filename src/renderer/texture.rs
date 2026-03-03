use ash::vk;

#[derive(Debug)]
pub struct TextureHandle {
    #[expect(unused)] // for debugging
    #[cfg(debug_assertions)]
    source_file_name: String,
    index: usize,
}

pub(super) struct TextureStorage(Vec<Option<Texture>>);

impl TextureStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add(&mut self, texture: Texture) -> TextureHandle {
        let handle = TextureHandle {
            #[cfg(debug_assertions)]
            source_file_name: texture.source_file_name.clone(),
            index: self.0.len(),
        };
        self.0.push(Some(texture));

        handle
    }

    pub fn get(&self, handle: &TextureHandle) -> &Texture {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn take(&mut self, handle: TextureHandle) -> Texture {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<Texture> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

pub(super) struct Texture {
    #[cfg_attr(not(debug_assertions), expect(unused))]
    pub(super) source_file_name: String,
    pub(super) image: vk::Image,
    pub(super) image_memory: vk::DeviceMemory,
    pub(super) image_view: vk::ImageView,
    pub(super) sampler: vk::Sampler,
    #[expect(unused)] // currently unused after init
    pub(super) mip_levels: u32,
    pub(super) image_layout: vk::ImageLayout,
}
