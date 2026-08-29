//! The bindless texture heap set up by Slang
//!
//! Slang assigns the binding numbers and places every heap in one descriptor set.
//! Vulkan permits `VARIABLE_DESCRIPTOR_COUNT` only on the highest-numbered binding
//! in a set, so each heap has a fixed descriptor count.
//!
//! Textures are never freed.

use ash::vk;

use super::texture::Texture;

/// Total slots available in the texture heap.
///
/// This needs to be fixed, rather than variable-count. It also needs to be
/// small enough for MoltenVK, which implements descriptor indexing on
/// Metal argument buffers with much lower limits than desktop Vulkan.
pub(super) const MAX_BINDLESS_TEXTURES: u32 = 4096;

/// Slang sets the heap binding numbers. Under the `None` bindless preset they
/// are 0 sampler, 1 combined image sampler, 2 sampled image, 3 storage image.
/// The shader compiler pins that preset in `load_bindless_options_module`.
const COMBINED_IMAGE_SAMPLER_BINDING: u32 = 1;
const STORAGE_IMAGE_BINDING: u32 = 3;

/// A texture's slot in the heap.
/// Distinct from its `TextureStorage` slab index, although they're both monotonic today.
#[derive(Debug, Clone, Copy)]
pub(super) struct BindlessIndex(u32);

impl BindlessIndex {
    pub(super) fn to_raw(self) -> u32 {
        self.0
    }
}

pub(super) struct DescriptorHeap {
    layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    next_texture_slot: u32,
    next_storage_image_slot: u32,
}

impl DescriptorHeap {
    /// The device-suitability gate has already rejected any device whose
    /// update-after-bind limits can't hold `MAX_BINDLESS_TEXTURES`; see
    /// [`renderer::undersized_limits`].
    pub(super) fn new(device: &ash::Device) -> anyhow::Result<Self> {
        // one entry per binding, and `bindings` and `binding_flags` must stay
        // parallel: Vulkan requires bindingCount == pBindingFlags.len()
        let bindings = [
            vk::DescriptorSetLayoutBinding::default()
                .binding(COMBINED_IMAGE_SAMPLER_BINDING)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(MAX_BINDLESS_TEXTURES)
                // matches reflected global bindings
                .stage_flags(vk::ShaderStageFlags::ALL),
            vk::DescriptorSetLayoutBinding::default()
                .binding(STORAGE_IMAGE_BINDING)
                .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(MAX_BINDLESS_TEXTURES)
                .stage_flags(vk::ShaderStageFlags::ALL),
        ];

        // PARTIALLY_BOUND to make the unwritten tail of a fixed-size array legal
        // UPDATE_AFTER_BIND to let one set serve every frame in flight
        let bindless_binding_flags = vk::DescriptorBindingFlags::PARTIALLY_BOUND
            | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND
            | vk::DescriptorBindingFlags::UPDATE_UNUSED_WHILE_PENDING;
        // duplicated for each bindless heap
        let binding_flags = [bindless_binding_flags, bindless_binding_flags];
        let mut binding_flags_info =
            vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&binding_flags);

        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
            .bindings(&bindings)
            .push_next(&mut binding_flags_info);
        let layout = unsafe { device.create_descriptor_set_layout(&layout_info, None)? };

        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(MAX_BINDLESS_TEXTURES),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_IMAGE)
                .descriptor_count(MAX_BINDLESS_TEXTURES),
        ];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
            .pool_sizes(&pool_sizes)
            .max_sets(1);
        let pool = unsafe { device.create_descriptor_pool(&pool_info, None)? };

        let set_layouts = [layout];
        let alloc_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&set_layouts);
        let set = unsafe { device.allocate_descriptor_sets(&alloc_info)?[0] };

        Ok(Self {
            layout,
            pool,
            set,
            next_texture_slot: 0,
            next_storage_image_slot: 0,
        })
    }

    /// Appended to the pipeline layout of every shader that declares a handle.
    /// Owned here, not by the pipeline that borrows it.
    pub(super) fn layout(&self) -> vk::DescriptorSetLayout {
        self.layout
    }

    /// Only one set shared by every frame in flight;
    /// update-after-bind is what makes that safe.
    pub(super) fn set(&self) -> vk::DescriptorSet {
        self.set
    }

    /// Claim the next slot and write the texture's descriptor into it.
    pub(super) fn insert_texture(
        &mut self,
        device: &ash::Device,
        texture: &Texture,
    ) -> anyhow::Result<BindlessIndex> {
        anyhow::ensure!(
            self.next_texture_slot < MAX_BINDLESS_TEXTURES,
            "bindless texture heap is full ({MAX_BINDLESS_TEXTURES} slots, \
             and slots are never released)"
        );
        let slot = self.next_texture_slot;
        self.next_texture_slot += 1;

        // not SHADER_READ_ONLY_OPTIMAL: sampled aliases of storage textures
        // live in GENERAL
        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(texture.image_layout)
            .image_view(texture.image_view)
            .sampler(texture.sampler)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(COMBINED_IMAGE_SAMPLER_BINDING)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        Ok(BindlessIndex(slot))
    }

    pub(super) fn insert_storage_image(
        &mut self,
        device: &ash::Device,
        image_view: vk::ImageView,
    ) -> anyhow::Result<BindlessIndex> {
        anyhow::ensure!(
            self.next_storage_image_slot < MAX_BINDLESS_TEXTURES,
            "bindless storage image heap is full ({MAX_BINDLESS_TEXTURES} slots, \
             and slots are never released)"
        );
        let slot = self.next_storage_image_slot;
        self.next_storage_image_slot += 1;

        // every storage texture is created in GENERAL and stays there
        let image_info = [vk::DescriptorImageInfo::default()
            .image_layout(vk::ImageLayout::GENERAL)
            .image_view(image_view)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(STORAGE_IMAGE_BINDING)
            .dst_array_element(slot)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(&image_info)];
        unsafe { device.update_descriptor_sets(&writes, &[]) };

        Ok(BindlessIndex(slot))
    }

    /// Destroying the pool frees the set allocated from it.
    pub(super) fn destroy(&self, device: &ash::Device) {
        unsafe {
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.layout, None);
        }
    }
}
