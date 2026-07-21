#![allow(clippy::type_complexity, clippy::too_many_arguments)]

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_char};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use ash::vk;
use glam::Vec2;
use sdl3::sys::vulkan::SDL_Vulkan_DestroySurface;
use sdl3::video::Window;
use vk_mem::Alloc as _;

use crate::game::MaxMSAASamples;
use crate::shaders;
use crate::shaders::atlas::{ComputeShaderAtlasEntry, PrecompiledShader, ShaderAtlasEntry};

#[cfg(debug_assertions)]
use crate::shader_watcher;
use crate::shaders::json::ReflectedDescriptorSetLayout;
#[cfg(debug_assertions)]
use log::*;

pub mod debug;
mod platform;

pub mod gpu_write;
use gpu_write::{GPUWrite, write_to_gpu_buffer};

pub mod vertex_description;
use vertex_description::VertexDescription;

pub mod texture;
pub use texture::*;

pub mod uniform_buffer;
pub use uniform_buffer::*;

pub mod storage_buffer;
pub use storage_buffer::*;

pub mod addr;
pub use addr::*;

pub mod storage_texture;
pub use storage_texture::*;

pub mod pipeline;
pub use pipeline::*;

pub mod egui;
pub use egui::EguiIntegration;

pub mod facet_egui;

mod picking;
use picking::PickingResources;

/// enables both the validation layer and debug utils logging
const ENABLE_VALIDATION: bool = cfg!(debug_assertions);
/// applies MSAA-like sampling within textures
const ENABLE_SAMPLE_SHADING: bool = false;

/// Max GPU frames executing concurrently. Each frame blocks in wait_semaphores
/// until frame_timeline reaches N - MAX_FRAMES_IN_FLIGHT, and slots indexed by
/// `flight_slot` are only touched after that wait, which guards their reuse.
/// (Compute command buffers get an equivalent wait on compute_timeline.)
const MAX_FRAMES_IN_FLIGHT: usize = 2;
/// Ring length for slots touched before the frame_timeline wait (CPU buffer
/// writes, swapchain acquire) or read one frame late (ping-pong history).
/// Pre-wait, only frame N - 3 is proven retired — by the previous frame's
/// wait — so the ring needs one slot more than MAX_FRAMES_IN_FLIGHT.
const PRE_WAIT_RING_LEN: usize = MAX_FRAMES_IN_FLIGHT + 1;

/// the subresource range of a single-mip color image
const COLOR_SUBRESOURCE_RANGE: vk::ImageSubresourceRange = vk::ImageSubresourceRange {
    aspect_mask: vk::ImageAspectFlags::COLOR,
    base_mip_level: 0,
    level_count: 1,
    base_array_layer: 0,
    layer_count: 1,
};

pub struct Renderer {
    // fields that are created once
    aspect_ratio: f32,
    width: f32,
    height: f32,
    total_frames: usize,
    #[cfg(debug_assertions)]
    shader_changes: shader_watcher::ShaderChanges,
    #[cfg(debug_assertions)]
    old_pipelines: Vec<(
        usize,
        vk::Pipeline,
        ash::vk::PipelineLayout,
        Vec<ash::vk::DescriptorSetLayout>,
    )>,
    #[expect(unused)]
    entry: ash::Entry,
    window: Window,
    instance: ash::Instance,
    debug_ext: vk::DebugUtilsMessengerEXT,
    surface_ext: ash::khr::surface::Instance,
    debug_loader: ash::ext::debug_utils::Instance,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    physical_device_properties: vk::PhysicalDeviceProperties,
    queue_family_indices: QueueFamilyIndices,
    device: ash::Device,
    /// ManuallyDrop because Renderer has a manual Drop impl: the allocator must be
    /// destroyed after all buffers/images are freed but before destroy_device.
    allocator: std::mem::ManuallyDrop<vk_mem::Allocator>,
    debug_utils_device: ash::ext::debug_utils::Device,
    graphics_queue: vk::Queue,
    presentation_queue: vk::Queue,
    swapchain_device_ext: ash::khr::swapchain::Device,
    msaa_samples: vk::SampleCountFlags,

    // fields that change, at least in theory
    image_format: vk::Format,
    image_extent: vk::Extent2D,
    swapchain: vk::SwapchainKHR,
    swapchain_images: Vec<vk::Image>,
    swapchain_image_views: Vec<vk::ImageView>,
    depth_format: vk::Format,
    color_image: vk::Image,
    color_image_memory: vk_mem::Allocation,
    color_image_view: vk::ImageView,
    depth_image: vk::Image,
    depth_image_memory: vk_mem::Allocation,
    depth_image_view: vk::ImageView,

    /// resolve images for upscaling, indexed by flight_slot
    render_scale: f32,
    render_extent: vk::Extent2D,
    resolve_images: [vk::Image; MAX_FRAMES_IN_FLIGHT],
    resolve_image_memories: [vk_mem::Allocation; MAX_FRAMES_IN_FLIGHT],
    resolve_image_views: [vk::ImageView; MAX_FRAMES_IN_FLIGHT],

    command_pool: vk::CommandPool,
    command_buffers: [vk::CommandBuffer; MAX_FRAMES_IN_FLIGHT],
    /// image semaphores indexed by ring_slot
    image_available: [vk::Semaphore; PRE_WAIT_RING_LEN],
    /// render finished semaphores indexed by image_index
    /// ie, one per swapchain image, not per frame-in-flight
    render_finished: Vec<vk::Semaphore>,
    /// timeline semaphore: the graphics submit for frame N signals value N (= total_frames)
    frame_timeline: vk::Semaphore,
    /// looping index for wait-guarded per-flight resources:
    ///   command buffers (graphics + compute), resolve images,
    ///   picking readback, egui texture frees
    /// (0..MAX_FRAMES_IN_FLIGHT)
    flight_slot: usize,
    /// looping index for the pre-wait ring:
    ///   per-frame buffers, acquire semaphores, and the descriptor
    ///   sets that reference each slot's buffers
    /// (0..PRE_WAIT_RING_LEN)
    ring_slot: usize,

    /// timeline semaphore: the k-th compute-signaling submit signals value k
    compute_timeline: vk::Semaphore,
    /// number of submits so far that signal compute_timeline (its highest pending value)
    compute_frames: u64,
    has_compute_pipelines: bool,

    /// Dedicated compute queue for async compute (None = single-queue fallback)
    compute_queue: Option<vk::Queue>,
    /// Command buffers for pipelined compute dispatches
    compute_command_buffers: [vk::CommandBuffer; MAX_FRAMES_IN_FLIGHT],
    /// Use pipelined compute submission; set once during Game::setup via
    /// Renderer::enable_pipelined_compute
    pipelined_compute: bool,

    pipelines: PipelineStorage,
    compute_pipelines: ComputePipelineStorage,
    textures: TextureStorage,
    storage_textures: StorageTextureStorage,
    uniform_buffers: UniformBufferStorage,
    storage_buffers: StorageBufferStorage,

    egui: Option<EguiIntegration>,
    text_input_active: bool,

    picking: Option<PickingResources>,
    last_picked_object_id: u32,
}

fn calculate_render_extent(display_extent: vk::Extent2D, render_scale: f32) -> vk::Extent2D {
    vk::Extent2D {
        width: ((display_extent.width as f32 * render_scale) as u32).max(1),
        height: ((display_extent.height as f32 * render_scale) as u32).max(1),
    }
}

fn create_resolve_images(
    allocator: &vk_mem::Allocator,
    device: &ash::Device,
    render_extent: vk::Extent2D,
    color_format: vk::Format,
) -> Result<
    (
        [vk::Image; MAX_FRAMES_IN_FLIGHT],
        [vk_mem::Allocation; MAX_FRAMES_IN_FLIGHT],
        [vk::ImageView; MAX_FRAMES_IN_FLIGHT],
    ),
    anyhow::Error,
> {
    let image_options = ImageOptions {
        extent: render_extent,
        format: color_format,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::TRANSFER_SRC | vk::ImageUsageFlags::COLOR_ATTACHMENT,
        mip_levels: 1,
        msaa_samples: vk::SampleCountFlags::TYPE_1,
    };

    let results: [_; MAX_FRAMES_IN_FLIGHT] = (0..MAX_FRAMES_IN_FLIGHT)
        .map(|_| -> anyhow::Result<_> {
            let (image, memory) = create_vk_image(allocator, image_options)?;
            let view =
                create_image_view(device, image, color_format, vk::ImageAspectFlags::COLOR, 1)?;
            Ok((image, memory, view))
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .unwrap();

    let images = results.map(|(image, _, _)| image);
    let memories = results.map(|(_, memory, _)| memory);
    let views = results.map(|(_, _, view)| view);

    Ok((images, memories, views))
}

impl Renderer {
    pub fn init(
        window: Window,
        enable_egui: bool,
        render_scale: f32,
        max_msaa_samples: MaxMSAASamples,
    ) -> Result<Self, anyhow::Error> {
        let render_scale = render_scale.clamp(0.25, 1.0);
        #[cfg(debug_assertions)]
        let shader_changes = shader_watcher::watch()?;

        let (window_width, window_height) = window.size();
        let aspect_ratio = window_width as f32 / window_height as f32;

        let entry = ash::Entry::linked();

        check_required_extensions(&entry)?;
        check_required_layers(&entry)?;

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"Vulkan Tutorial")
            .engine_name(c"No Engine")
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_3);

        let mut enabled_extension_names = vec![];
        let window_required_extensions: Vec<_> = window
            .vulkan_instance_extensions()?
            .into_iter()
            .map(|s| CString::new(s).unwrap())
            .collect();
        for name in &window_required_extensions {
            enabled_extension_names.push(name.as_ptr())
        }
        enabled_extension_names.push(ash::ext::debug_utils::NAME.as_ptr());

        for platform_instance_ext in platform::ADDITIONAL_INSTANCE_EXTENSIONS {
            enabled_extension_names.push(platform_instance_ext.as_ptr());
        }

        let create_flags = platform::instance_create_flags();

        let mut enabled_layer_names = vec![];
        for layer_name in get_required_layers() {
            enabled_layer_names.push(layer_name.as_ptr())
        }

        let mut create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_layer_names(&enabled_layer_names)
            .enabled_extension_names(&enabled_extension_names)
            .flags(create_flags);
        let mut debug_create_info = debug::build_messenger_create_info();
        if ENABLE_VALIDATION {
            create_info = create_info.push_next(&mut debug_create_info);
        }

        let instance = unsafe { entry.create_instance(&create_info, None)? };
        let (debug_loader, debug_ext) =
            debug::maybe_create_debug_messager_extension(&entry, &instance, &debug_create_info);

        let surface_ext = ash::khr::surface::Instance::new(&entry, &instance);

        let surface = window.vulkan_create_surface(instance.handle())?;

        let (physical_device, queue_family_indices, physical_device_properties) =
            choose_physical_device(&instance, &surface_ext, surface)?;
        let device = create_logical_device(&instance, physical_device, &queue_family_indices)?;
        let debug_utils_device = ash::ext::debug_utils::Device::new(&instance, &device);

        let allocator = {
            let mut allocator_create_info =
                vk_mem::AllocatorCreateInfo::new(&instance, &device, physical_device);
            allocator_create_info.vulkan_api_version = vk::API_VERSION_1_3;
            // matches the buffer_device_address feature enabled at device creation
            allocator_create_info.flags |= vk_mem::AllocatorCreateFlags::BUFFER_DEVICE_ADDRESS;
            std::mem::ManuallyDrop::new(unsafe { vk_mem::Allocator::new(allocator_create_info)? })
        };

        let msaa_samples =
            get_max_usable_sample_count(physical_device_properties, max_msaa_samples);

        let graphics_queue = unsafe { device.get_device_queue(queue_family_indices.graphics, 0) };
        let presentation_queue =
            unsafe { device.get_device_queue(queue_family_indices.presentation, 0) };
        let compute_queue = if queue_family_indices.graphics_queue_count >= 2 {
            Some(unsafe { device.get_device_queue(queue_family_indices.graphics, 1) })
        } else {
            None
        };

        let swapchain_device_ext = ash::khr::swapchain::Device::new(&instance, &device);
        let CreatedSwapchain {
            swapchain,
            image_format,
            image_extent,
        } = create_swapchain(
            &window,
            &swapchain_device_ext,
            &surface_ext,
            surface,
            physical_device,
            &queue_family_indices,
        )?;

        let swapchain_images = unsafe { swapchain_device_ext.get_swapchain_images(swapchain)? };
        let swapchain_image_views =
            create_swapchain_image_views(&device, image_format, &swapchain_images)?;
        let (image_available, render_finished, frame_timeline, compute_timeline) =
            create_sync_objects(&device, &swapchain_images)?;

        let depth_format = find_depth_format(&instance, physical_device);

        let egui = if enable_egui {
            Some(EguiIntegration::new(
                &instance,
                physical_device,
                device.clone(),
                image_format,
            )?)
        } else {
            None
        };

        let command_pool = create_command_pool(&device, &queue_family_indices)?;
        let command_buffers = create_command_buffers(&device, command_pool)?;
        let compute_command_buffers = create_command_buffers(&device, command_pool)?;

        // Calculate scaled render extent
        let render_extent = calculate_render_extent(image_extent, render_scale);

        // Create resolve images at render_extent
        let (resolve_images, resolve_image_memories, resolve_image_views) =
            create_resolve_images(&allocator, &device, render_extent, image_format)?;

        // Color and depth buffers at render_extent (scaled resolution)
        let (color_image, color_image_memory, color_image_view) = create_color_image(
            &allocator,
            &device,
            render_extent,
            image_format,
            msaa_samples,
        )?;

        let (depth_image, depth_image_memory, depth_image_view) = create_depth_buffer_image(
            &allocator,
            &instance,
            &device,
            physical_device,
            command_pool,
            graphics_queue,
            render_extent,
            msaa_samples,
        )?;

        let pipelines = PipelineStorage::new();
        let compute_pipelines = ComputePipelineStorage::new();
        let textures = TextureStorage::new();
        let uniform_buffers = UniformBufferStorage::new();
        let storage_buffers = StorageBufferStorage::new();

        Ok(Self {
            aspect_ratio,
            width: window_width as f32,
            height: window_height as f32,
            total_frames: 0,
            #[cfg(debug_assertions)]
            shader_changes,
            #[cfg(debug_assertions)]
            old_pipelines: vec![],
            window: window.clone(),
            entry,
            instance,
            debug_ext,
            surface_ext,
            debug_loader,
            surface,
            physical_device,
            physical_device_properties,
            queue_family_indices,
            device,
            allocator,
            debug_utils_device,
            graphics_queue,
            presentation_queue,
            swapchain_device_ext,
            msaa_samples,
            image_format,
            image_extent,
            swapchain,
            swapchain_images,
            swapchain_image_views,
            depth_format,
            color_image,
            color_image_memory,
            color_image_view,
            depth_image,
            depth_image_memory,
            depth_image_view,
            render_scale,
            render_extent,
            resolve_images,
            resolve_image_memories,
            resolve_image_views,
            command_pool,
            command_buffers,
            image_available,
            render_finished,
            frame_timeline,
            flight_slot: 0,
            ring_slot: 0,

            compute_timeline,
            compute_frames: 0,
            has_compute_pipelines: false,
            compute_queue,
            compute_command_buffers,
            pipelined_compute: false,

            pipelines,
            compute_pipelines,
            textures,
            storage_textures: StorageTextureStorage::new(),
            uniform_buffers,
            storage_buffers,
            egui,
            picking: None,
            last_picked_object_id: 0,
            text_input_active: false,
        })
    }

    fn renderer_pipeline<D>(&self, handle: &PipelineHandle<D>) -> &RendererPipeline {
        self.pipelines.get(handle)
    }

    fn set_debug_name<T: vk::Handle>(&self, object: T, name: &str) {
        let c_name = CString::new(name).unwrap();
        let name_info = vk::DebugUtilsObjectNameInfoEXT::default()
            .object_handle(object)
            .object_name(&c_name);
        unsafe {
            self.debug_utils_device
                .set_debug_utils_object_name(&name_info)
                .ok();
        }
    }

    pub fn create_texture(
        &mut self,
        source_file_name: impl Into<String>,
        image: &image::DynamicImage,
        texture_filter: TextureFilter,
    ) -> anyhow::Result<TextureHandle> {
        let texture = create_texture(
            source_file_name.into(),
            image,
            &self.allocator,
            &self.instance,
            &self.device,
            self.physical_device,
            self.physical_device_properties,
            self.command_pool,
            self.graphics_queue,
            texture_filter,
        )?;

        let handle = self.textures.add(texture);

        Ok(handle)
    }

    /// Create a texture from pre-baked mip level data, such as from a KTX2 file.
    /// Unlike [`Self::create_texture`], this uploads all provided mip levels
    /// directly instead of generating them at runtime.
    ///
    /// `mip_data` must contain one entry per mip level, level 0 (largest) first;
    /// the extent of level `i` is derived as `(extent >> i).max(1)`.
    pub fn create_texture_with_mips(
        &mut self,
        source_file_name: impl Into<String>,
        format: vk::Format,
        extent: vk::Extent2D,
        mip_data: &[&[u8]],
        texture_filter: TextureFilter,
    ) -> anyhow::Result<TextureHandle> {
        let texture = create_texture_from_mips(
            source_file_name.into(),
            format,
            extent,
            mip_data,
            &self.allocator,
            &self.instance,
            &self.device,
            self.physical_device,
            self.physical_device_properties,
            self.command_pool,
            self.graphics_queue,
            texture_filter,
        )?;

        let handle = self.textures.add(texture);

        Ok(handle)
    }

    pub fn drop_texture(&mut self, texture_handle: TextureHandle) {
        let texture = self.textures.take(texture_handle);
        self.destroy_texture(texture);
    }

    fn destroy_texture(&mut self, texture: Texture) {
        unsafe {
            self.device.destroy_sampler(texture.sampler, None);
            self.device.destroy_image_view(texture.image_view, None);
            if let texture::ImageOwnership::Owned(mut allocation) = texture.image_ownership {
                self.allocator.destroy_image(texture.image, &mut allocation);
            }
        }
    }

    pub fn create_storage_texture(
        &mut self,
        width: u32,
        height: u32,
        format: vk::Format,
    ) -> anyhow::Result<StorageTextureHandle> {
        let (image, image_memory) = create_vk_image(
            &self.allocator,
            ImageOptions {
                extent: vk::Extent2D { width, height },
                format,
                tiling: vk::ImageTiling::OPTIMAL,
                usage: vk::ImageUsageFlags::STORAGE
                    | vk::ImageUsageFlags::SAMPLED
                    | vk::ImageUsageFlags::TRANSFER_DST,
                mip_levels: 1,
                msaa_samples: vk::SampleCountFlags::TYPE_1,
            },
        )?;

        transition_image_layout(
            &self.device,
            self.command_pool,
            self.graphics_queue,
            image,
            format,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::GENERAL,
            1,
        )?;

        let image_view =
            create_image_view(&self.device, image, format, vk::ImageAspectFlags::COLOR, 1)?;

        let storage_texture = storage_texture::StorageTexture {
            image,
            image_memory,
            image_view,
            format,
            width,
            height,
        };

        let handle = self.storage_textures.add(storage_texture);

        Ok(handle)
    }

    /// Create a sampled TextureHandle that aliases the same image as the given storage texture.
    /// The image stays in GENERAL layout (valid for both storage and sampled access).
    pub fn storage_texture_as_sampled(
        &mut self,
        storage_texture_handle: &StorageTextureHandle,
    ) -> anyhow::Result<TextureHandle> {
        let st = self.storage_textures.get(storage_texture_handle);

        let image_view = create_image_view(
            &self.device,
            st.image,
            st.format,
            vk::ImageAspectFlags::COLOR,
            1,
        )?;

        let sampler = create_texture_sampler(
            &self.device,
            self.physical_device_properties,
            TextureFilter::Linear,
        )?;

        let texture = texture::Texture {
            source_file_name: "storage_texture_sampled_alias".to_string(),
            image: st.image,
            image_ownership: texture::ImageOwnership::Aliased,
            image_view,
            sampler,
            mip_levels: 1,
            image_layout: vk::ImageLayout::GENERAL,
        };

        let handle = self.textures.add(texture);

        Ok(handle)
    }

    pub fn clear_storage_texture(&self, handle: &StorageTextureHandle) -> anyhow::Result<()> {
        let st = self.storage_textures.get(handle);

        let command_buffer = begin_single_time_commands(&self.device, self.command_pool)?;

        let subresource_range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);

        unsafe {
            self.device.cmd_clear_color_image(
                command_buffer,
                st.image,
                vk::ImageLayout::GENERAL,
                &vk::ClearColorValue {
                    float32: Default::default(),
                },
                &[subresource_range],
            );
        }

        end_single_time_commands(
            &self.device,
            self.command_pool,
            self.graphics_queue,
            command_buffer,
        )?;

        Ok(())
    }

    /// Upload CPU data to a storage texture via a staging buffer.
    /// The texture must already be in GENERAL layout (as created by `create_storage_texture`).
    pub fn write_storage_texture<T: GPUWrite>(
        &self,
        handle: &StorageTextureHandle,
        data: &[T],
    ) -> anyhow::Result<()> {
        let st = self.storage_textures.get(handle);
        let buffer_size = std::mem::size_of_val(data) as u64;

        let (staging_buffer, mut staging_buffer_memory) = create_memory_buffer(
            &self.allocator,
            buffer_size,
            vk::BufferUsageFlags::TRANSFER_SRC,
            BufferMemory::Staging,
        )?;

        unsafe { write_to_gpu_buffer(&self.allocator, &mut staging_buffer_memory, data)? };

        let command_buffer = begin_single_time_commands(&self.device, self.command_pool)?;

        // Transition GENERAL -> TRANSFER_DST_OPTIMAL
        let subresource_range = vk::ImageSubresourceRange::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .level_count(1)
            .layer_count(1);

        let barrier_to_transfer = vk::ImageMemoryBarrier2::default()
            .old_layout(vk::ImageLayout::GENERAL)
            .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(st.image)
            .subresource_range(subresource_range)
            .src_stage_mask(vk::PipelineStageFlags2::NONE)
            .src_access_mask(vk::AccessFlags2::NONE)
            .dst_stage_mask(vk::PipelineStageFlags2::COPY)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE);

        cmd_barrier2(&self.device, command_buffer, &[barrier_to_transfer]);

        // Copy buffer to image
        let image_subresource = vk::ImageSubresourceLayers::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(0)
            .base_array_layer(0)
            .layer_count(1);

        let extent = vk::Extent2D {
            width: st.width,
            height: st.height,
        };

        let region = vk::BufferImageCopy::default()
            .buffer_offset(0)
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(image_subresource)
            .image_offset(vk::Offset3D::default())
            .image_extent(extent.into());

        unsafe {
            self.device.cmd_copy_buffer_to_image(
                command_buffer,
                staging_buffer,
                st.image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
        }

        // Transition TRANSFER_DST_OPTIMAL -> GENERAL
        let barrier_to_general = vk::ImageMemoryBarrier2::default()
            .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
            .new_layout(vk::ImageLayout::GENERAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(st.image)
            .subresource_range(subresource_range)
            .src_stage_mask(vk::PipelineStageFlags2::COPY)
            .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
            .dst_access_mask(vk::AccessFlags2::SHADER_READ);

        cmd_barrier2(&self.device, command_buffer, &[barrier_to_general]);

        end_single_time_commands(
            &self.device,
            self.command_pool,
            self.graphics_queue,
            command_buffer,
        )?;

        unsafe {
            self.allocator
                .destroy_buffer(staging_buffer, &mut staging_buffer_memory);
        }

        Ok(())
    }

    pub fn create_uniform_buffer<T: GPUWrite>(&mut self) -> anyhow::Result<UniformBufferHandle<T>> {
        let buffer_size = std::mem::size_of::<T>() as u64;

        let mut buffers_per_frame: [Option<RawUniformBuffer>; PRE_WAIT_RING_LEN] =
            [const { None }; PRE_WAIT_RING_LEN];
        #[expect(clippy::needless_range_loop)]
        for i in 0..PRE_WAIT_RING_LEN {
            let (buffer, allocation) = create_memory_buffer(
                &self.allocator,
                buffer_size,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                BufferMemory::PersistentlyMapped,
            )?;

            let mapped_mem = self.allocator.get_allocation_info(&allocation).mapped_data;

            buffers_per_frame[i] = Some(RawUniformBuffer {
                buffer,
                allocation,
                mapped_mem,
            });
        }
        let buffers_per_frame = buffers_per_frame.map(Option::unwrap);

        let handle = self.uniform_buffers.add(buffers_per_frame);

        Ok(handle)
    }

    pub fn drop_uniform_buffer<T>(&mut self, uniform_buffer: UniformBufferHandle<T>) {
        let buffers_per_frame = self.uniform_buffers.take(uniform_buffer);
        for raw_uniform_buffer in buffers_per_frame {
            self.destroy_uniform_buffer(raw_uniform_buffer);
        }
    }

    fn destroy_uniform_buffer(&mut self, mut uniform_buffer: RawUniformBuffer) {
        unsafe {
            self.allocator
                .destroy_buffer(uniform_buffer.buffer, &mut uniform_buffer.allocation);
        }
    }

    pub fn create_storage_buffer<T: GPUWrite>(
        &mut self,
        len: u32,
    ) -> anyhow::Result<StorageBufferHandle<T>> {
        let buffers_per_frame = self.create_storage_buffers_per_frame::<T>(len)?;
        Ok(self.storage_buffers.add(buffers_per_frame, len))
    }

    pub fn create_immutable_buffer<T: GPUWrite>(
        &mut self,
        len: u32,
    ) -> anyhow::Result<ImmutableBufferHandle<T>> {
        let buffers_per_frame = self.create_storage_buffers_per_frame::<T>(len)?;
        Ok(self.storage_buffers.add_immutable(buffers_per_frame, len))
    }

    pub fn create_gpu_only_buffer<T: GPUWrite>(
        &mut self,
        len: u32,
    ) -> anyhow::Result<GpuOnlyBufferHandle<T>> {
        let buffers_per_frame = self.create_storage_buffers_per_frame::<T>(len)?;
        Ok(self.storage_buffers.add_gpu_only(buffers_per_frame, len))
    }

    /// Enable pipelined async compute; call during Game::setup.
    /// Compute dispatches are submitted separately so they can run concurrently
    /// with the previous frame's graphics, on a dedicated compute queue when one
    /// exists (single-queue fallback otherwise).
    ///
    /// In this mode a frame's graphics submit does NOT wait for that frame's
    /// compute — graphics shaders must read compute output via
    /// `Gpu::previous_addr`, never `Gpu::current_gpu_only_addr`.
    /// TODO: make the warning above unnecessary
    pub fn enable_pipelined_compute(&mut self) {
        self.pipelined_compute = true;
    }

    fn create_storage_buffers_per_frame<T: GPUWrite>(
        &mut self,
        len: u32,
    ) -> anyhow::Result<[RawStorageBuffer; PRE_WAIT_RING_LEN]> {
        let buffer_size = (len as usize * std::mem::size_of::<T>()) as u64;

        let mut buffers_per_frame: [Option<RawStorageBuffer>; PRE_WAIT_RING_LEN] =
            [const { None }; PRE_WAIT_RING_LEN];
        #[expect(clippy::needless_range_loop)]
        for i in 0..PRE_WAIT_RING_LEN {
            let (buffer, allocation) = create_memory_buffer(
                &self.allocator,
                buffer_size,
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
                BufferMemory::PersistentlyMapped,
            )?;

            let mapped_mem = self.allocator.get_allocation_info(&allocation).mapped_data;

            let device_address = unsafe {
                self.device.get_buffer_device_address(
                    &vk::BufferDeviceAddressInfo::default().buffer(buffer),
                )
            };
            debug_assert_ne!(device_address, 0);

            buffers_per_frame[i] = Some(RawStorageBuffer {
                buffer,
                allocation,
                mapped_mem,
                device_address,
            });
        }
        Ok(buffers_per_frame.map(Option::unwrap))
    }

    pub fn write_storage_all_frames<T>(&mut self, buf: &mut StorageBufferHandle<T>, data: &[T]) {
        let len = data.len().min(buf.len() as usize);
        for frame in 0..PRE_WAIT_RING_LEN {
            let mapped = self.storage_buffers.get_mapped_mem_for_frame(buf, frame);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped, len);
            }
        }
    }

    pub fn write_immutable_all_frames<T>(
        &mut self,
        buf: &mut ImmutableBufferHandle<T>,
        data: &[T],
    ) {
        let len = data.len().min(buf.len() as usize);
        for frame in 0..PRE_WAIT_RING_LEN {
            let mapped = self
                .storage_buffers
                .get_mapped_mem_for_frame_immutable(buf, frame);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped, len);
            }
        }
    }

    /// setup-time initialization for a GPU-only buffer; the only CPU write
    /// path it has, safe because nothing is in flight yet
    pub fn write_gpu_only_all_frames<T>(&mut self, buf: &mut GpuOnlyBufferHandle<T>, data: &[T]) {
        let len = data.len().min(buf.len() as usize);
        for frame in 0..PRE_WAIT_RING_LEN {
            let mapped = self
                .storage_buffers
                .get_mapped_mem_for_frame_gpu_only(buf, frame);
            unsafe {
                std::ptr::copy_nonoverlapping(data.as_ptr(), mapped, len);
            }
        }
    }

    pub fn drop_storage_buffer<T>(&mut self, storage_buffer: StorageBufferHandle<T>) {
        let buffers_per_frame = self.storage_buffers.take(storage_buffer);
        for raw_storage_buffer in buffers_per_frame {
            self.destroy_storage_buffer(raw_storage_buffer);
        }
    }

    pub fn drop_immutable_buffer<T>(&mut self, immutable_buffer: ImmutableBufferHandle<T>) {
        let buffers_per_frame = self.storage_buffers.take_immutable(immutable_buffer);
        for raw_storage_buffer in buffers_per_frame {
            self.destroy_storage_buffer(raw_storage_buffer);
        }
    }

    pub fn drop_gpu_only_buffer<T>(&mut self, gpu_only_buffer: GpuOnlyBufferHandle<T>) {
        let buffers_per_frame = self.storage_buffers.take_gpu_only(gpu_only_buffer);
        for raw_storage_buffer in buffers_per_frame {
            self.destroy_storage_buffer(raw_storage_buffer);
        }
    }

    fn destroy_storage_buffer(&mut self, mut storage_buffer: RawStorageBuffer) {
        unsafe {
            self.allocator
                .destroy_buffer(storage_buffer.buffer, &mut storage_buffer.allocation);
        }
    }

    pub fn create_pipeline<V: VertexDescription, D: DrawCall>(
        &mut self,
        config: PipelineConfig<V, D>,
    ) -> anyhow::Result<PipelineHandle<D>> {
        let pipeline = self.init_pipeline(config)?;
        let handle = self.pipelines.add(pipeline);

        Ok(handle)
    }

    pub fn create_picking_pipeline<V: VertexDescription>(
        &mut self,
        picking_config: PipelineConfig<V, DrawVertexCount>,
    ) -> anyhow::Result<PickingPipelineHandle> {
        // Lazily initialize picking resources on first use
        if self.picking.is_none() {
            self.picking = Some(PickingResources::init(
                &self.allocator,
                &self.device,
                self.render_extent,
            )?);
        }

        let picking_pipeline_layout =
            ShaderPipelineLayout::create_from_atlas(&self.device, &*picking_config.shader)?;
        let picking_pipeline = create_graphics_pipeline(
            &self.device,
            picking::PICKING_FORMAT,
            None, // no depth attachment for picking
            vk::SampleCountFlags::TYPE_1,
            &picking_pipeline_layout,
            &picking_config.shader.vertex_binding_descriptions(),
            &picking_config.shader.vertex_attribute_descriptions(),
            false, // no depth test for picking
            false, // no blending for uint render target
        )?;

        let layout_bindings = picking_config.shader.layout_bindings();
        let descriptor_pool = create_descriptor_pool(&self.device, &picking_pipeline_layout)?;

        let uniform_buffers_in_layout_frame_order: Vec<&[RawUniformBuffer; PRE_WAIT_RING_LEN]> =
            picking_config
                .uniform_buffer_handles
                .iter()
                .map(|raw_handle| self.uniform_buffers.get_raw(raw_handle))
                .collect();

        let set_layouts: Vec<_> = picking_pipeline_layout
            .descriptor_set_layouts
            .iter()
            .map(|t| t.0)
            .collect();
        let descriptor_sets = create_descriptor_sets(
            &self.device,
            descriptor_pool,
            &set_layouts,
            &uniform_buffers_in_layout_frame_order,
            &[],
            &[],
            layout_bindings,
        )?;

        let renderer_pipeline = RendererPipeline {
            layout: picking_pipeline_layout,
            pipeline: picking_pipeline,
            vertex_pipeline_config: VertexPipelineConfig::VertexCount,
            descriptor_pool,
            descriptor_sets,
            shader: picking_config.shader,
            disable_depth_test: true,
        };

        let handle = self.pipelines.add_picking(renderer_pipeline);
        Ok(handle)
    }

    fn destroy_pipeline(&mut self, pipeline: RendererPipeline) {
        unsafe {
            // this also destroys the sets from the pool
            self.device
                .destroy_descriptor_pool(pipeline.descriptor_pool, None);

            for &(desc_set_layout, _) in &pipeline.layout.descriptor_set_layouts {
                self.device
                    .destroy_descriptor_set_layout(desc_set_layout, None);
            }

            match pipeline.vertex_pipeline_config {
                VertexPipelineConfig::VertexAndIndexBuffers(mut vi_bufs) => {
                    self.allocator
                        .destroy_buffer(vi_bufs.index_buffer, &mut vi_bufs.index_buffer_memory);
                    self.allocator
                        .destroy_buffer(vi_bufs.vertex_buffer, &mut vi_bufs.vertex_buffer_memory);
                }

                VertexPipelineConfig::VertexCount => {}
            }

            self.device.destroy_pipeline(pipeline.pipeline, None);
            self.device
                .destroy_pipeline_layout(pipeline.layout.pipeline_layout, None);
        }
    }

    pub fn create_compute_pipeline(
        &mut self,
        config: ComputePipelineConfig,
    ) -> anyhow::Result<PipelineHandle<Compute>> {
        let pipeline_layout =
            ComputeShaderPipelineLayout::create_from_atlas(&self.device, &*config.shader)?;

        let shader_module = {
            let shader_module_create_info = vk::ShaderModuleCreateInfo::default()
                .code(&pipeline_layout.compute_shader.spv_bytes);
            unsafe {
                self.device
                    .create_shader_module(&shader_module_create_info, None)?
            }
        };

        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&pipeline_layout.compute_shader.entry_point_name);

        let compute_pipeline_create_info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(pipeline_layout.pipeline_layout);

        let pipeline = unsafe {
            self.device.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[compute_pipeline_create_info],
                None,
            )
        }
        .map_err(|(_pipelines, err)| err)?[0];

        self.set_debug_name(
            pipeline,
            debug::clean_shader_name(config.shader.source_file_name()),
        );

        unsafe {
            self.device.destroy_shader_module(shader_module, None);
        }

        let layout_bindings = config.shader.layout_bindings();
        let descriptor_pool = create_descriptor_pool_from_layouts(
            &self.device,
            &pipeline_layout.descriptor_set_layouts,
        )?;

        let textures = {
            let mut textures = vec![];
            for texture_handle in config.texture_handles {
                let texture = self.textures.get(texture_handle);
                textures.push(texture);
            }
            textures
        };

        let uniform_buffers_in_layout_frame_order: Vec<&[RawUniformBuffer; PRE_WAIT_RING_LEN]> =
            config
                .uniform_buffer_handles
                .iter()
                .map(|raw_handle| self.uniform_buffers.get_raw(raw_handle))
                .collect();

        let storage_images: Vec<&storage_texture::StorageTexture> = config
            .storage_texture_handles
            .iter()
            .map(|handle| self.storage_textures.get(handle))
            .collect();

        let set_layouts: Vec<_> = pipeline_layout
            .descriptor_set_layouts
            .iter()
            .map(|t| t.0)
            .collect();
        let descriptor_sets = create_descriptor_sets(
            &self.device,
            descriptor_pool,
            &set_layouts,
            &uniform_buffers_in_layout_frame_order,
            &textures,
            &storage_images,
            layout_bindings,
        )?;

        let compute_renderer_pipeline = ComputeRendererPipeline {
            layout: pipeline_layout,
            pipeline,
            descriptor_pool,
            descriptor_sets,
            shader: config.shader,
        };

        let handle = self.compute_pipelines.add(compute_renderer_pipeline);
        self.has_compute_pipelines = true;

        Ok(handle)
    }

    fn destroy_compute_pipeline(&mut self, pipeline: ComputeRendererPipeline) {
        unsafe {
            self.device
                .destroy_descriptor_pool(pipeline.descriptor_pool, None);

            for &(desc_set_layout, _) in &pipeline.layout.descriptor_set_layouts {
                self.device
                    .destroy_descriptor_set_layout(desc_set_layout, None);
            }

            self.device.destroy_pipeline(pipeline.pipeline, None);
            self.device
                .destroy_pipeline_layout(pipeline.layout.pipeline_layout, None);
        }
    }

    fn init_pipeline<V: VertexDescription, D: DrawCall>(
        &mut self,
        config: PipelineConfig<V, D>,
    ) -> anyhow::Result<RendererPipeline> {
        let pipeline_layout =
            ShaderPipelineLayout::create_from_atlas(&self.device, &*config.shader)?;
        let pipeline = create_graphics_pipeline(
            &self.device,
            self.image_format,
            Some(self.depth_format),
            self.msaa_samples,
            &pipeline_layout,
            &config.shader.vertex_binding_descriptions(),
            &config.shader.vertex_attribute_descriptions(),
            !config.disable_depth_test,
            true,
        )?;

        self.set_debug_name(
            pipeline,
            debug::clean_shader_name(config.shader.source_file_name()),
        );

        let vertex_pipeline_config = match &config.vertex_config {
            VertexConfig::VertexAndIndexBuffers(vertices, indices) => {
                let (vertex_buffer, vertex_buffer_memory) = create_vertex_buffer(
                    &self.allocator,
                    &self.device,
                    self.command_pool,
                    self.graphics_queue,
                    vertices,
                )?;

                let (index_buffer, index_buffer_memory) = create_index_buffer(
                    &self.allocator,
                    &self.device,
                    self.command_pool,
                    self.graphics_queue,
                    indices,
                )?;

                let index_count = indices.len() as u32;

                let vi_bufs = VertexAndIndexBuffers {
                    vertex_buffer,
                    vertex_buffer_memory,
                    index_buffer,
                    index_buffer_memory,
                    index_count,
                };

                VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs)
            }

            VertexConfig::VertexCount => VertexPipelineConfig::VertexCount,
        };

        let layout_bindings = config.shader.layout_bindings();

        let descriptor_pool = create_descriptor_pool(&self.device, &pipeline_layout)?;

        let textures = {
            let mut textures = vec![];
            for texture_handle in config.texture_handles {
                let texture = self.textures.get(texture_handle);
                textures.push(texture);
            }
            textures
        };

        let uniform_buffers_in_layout_frame_order: Vec<&[RawUniformBuffer; PRE_WAIT_RING_LEN]> =
            config
                .uniform_buffer_handles
                .iter()
                .map(|raw_handle| self.uniform_buffers.get_raw(raw_handle))
                .collect();

        let storage_images: Vec<&storage_texture::StorageTexture> = config
            .storage_texture_handles
            .iter()
            .map(|handle| self.storage_textures.get(handle))
            .collect();

        let set_layouts: Vec<_> = pipeline_layout
            .descriptor_set_layouts
            .iter()
            .map(|t| t.0)
            .collect();
        let descriptor_sets = create_descriptor_sets(
            &self.device,
            descriptor_pool,
            &set_layouts,
            &uniform_buffers_in_layout_frame_order,
            &textures,
            &storage_images,
            layout_bindings,
        )?;

        Ok(RendererPipeline {
            layout: pipeline_layout,
            pipeline,
            vertex_pipeline_config,
            descriptor_pool,
            descriptor_sets,
            shader: config.shader,
            disable_depth_test: config.disable_depth_test,
        })
    }

    fn record_compute_commands(
        &self,
        command_buffer: vk::CommandBuffer,
        pending_compute: &[PendingComputeCommand],
    ) {
        for cmd in pending_compute {
            match cmd {
                PendingComputeCommand::Dispatch {
                    pipeline_index,
                    group_count,
                } => {
                    let compute_pipeline = self.compute_pipelines.get_by_index(*pipeline_index);

                    let label_name = CString::new(debug::clean_shader_name(
                        compute_pipeline.shader.source_file_name(),
                    ))
                    .unwrap();
                    let label = vk::DebugUtilsLabelEXT::default()
                        .label_name(&label_name)
                        .color([0.4, 0.8, 0.4, 1.0]);
                    unsafe {
                        self.debug_utils_device
                            .cmd_begin_debug_utils_label(command_buffer, &label);
                    }

                    let descriptor_sets_per_frame =
                        compute_pipeline.layout.descriptor_set_layouts.len();
                    let compute_descriptor_sets = compute_pipeline
                        .descriptor_sets
                        .chunks(descriptor_sets_per_frame)
                        .nth(self.ring_slot)
                        .unwrap();

                    unsafe {
                        self.device.cmd_bind_pipeline(
                            command_buffer,
                            vk::PipelineBindPoint::COMPUTE,
                            compute_pipeline.pipeline,
                        );

                        self.device.cmd_bind_descriptor_sets(
                            command_buffer,
                            vk::PipelineBindPoint::COMPUTE,
                            compute_pipeline.layout.pipeline_layout,
                            0,
                            compute_descriptor_sets,
                            &[],
                        );

                        self.device.cmd_dispatch(
                            command_buffer,
                            group_count[0],
                            group_count[1],
                            group_count[2],
                        );
                    }

                    unsafe {
                        self.debug_utils_device
                            .cmd_end_debug_utils_label(command_buffer);
                    }
                }

                PendingComputeCommand::Barrier {
                    src_stage,
                    dst_stage,
                    src_access,
                    dst_access,
                } => {
                    let memory_barrier = vk::MemoryBarrier2::default()
                        .src_stage_mask(*src_stage)
                        .src_access_mask(*src_access)
                        .dst_stage_mask(*dst_stage)
                        .dst_access_mask(*dst_access);
                    let memory_barriers = [memory_barrier];
                    let dependency_info =
                        vk::DependencyInfo::default().memory_barriers(&memory_barriers);

                    unsafe {
                        self.device
                            .cmd_pipeline_barrier2(command_buffer, &dependency_info);
                    }
                }
            }
        }
    }

    /// Record a standalone compute command buffer for pipelined async compute submission
    fn record_compute_command_buffer(
        &self,
        pending_compute: &[PendingComputeCommand],
    ) -> Result<(), anyhow::Error> {
        let command_buffer = self.compute_command_buffers[self.flight_slot];

        let begin_info = vk::CommandBufferBeginInfo::default();
        unsafe {
            self.device
                .begin_command_buffer(command_buffer, &begin_info)?;
        }

        self.record_compute_commands(command_buffer, pending_compute);

        unsafe { self.device.end_command_buffer(command_buffer)? };
        Ok(())
    }

    fn record_command_buffer<D>(
        &mut self,
        pipeline_handle: &PipelineHandle<D>,
        image_index: u32,
        draw_call: DrawCallConfig,
        picking_config: Option<&PickingDrawConfig>,
        pending_compute: &[PendingComputeCommand],
        compute_placement: ComputePlacement,
    ) -> Result<(), anyhow::Error> {
        let command_buffer = self.command_buffers[self.flight_slot];

        let begin_info = vk::CommandBufferBeginInfo::default();
        unsafe {
            self.device
                .begin_command_buffer(command_buffer, &begin_info)?;
        }

        if compute_placement == ComputePlacement::BeforeGraphics {
            self.record_compute_commands(command_buffer, pending_compute);
        }

        // PICKING RENDER PASS (before main pass)
        if let (Some(picking_config), Some(picking)) = (picking_config, self.picking.as_ref()) {
            let label_name = c"Picking";
            let label = vk::DebugUtilsLabelEXT::default()
                .label_name(label_name)
                .color([0.8, 0.4, 0.4, 1.0]);
            unsafe {
                self.debug_utils_device
                    .cmd_begin_debug_utils_label(command_buffer, &label);
            }
            let picking_pipeline = self.pipelines.get_picking(&picking_config.picking_handle);
            let picking_image = picking.images[self.flight_slot];
            let picking_render_area = vk::Rect2D::default()
                .offset(vk::Offset2D::default())
                .extent(self.render_extent);

            // transition the picking image for rendering;
            // the previous readback copy from this image was 2 frames ago
            let barrier_to_attachment = vk::ImageMemoryBarrier2::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(picking_image)
                .subresource_range(COLOR_SUBRESOURCE_RANGE)
                .src_stage_mask(vk::PipelineStageFlags2::COPY)
                .src_access_mask(vk::AccessFlags2::NONE)
                .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE);
            cmd_barrier2(&self.device, command_buffer, &[barrier_to_attachment]);

            let picking_clear = vk::ClearValue {
                color: vk::ClearColorValue {
                    uint32: [0, 0, 0, 0],
                },
            };
            let picking_color_attachment = vk::RenderingAttachmentInfo::default()
                .image_view(picking.image_views[self.flight_slot])
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .clear_value(picking_clear);
            let picking_color_attachments = [picking_color_attachment];
            let picking_rendering_info = vk::RenderingInfo::default()
                .render_area(picking_render_area)
                .layer_count(1)
                .color_attachments(&picking_color_attachments);

            unsafe {
                self.device
                    .cmd_begin_rendering(command_buffer, &picking_rendering_info);

                self.device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    picking_pipeline.pipeline,
                );
            }

            let viewport = vk::Viewport::default()
                .x(0.0)
                .y(0.0)
                .width(self.render_extent.width as f32)
                .height(self.render_extent.height as f32)
                .min_depth(0.0)
                .max_depth(1.0);
            let viewports = [viewport];
            unsafe { self.device.cmd_set_viewport(command_buffer, 0, &viewports) };

            let scissor = vk::Rect2D::default()
                .offset(vk::Offset2D::default())
                .extent(self.render_extent);
            let scissors = [scissor];
            unsafe { self.device.cmd_set_scissor(command_buffer, 0, &scissors) };

            let picking_descriptor_sets =
                self.picking_descriptor_sets_for_frame(&picking_config.picking_handle);
            unsafe {
                self.device.cmd_bind_descriptor_sets(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    picking_pipeline.layout.pipeline_layout,
                    0,
                    picking_descriptor_sets,
                    &[],
                );

                self.device.cmd_draw(command_buffer, 3, 1, 0, 0);
                self.device.cmd_end_rendering(command_buffer);
            }

            // transition the picking image for the readback copy
            let barrier_to_copy = vk::ImageMemoryBarrier2::default()
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(picking_image)
                .subresource_range(COLOR_SUBRESOURCE_RANGE)
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::COPY)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_READ);
            cmd_barrier2(&self.device, command_buffer, &[barrier_to_copy]);

            // Copy 1 pixel from picking image to readback buffer
            let mouse_x =
                picking_config.mouse_pixel[0].min(self.render_extent.width.saturating_sub(1));
            let mouse_y =
                picking_config.mouse_pixel[1].min(self.render_extent.height.saturating_sub(1));

            let region = vk::BufferImageCopy::default()
                .buffer_offset(0)
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .mip_level(0)
                        .base_array_layer(0)
                        .layer_count(1),
                )
                .image_offset(vk::Offset3D {
                    x: mouse_x as i32,
                    y: mouse_y as i32,
                    z: 0,
                })
                .image_extent(vk::Extent3D {
                    width: 1,
                    height: 1,
                    depth: 1,
                });

            unsafe {
                self.device.cmd_copy_image_to_buffer(
                    command_buffer,
                    picking.images[self.flight_slot],
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    picking.readback_buffers[self.flight_slot],
                    &[region],
                );
            }

            unsafe {
                self.debug_utils_device
                    .cmd_end_debug_utils_label(command_buffer);
            }
        }

        // MAIN RENDER PASS
        {
            let shader_name = debug::clean_shader_name(
                self.renderer_pipeline(pipeline_handle)
                    .shader
                    .source_file_name(),
            );
            let label_name = CString::new(shader_name).unwrap();
            let label = vk::DebugUtilsLabelEXT::default()
                .label_name(&label_name)
                .color([1.0, 1.0, 1.0, 1.0]);
            unsafe {
                self.debug_utils_device
                    .cmd_begin_debug_utils_label(command_buffer, &label);
            }
        }

        // Main rendering uses render_extent (scaled resolution)
        let render_area = vk::Rect2D::default()
            .offset(vk::Offset2D::default())
            .extent(self.render_extent);

        // transition attachments for rendering
        // (replaces the implicit transitions and entry dependency of the old render pass;
        // the MSAA color and depth images are shared by all frames in flight)
        let color_barrier = vk::ImageMemoryBarrier2::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.color_image)
            .subresource_range(COLOR_SUBRESOURCE_RANGE)
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE);

        let mut depth_aspect = vk::ImageAspectFlags::DEPTH;
        if has_stencil_component(self.depth_format) {
            depth_aspect |= vk::ImageAspectFlags::STENCIL;
        }
        let depth_barrier = vk::ImageMemoryBarrier2::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.depth_image)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: depth_aspect,
                ..COLOR_SUBRESOURCE_RANGE
            })
            .src_stage_mask(vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS)
            .src_access_mask(vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE)
            .dst_stage_mask(
                vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                    | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
            )
            .dst_access_mask(
                vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                    | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
            );

        // the previous use of this frame's resolve image was the upscale blit read
        let resolve_barrier = vk::ImageMemoryBarrier2::default()
            .old_layout(vk::ImageLayout::UNDEFINED)
            .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.resolve_images[self.flight_slot])
            .subresource_range(COLOR_SUBRESOURCE_RANGE)
            .src_stage_mask(vk::PipelineStageFlags2::BLIT)
            .src_access_mask(vk::AccessFlags2::NONE)
            .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE);

        cmd_barrier2(
            &self.device,
            command_buffer,
            &[color_barrier, depth_barrier, resolve_barrier],
        );

        let clear_color = vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.0, 0.0, 0.0, 1.0],
            },
        };
        let clear_depth_stencil = vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        };

        // MSAA color renders at msaa_samples and resolves into this frame's resolve
        // image; only the resolved output is consumed (by the upscale blit)
        let color_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(self.color_image_view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .resolve_mode(vk::ResolveModeFlags::AVERAGE)
            .resolve_image_view(self.resolve_image_views[self.flight_slot])
            .resolve_image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(clear_color);
        let color_attachments = [color_attachment];
        let depth_attachment = vk::RenderingAttachmentInfo::default()
            .image_view(self.depth_image_view)
            .image_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            .clear_value(clear_depth_stencil);
        let rendering_info = vk::RenderingInfo::default()
            .render_area(render_area)
            .layer_count(1)
            .color_attachments(&color_attachments)
            .depth_attachment(&depth_attachment);

        // BEGIN RENDERING
        unsafe {
            self.device
                .cmd_begin_rendering(command_buffer, &rendering_info);
        }

        unsafe {
            self.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.renderer_pipeline(pipeline_handle).pipeline,
            );
        }

        // Use render_extent for game rendering (scaled resolution)
        let viewport = vk::Viewport::default()
            .x(0.0)
            .y(0.0)
            .width(self.render_extent.width as f32)
            .height(self.render_extent.height as f32)
            .min_depth(0.0)
            .max_depth(1.0);
        let viewports = [viewport];
        unsafe { self.device.cmd_set_viewport(command_buffer, 0, &viewports) };

        let scissor = vk::Rect2D::default()
            .offset(vk::Offset2D::default())
            .extent(self.render_extent);
        let scissors = [scissor];
        unsafe { self.device.cmd_set_scissor(command_buffer, 0, &scissors) };

        match &self
            .renderer_pipeline(pipeline_handle)
            .vertex_pipeline_config
        {
            VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs) => {
                let buffers = [vi_bufs.vertex_buffer];
                let offsets = [0];
                unsafe {
                    self.device
                        .cmd_bind_vertex_buffers(command_buffer, 0, &buffers, &offsets);

                    self.device.cmd_bind_index_buffer(
                        command_buffer,
                        vi_bufs.index_buffer,
                        0,
                        vk::IndexType::UINT32,
                    );
                }
            }

            VertexPipelineConfig::VertexCount => {}
        }

        let descriptor_sets = self.descriptor_sets_for_frame(pipeline_handle);
        unsafe {
            self.device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.renderer_pipeline(pipeline_handle)
                    .layout
                    .pipeline_layout,
                0,
                descriptor_sets,
                &[],
            );
        }

        match draw_call {
            DrawCallConfig::VertexCount(vertex_count) => unsafe {
                self.device.cmd_draw(command_buffer, vertex_count, 1, 0, 0);
            },

            DrawCallConfig::IndexCount(index_count) => unsafe {
                self.device
                    .cmd_draw_indexed(command_buffer, index_count, 1, 0, 0, 0);
            },
        }

        // END MAIN RENDERING
        unsafe { self.device.cmd_end_rendering(command_buffer) };

        // transition this frame's resolve image for the upscale blit read
        // (replaces the old render pass's TRANSFER_SRC final layout and exit dependency)
        let resolve_to_blit_src = vk::ImageMemoryBarrier2::default()
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(self.resolve_images[self.flight_slot])
            .subresource_range(COLOR_SUBRESOURCE_RANGE)
            .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(vk::PipelineStageFlags2::BLIT)
            .dst_access_mask(vk::AccessFlags2::TRANSFER_READ);
        cmd_barrier2(&self.device, command_buffer, &[resolve_to_blit_src]);

        unsafe {
            self.debug_utils_device
                .cmd_end_debug_utils_label(command_buffer);
        }

        // BLIT FROM RESOLVE IMAGE TO SWAPCHAIN (upscale step)
        {
            let label = vk::DebugUtilsLabelEXT::default()
                .label_name(c"Blit")
                .color([0.4, 0.4, 0.8, 1.0]);
            unsafe {
                self.debug_utils_device
                    .cmd_begin_debug_utils_label(command_buffer, &label);
            }
        }
        let swapchain_image = self.swapchain_images[image_index as usize];
        {
            // Transition swapchain image from UNDEFINED to TRANSFER_DST
            let barrier_to_transfer = vk::ImageMemoryBarrier2::default()
                .old_layout(vk::ImageLayout::UNDEFINED)
                .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(swapchain_image)
                .subresource_range(vk::ImageSubresourceRange {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    base_mip_level: 0,
                    level_count: 1,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::NONE)
                .dst_stage_mask(vk::PipelineStageFlags2::BLIT)
                .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE);

            cmd_barrier2(&self.device, command_buffer, &[barrier_to_transfer]);

            // Blit from resolve_image to swapchain_image
            let src_subresource = vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(0)
                .base_array_layer(0)
                .layer_count(1);

            let blit = vk::ImageBlit::default()
                .src_offsets([
                    vk::Offset3D::default(),
                    vk::Offset3D {
                        x: self.render_extent.width as i32,
                        y: self.render_extent.height as i32,
                        z: 1,
                    },
                ])
                .src_subresource(src_subresource)
                .dst_offsets([
                    vk::Offset3D::default(),
                    vk::Offset3D {
                        x: self.image_extent.width as i32,
                        y: self.image_extent.height as i32,
                        z: 1,
                    },
                ])
                .dst_subresource(src_subresource);

            unsafe {
                self.device.cmd_blit_image(
                    command_buffer,
                    self.resolve_images[self.flight_slot],
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    swapchain_image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[blit],
                    vk::Filter::LINEAR,
                );
            }

            // With egui active the swapchain image is rendered to once more (the egui
            // overlay loads and draws on top); otherwise it goes straight to present.
            let barrier_to_next = if self.egui.is_some() {
                vk::ImageMemoryBarrier2::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .src_stage_mask(vk::PipelineStageFlags2::BLIT)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                    .dst_access_mask(
                        vk::AccessFlags2::COLOR_ATTACHMENT_READ
                            | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                    )
            } else {
                vk::ImageMemoryBarrier2::default()
                    .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                    .src_stage_mask(vk::PipelineStageFlags2::BLIT)
                    .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
                    // presentation waits on the render_finished semaphore, not this barrier
                    .dst_stage_mask(vk::PipelineStageFlags2::NONE)
                    .dst_access_mask(vk::AccessFlags2::NONE)
            }
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(swapchain_image)
            .subresource_range(COLOR_SUBRESOURCE_RANGE);

            cmd_barrier2(&self.device, command_buffer, &[barrier_to_next]);
        }

        unsafe {
            self.debug_utils_device
                .cmd_end_debug_utils_label(command_buffer);
        }

        // EGUI RENDERING (separate 1-sample rendering for egui overlay)
        if let Some(egui) = &mut self.egui {
            let label = vk::DebugUtilsLabelEXT::default()
                .label_name(c"Egui")
                .color([0.8, 0.8, 0.4, 1.0]);
            unsafe {
                self.debug_utils_device
                    .cmd_begin_debug_utils_label(command_buffer, &label);
            }

            let render_area = vk::Rect2D::default()
                .offset(vk::Offset2D::default())
                .extent(self.image_extent);
            // draws over the blitted frame on the swapchain image
            let egui_color_attachment = vk::RenderingAttachmentInfo::default()
                .image_view(self.swapchain_image_views[image_index as usize])
                .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .load_op(vk::AttachmentLoadOp::LOAD)
                .store_op(vk::AttachmentStoreOp::STORE);
            let egui_color_attachments = [egui_color_attachment];
            let egui_rendering_info = vk::RenderingInfo::default()
                .render_area(render_area)
                .layer_count(1)
                .color_attachments(&egui_color_attachments);

            unsafe {
                self.device
                    .cmd_begin_rendering(command_buffer, &egui_rendering_info);
            }

            // Draw egui overlay (begin_frame is idempotent, safe to call if already begun)
            let screen_size = [self.width, self.height];
            egui.begin_frame(screen_size);
            let wants_keyboard_input = egui.end_frame_and_draw(
                self.graphics_queue,
                self.command_pool,
                command_buffer,
                self.image_extent,
                self.flight_slot,
            );

            // Toggle SDL text input based on whether egui has a text field focused
            if wants_keyboard_input && !self.text_input_active {
                unsafe { sdl3::sys::keyboard::SDL_StartTextInput(self.window.raw()) };
                self.text_input_active = true;
            } else if !wants_keyboard_input && self.text_input_active {
                unsafe { sdl3::sys::keyboard::SDL_StopTextInput(self.window.raw()) };
                self.text_input_active = false;
            }

            unsafe { self.device.cmd_end_rendering(command_buffer) };

            // transition the swapchain image for presentation
            let barrier_to_present = vk::ImageMemoryBarrier2::default()
                .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .image(swapchain_image)
                .subresource_range(COLOR_SUBRESOURCE_RANGE)
                .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
                .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
                // presentation waits on the render_finished semaphore, not this barrier
                .dst_stage_mask(vk::PipelineStageFlags2::NONE)
                .dst_access_mask(vk::AccessFlags2::NONE);
            cmd_barrier2(&self.device, command_buffer, &[barrier_to_present]);

            unsafe {
                self.debug_utils_device
                    .cmd_end_debug_utils_label(command_buffer);
            }
        }

        unsafe { self.device.end_command_buffer(command_buffer)? };

        Ok(())
    }

    fn descriptor_sets_for_frame<D>(
        &self,
        pipeline_handle: &PipelineHandle<D>,
    ) -> &[vk::DescriptorSet] {
        // see create_descriptor_sets
        let descriptor_sets_per_frame = self
            .renderer_pipeline(pipeline_handle)
            .layout
            .descriptor_set_layouts
            .len();
        self.renderer_pipeline(pipeline_handle)
            .descriptor_sets
            .chunks(descriptor_sets_per_frame)
            .nth(self.ring_slot)
            .unwrap()
    }

    #[expect(unused)]
    fn descriptor_sets_for_compute_frame(
        &self,
        pipeline_handle: &PipelineHandle<Compute>,
    ) -> &[vk::DescriptorSet] {
        let compute_pipeline = self.compute_pipelines.get(pipeline_handle);
        let descriptor_sets_per_frame = compute_pipeline.layout.descriptor_set_layouts.len();
        compute_pipeline
            .descriptor_sets
            .chunks(descriptor_sets_per_frame)
            .nth(self.ring_slot)
            .unwrap()
    }

    fn picking_descriptor_sets_for_frame(
        &self,
        handle: &PickingPipelineHandle,
    ) -> &[vk::DescriptorSet] {
        let pipeline = self.pipelines.get_picking(handle);
        let descriptor_sets_per_frame = pipeline.layout.descriptor_set_layouts.len();
        pipeline
            .descriptor_sets
            .chunks(descriptor_sets_per_frame)
            .nth(self.ring_slot)
            .unwrap()
    }

    fn draw_frame<D>(
        &mut self,
        pipeline_handle: &PipelineHandle<D>,
        draw_call: DrawCallConfig,
        picking_config: Option<PickingDrawConfig>,
        pending_compute: Vec<PendingComputeCommand>,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), anyhow::Error> {
        #[cfg(debug_assertions)]
        {
            let mut compute_indices: Vec<usize> = pending_compute
                .iter()
                .filter_map(|cmd| match cmd {
                    PendingComputeCommand::Dispatch { pipeline_index, .. } => Some(*pipeline_index),
                    _ => None,
                })
                .collect();
            compute_indices.sort_unstable();
            compute_indices.dedup();
            self.check_for_shader_recompile(pipeline_handle, &compute_indices)?;
        }

        let command_buffer = self.command_buffers[self.flight_slot];

        // 1. Acquire swapchain image (can block on vsync)
        let (image_index, swapchain_was_suboptimal_on_image_acquire) = unsafe {
            match self.swapchain_device_ext.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available[self.ring_slot],
                vk::Fence::null(),
            ) {
                Ok(tup) => tup,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    return self.recreate_swapchain();
                }
                Err(other_error) => {
                    return Err(other_error.into());
                }
            }
        };

        self.total_frames += 1;
        let frame_value = self.total_frames as u64;

        // 2. CPU buffer writes BEFORE the timeline wait
        //    Safe because buffer[ring_slot] was last used by frame (total - PRE_WAIT_RING_LEN)
        //    and that frame's timeline value was waited for during frame (total - 1)
        let mut gpu = Gpu {
            ring_slot: self.ring_slot,
            uniform_buffers: &mut self.uniform_buffers,
            storage_buffers: &mut self.storage_buffers,
        };
        gpu_update(&mut gpu);

        // 3. Wait until frame (N - MAX_FRAMES_IN_FLIGHT)'s graphics submit retires
        //    (command buffer reuse). Frames 1 and 2 wait on value 0, trivially satisfied.
        let semaphores = [self.frame_timeline];
        let values = [frame_value.saturating_sub(MAX_FRAMES_IN_FLIGHT as u64)];
        let wait_info = vk::SemaphoreWaitInfo::default()
            .semaphores(&semaphores)
            .values(&values);
        unsafe { self.device.wait_semaphores(&wait_info, u64::MAX)? };

        // 3a. Read picking result from staging buffer (written 2 frames ago, now safe to read)
        if let Some(picking) = &self.picking {
            let id = unsafe { *picking.readback_mapped[self.flight_slot] };
            self.last_picked_object_id = id;
        }

        // 4. Free egui textures (must be after the timeline wait)
        if let Some(egui) = &mut self.egui {
            egui.free_pending_textures(self.flight_slot);
        }

        // Determine if we should use pipelined async compute this frame.
        // The first compute frame always goes through the combined path below,
        // so graphics sees that frame's compute output.
        let use_pipelined =
            self.pipelined_compute && self.has_compute_pipelines && self.compute_frames > 0;
        // This frame's compute_timeline value, if compute is submitted
        let compute_value = self.compute_frames + 1;

        if use_pipelined {
            // --- PIPELINED: separate compute and graphics submissions ---
            // Uses dedicated compute queue when available, otherwise same graphics queue.
            // Two separate vkQueueSubmit calls give the driver freedom to overlap execution.
            let compute_queue = self.compute_queue.unwrap_or(self.graphics_queue);
            let compute_cb = self.compute_command_buffers[self.flight_slot];

            // Wait until this slot's previous compute submit retires (compute CB reuse).
            // compute_frames advances every frame once compute is active, so the slot's
            // last user signaled compute_value - MAX_FRAMES_IN_FLIGHT.
            let semaphores = [self.compute_timeline];
            let values = [compute_value.saturating_sub(MAX_FRAMES_IN_FLIGHT as u64)];
            let wait_info = vk::SemaphoreWaitInfo::default()
                .semaphores(&semaphores)
                .values(&values);
            unsafe { self.device.wait_semaphores(&wait_info, u64::MAX)? };

            // Record compute command buffer
            unsafe {
                self.device
                    .reset_command_buffer(compute_cb, Default::default())?;
            }
            self.record_compute_command_buffer(&pending_compute)?;

            // Record graphics command buffer (skip compute section)
            unsafe {
                self.device
                    .reset_command_buffer(command_buffer, Default::default())?;
            }
            self.record_command_buffer(
                pipeline_handle,
                image_index,
                draw_call,
                picking_config.as_ref(),
                &pending_compute,
                ComputePlacement::SeparateCommandBuffer,
            )?;

            // Submit compute: wait on the previous frame's compute, signal this frame's value
            let compute_waits = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.compute_timeline)
                .value(compute_value - 1)
                .stage_mask(vk::PipelineStageFlags2::COMPUTE_SHADER)];
            let compute_signals = [vk::SemaphoreSubmitInfo::default()
                .semaphore(self.compute_timeline)
                .value(compute_value)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS)];
            let compute_cbs = [vk::CommandBufferSubmitInfo::default().command_buffer(compute_cb)];
            let compute_submit = vk::SubmitInfo2::default()
                .wait_semaphore_infos(&compute_waits)
                .command_buffer_infos(&compute_cbs)
                .signal_semaphore_infos(&compute_signals);
            unsafe {
                self.device
                    .queue_submit2(compute_queue, &[compute_submit], vk::Fence::null())?;
            }

            // Submit graphics: wait on image_available + previous frame's compute,
            // signal render_finished + this frame's timeline value
            let gfx_waits = [
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(self.image_available[self.ring_slot])
                    .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT),
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(self.compute_timeline)
                    .value(compute_value - 1)
                    // compute output may be read by the vertex stage (e.g. particle
                    // rendering), the fragment stage, or same-frame compute
                    .stage_mask(
                        vk::PipelineStageFlags2::VERTEX_SHADER
                            | vk::PipelineStageFlags2::FRAGMENT_SHADER
                            | vk::PipelineStageFlags2::COMPUTE_SHADER,
                    ),
            ];
            let gfx_signals = [
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(self.render_finished[image_index as usize])
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
                vk::SemaphoreSubmitInfo::default()
                    .semaphore(self.frame_timeline)
                    .value(frame_value)
                    .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
            ];
            let gfx_cbs = [vk::CommandBufferSubmitInfo::default().command_buffer(command_buffer)];
            let gfx_submit = vk::SubmitInfo2::default()
                .wait_semaphore_infos(&gfx_waits)
                .command_buffer_infos(&gfx_cbs)
                .signal_semaphore_infos(&gfx_signals);
            unsafe {
                self.device
                    .queue_submit2(self.graphics_queue, &[gfx_submit], vk::Fence::null())?;
            }

            self.compute_frames += 1;
        } else {
            // --- NON-PIPELINED: compute + graphics in one command buffer ---
            // Compute runs first with barriers, then graphics reads results in same frame.
            unsafe {
                self.device
                    .reset_command_buffer(command_buffer, Default::default())?;
            }
            self.record_command_buffer(
                pipeline_handle,
                image_index,
                draw_call,
                picking_config.as_ref(),
                &pending_compute,
                ComputePlacement::BeforeGraphics,
            )?;

            let submit_command_buffers =
                [vk::CommandBufferSubmitInfo::default().command_buffer(command_buffer)];

            let image_available_wait = vk::SemaphoreSubmitInfo::default()
                .semaphore(self.image_available[self.ring_slot])
                .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT);
            let render_finished_signal = vk::SemaphoreSubmitInfo::default()
                .semaphore(self.render_finished[image_index as usize])
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS);
            let frame_timeline_signal = vk::SemaphoreSubmitInfo::default()
                .semaphore(self.frame_timeline)
                .value(frame_value)
                .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS);

            // When compute pipelines exist, add cross-frame synchronization:
            // - Wait on the previous frame's compute_timeline value (so reads see prior writes)
            // - Signal this frame's value (for the next frame to wait on)
            let (wait_semaphores, signal_semaphores);
            if self.has_compute_pipelines {
                wait_semaphores = vec![
                    image_available_wait,
                    vk::SemaphoreSubmitInfo::default()
                        .semaphore(self.compute_timeline)
                        // 0 on the first compute frame, which is trivially satisfied
                        .value(compute_value - 1)
                        // compute output may be read by this frame's compute, or by the
                        // vertex or fragment stages (e.g. particle rendering)
                        .stage_mask(
                            vk::PipelineStageFlags2::VERTEX_SHADER
                                | vk::PipelineStageFlags2::FRAGMENT_SHADER
                                | vk::PipelineStageFlags2::COMPUTE_SHADER,
                        ),
                ];
                signal_semaphores = vec![
                    render_finished_signal,
                    frame_timeline_signal,
                    vk::SemaphoreSubmitInfo::default()
                        .semaphore(self.compute_timeline)
                        .value(compute_value)
                        .stage_mask(vk::PipelineStageFlags2::ALL_COMMANDS),
                ];
            } else {
                wait_semaphores = vec![image_available_wait];
                signal_semaphores = vec![render_finished_signal, frame_timeline_signal];
            }

            let submit_info = vk::SubmitInfo2::default()
                .wait_semaphore_infos(&wait_semaphores)
                .command_buffer_infos(&submit_command_buffers)
                .signal_semaphore_infos(&signal_semaphores);
            unsafe {
                self.device.queue_submit2(
                    self.graphics_queue,
                    &[submit_info],
                    vk::Fence::null(),
                )?;
            }

            if self.has_compute_pipelines {
                self.compute_frames += 1;
            }
        }

        // 6. Advance both frame counters BEFORE present
        //    This ensures that if present triggers swapchain recreation (early return),
        //    the next frame won't reuse the same ring slot whose semaphores
        //    are still signaled from this frame's submit.
        self.flight_slot = (self.flight_slot + 1) % MAX_FRAMES_IN_FLIGHT;
        self.ring_slot = (self.ring_slot + 1) % PRE_WAIT_RING_LEN;

        let swapchains = [self.swapchain];
        let image_indices = [image_index];
        let present_wait = [self.render_finished[image_index as usize]];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&present_wait)
            .swapchains(&swapchains)
            .image_indices(&image_indices);
        unsafe {
            match self
                .swapchain_device_ext
                .queue_present(self.presentation_queue, &present_info)
            {
                Ok(false) => {
                    // not suboptimal, aka fine, or optimal i guess
                }
                Ok(true) | Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                    // suboptimal (vk::Result::SUBOPTIMAL_KHR) or out of date
                    return self.recreate_swapchain();
                }
                Err(other_error) => {
                    return Err(other_error.into());
                }
            }
        }

        if swapchain_was_suboptimal_on_image_acquire {
            return self.recreate_swapchain();
        }

        Ok(())
    }

    pub fn drain_gpu(&mut self) -> Result<(), anyhow::Error> {
        unsafe { self.device.device_wait_idle()? };
        Ok(())
    }

    // to be called on window resize
    pub fn recreate_swapchain(&mut self) -> Result<(), anyhow::Error> {
        unsafe { self.device.device_wait_idle()? }

        // NOTE: the timeline semaphores are monotonic and must NOT be recreated here —
        // resetting their values to 0 would deadlock the next frame's waits.

        self.cleanup_swapchain();
        unsafe {
            self.device.destroy_image_view(self.depth_image_view, None);
            self.allocator
                .destroy_image(self.depth_image, &mut self.depth_image_memory);
        }
        unsafe {
            self.device.destroy_image_view(self.color_image_view, None);
            self.allocator
                .destroy_image(self.color_image, &mut self.color_image_memory);
        }
        unsafe {
            for i in 0..MAX_FRAMES_IN_FLIGHT {
                self.device
                    .destroy_image_view(self.resolve_image_views[i], None);
                self.allocator
                    .destroy_image(self.resolve_images[i], &mut self.resolve_image_memories[i]);
            }
        }

        let CreatedSwapchain {
            swapchain,
            image_format,
            image_extent,
        } = create_swapchain(
            &self.window,
            &self.swapchain_device_ext,
            &self.surface_ext,
            self.surface,
            self.physical_device,
            &self.queue_family_indices,
        )?;
        self.swapchain = swapchain;
        self.image_format = image_format;
        self.image_extent = image_extent;

        // Recalculate render extent
        self.render_extent = calculate_render_extent(image_extent, self.render_scale);

        self.swapchain_images =
            unsafe { self.swapchain_device_ext.get_swapchain_images(swapchain)? };

        self.swapchain_image_views =
            create_swapchain_image_views(&self.device, self.image_format, &self.swapchain_images)?;

        // Recreate resolve images at render_extent (one per frame-in-flight)
        let (resolve_images, resolve_image_memories, resolve_image_views) = create_resolve_images(
            &self.allocator,
            &self.device,
            self.render_extent,
            self.image_format,
        )?;
        self.resolve_images = resolve_images;
        self.resolve_image_memories = resolve_image_memories;
        self.resolve_image_views = resolve_image_views;

        // Depth and color at render_extent
        let (depth_image, depth_image_memory, depth_image_view) = create_depth_buffer_image(
            &self.allocator,
            &self.instance,
            &self.device,
            self.physical_device,
            self.command_pool,
            self.graphics_queue,
            self.render_extent,
            self.msaa_samples,
        )?;
        self.depth_image = depth_image;
        self.depth_image_memory = depth_image_memory;
        self.depth_image_view = depth_image_view;

        let (color_image, color_image_memory, color_image_view) = create_color_image(
            &self.allocator,
            &self.device,
            self.render_extent,
            self.image_format,
            self.msaa_samples,
        )?;
        self.color_image = color_image;
        self.color_image_memory = color_image_memory;
        self.color_image_view = color_image_view;

        if let Some(picking) = &mut self.picking {
            picking.recreate_images(&self.allocator, &self.device, self.render_extent)?;
        }

        Ok(())
    }

    fn cleanup_swapchain(&mut self) {
        unsafe {
            for image_view in &self.swapchain_image_views {
                self.device.destroy_image_view(*image_view, None);
            }

            // NOTE this also frees the images
            self.swapchain_device_ext
                .destroy_swapchain(self.swapchain, None);
        }
    }

    #[cfg(debug_assertions)]
    fn check_for_shader_recompile<D>(
        &mut self,
        pipeline_handle: &PipelineHandle<D>,
        compute_pipeline_indices: &[usize],
    ) -> Result<(), anyhow::Error> {
        // drop old graphics reloaded pipelines for frames that are no longer needed
        let mut to_remove = vec![];
        for (i, (old_frame, old_pipeline, old_pipeline_layout, old_descriptor_set_layouts)) in
            self.old_pipelines.iter().enumerate()
        {
            let unused = *old_frame < (self.total_frames - MAX_FRAMES_IN_FLIGHT);
            if !unused {
                continue;
            }

            unsafe {
                self.device.destroy_pipeline(*old_pipeline, None);
                self.device
                    .destroy_pipeline_layout(*old_pipeline_layout, None);
            }

            for &desc_set_layout in old_descriptor_set_layouts {
                unsafe {
                    self.device
                        .destroy_descriptor_set_layout(desc_set_layout, None);
                }
            }

            to_remove.push(i);
        }
        to_remove.sort_unstable();
        for i in to_remove.into_iter().rev() {
            self.old_pipelines.swap_remove(i);
        }

        // recompile shaders if necessary
        let edit_events = self.shader_changes.events()?;
        if !edit_events.is_empty() {
            info!("recompiling shaders...");
            self.try_shader_recompile(pipeline_handle, &edit_events)?;
            for &compute_index in compute_pipeline_indices {
                self.try_compute_shader_recompile(compute_index)?;
            }
        }

        Ok(())
    }

    // shader hot reload
    #[cfg(debug_assertions)]
    fn try_shader_recompile<D>(
        &mut self,
        pipeline_handle: &PipelineHandle<D>,
        _edit_events: &[notify::Event],
    ) -> Result<(), anyhow::Error> {
        let mut tmp_pipeline_layout = match ShaderPipelineLayout::create_from_atlas(
            &self.device,
            &*self.renderer_pipeline(pipeline_handle).shader,
        ) {
            Ok(shaders) => shaders,
            Err(e) => {
                error!("failed to compile shaders: {e}");
                return Ok(());
            }
        };

        let render_pipeline_mut = self.pipelines.get_mut(pipeline_handle);

        std::mem::swap(&mut tmp_pipeline_layout, &mut render_pipeline_mut.layout);

        let descriptor_set_layouts = tmp_pipeline_layout
            .descriptor_set_layouts
            .into_iter()
            .map(|t| t.0)
            .collect();
        self.old_pipelines.push((
            self.total_frames,
            render_pipeline_mut.pipeline,
            tmp_pipeline_layout.pipeline_layout,
            descriptor_set_layouts,
        ));

        render_pipeline_mut.pipeline = create_graphics_pipeline(
            &self.device,
            self.image_format,
            Some(self.depth_format),
            self.msaa_samples,
            &render_pipeline_mut.layout,
            &render_pipeline_mut.shader.vertex_binding_descriptions(),
            &render_pipeline_mut.shader.vertex_attribute_descriptions(),
            !render_pipeline_mut.disable_depth_test,
            true,
        )?;

        info!("finished recompiling shaders");

        Ok(())
    }

    #[cfg(debug_assertions)]
    fn try_compute_shader_recompile(
        &mut self,
        compute_pipeline_index: usize,
    ) -> Result<(), anyhow::Error> {
        let compute_pipeline = self.compute_pipelines.get_by_index(compute_pipeline_index);

        let mut tmp_layout = match ComputeShaderPipelineLayout::create_from_atlas(
            &self.device,
            &*compute_pipeline.shader,
        ) {
            Ok(layout) => layout,
            Err(e) => {
                error!("failed to compile compute shader: {e}");
                return Ok(());
            }
        };

        let compute_pipeline_mut = self
            .compute_pipelines
            .get_mut_by_index(compute_pipeline_index);

        std::mem::swap(&mut tmp_layout, &mut compute_pipeline_mut.layout);

        let descriptor_set_layouts = tmp_layout
            .descriptor_set_layouts
            .into_iter()
            .map(|t| t.0)
            .collect();
        self.old_pipelines.push((
            self.total_frames,
            compute_pipeline_mut.pipeline,
            tmp_layout.pipeline_layout,
            descriptor_set_layouts,
        ));

        // Create new compute pipeline
        let shader_module = {
            let shader_module_create_info = vk::ShaderModuleCreateInfo::default()
                .code(&compute_pipeline_mut.layout.compute_shader.spv_bytes);
            unsafe {
                self.device
                    .create_shader_module(&shader_module_create_info, None)?
            }
        };

        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(shader_module)
            .name(&compute_pipeline_mut.layout.compute_shader.entry_point_name);

        let compute_pipeline_create_info = vk::ComputePipelineCreateInfo::default()
            .stage(stage)
            .layout(compute_pipeline_mut.layout.pipeline_layout);

        compute_pipeline_mut.pipeline = unsafe {
            self.device.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[compute_pipeline_create_info],
                None,
            )
        }
        .map_err(|(_pipelines, err)| err)?[0];

        unsafe {
            self.device.destroy_shader_module(shader_module, None);
        }

        let name = compute_pipeline_mut.shader.source_file_name();
        info!("finished recompiling compute shader: {name}");

        Ok(())
    }

    pub fn on_resize(&mut self) -> anyhow::Result<()> {
        self.recreate_swapchain()?;

        let (width, height) = (self.image_extent.width, self.image_extent.height);
        self.aspect_ratio = width as f32 / height as f32;
        self.width = width as f32;
        self.height = height as f32;

        Ok(())
    }

    /// Get mutable access to the egui integration for event handling
    pub fn egui(&mut self) -> Option<&mut EguiIntegration> {
        self.egui.as_mut()
    }

    /// Begin the egui frame early, before command buffer recording.
    /// This allows games to build egui UI before draw_frame is called.
    /// Idempotent: safe to call multiple times per frame.
    pub fn begin_egui_frame(&mut self) {
        if let Some(egui) = &mut self.egui {
            let screen_size = [self.width, self.height];
            egui.begin_frame(screen_size);
        }
    }

    /// Get a clone of the egui context for building UI.
    /// Returns None if egui is disabled.
    pub fn egui_context(&self) -> Option<::egui::Context> {
        self.egui.as_ref().map(|e| e.ctx.clone())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        // this is necessary to avoid validation errors after a panic
        let _ = unsafe { self.device.device_wait_idle() };

        unsafe {
            for semaphore in &self.render_finished {
                self.device.destroy_semaphore(*semaphore, None);
            }
            for semaphore in &self.image_available {
                self.device.destroy_semaphore(*semaphore, None);
            }
            self.device.destroy_semaphore(self.frame_timeline, None);
            self.device.destroy_semaphore(self.compute_timeline, None);

            self.device.destroy_command_pool(self.command_pool, None);

            self.device.destroy_image_view(self.depth_image_view, None);
            self.allocator
                .destroy_image(self.depth_image, &mut self.depth_image_memory);

            self.device.destroy_image_view(self.color_image_view, None);
            self.allocator
                .destroy_image(self.color_image, &mut self.color_image_memory);

            for i in 0..MAX_FRAMES_IN_FLIGHT {
                self.device
                    .destroy_image_view(self.resolve_image_views[i], None);
                self.allocator
                    .destroy_image(self.resolve_images[i], &mut self.resolve_image_memories[i]);
            }

            #[cfg(debug_assertions)]
            for (_frame, old_pipeline, old_pipeline_layout, old_descriptor_set_layouts) in
                &self.old_pipelines
            {
                self.device.destroy_pipeline(*old_pipeline, None);
                self.device
                    .destroy_pipeline_layout(*old_pipeline_layout, None);

                for &desc_set_layout in old_descriptor_set_layouts {
                    self.device
                        .destroy_descriptor_set_layout(desc_set_layout, None);
                }
            }

            if let Some(picking) = self.picking.take() {
                picking.destroy(&self.allocator, &self.device);
            }

            self.cleanup_swapchain();

            for texture in self.textures.take_all() {
                self.destroy_texture(texture);
            }
            for mut storage_texture in self.storage_textures.take_all() {
                self.device
                    .destroy_image_view(storage_texture.image_view, None);
                self.allocator
                    .destroy_image(storage_texture.image, &mut storage_texture.image_memory);
            }
            for pipeline in self.pipelines.take_all() {
                self.destroy_pipeline(pipeline);
            }
            for compute_pipeline in self.compute_pipelines.take_all() {
                self.destroy_compute_pipeline(compute_pipeline);
            }
            for buffers_per_frame in self.uniform_buffers.take_all() {
                for uniform_buffer in buffers_per_frame {
                    self.destroy_uniform_buffer(uniform_buffer);
                }
            }
            for buffers_per_frame in self.storage_buffers.take_all() {
                for storage_buffer in buffers_per_frame {
                    self.destroy_storage_buffer(storage_buffer);
                }
            }

            // Drop egui before device destruction so it can clean up its Vulkan resources
            drop(self.egui.take());

            // All allocations must be freed by this point; VMA reports leaks here.
            std::mem::ManuallyDrop::drop(&mut self.allocator);

            self.device.destroy_device(None);

            // NOTE This must be called before dropping the sdl window,
            // which means that the Renderer must be dropped before the window.
            // That should happen by default, since Renderer::init requires a window,
            // and rust drops variables in reverse initialization order.
            SDL_Vulkan_DestroySurface(self.instance.handle(), self.surface, std::ptr::null());

            if ENABLE_VALIDATION {
                self.debug_loader
                    .destroy_debug_utils_messenger(self.debug_ext, None);
            }

            self.instance.destroy_instance(None);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureFilter {
    Linear,
    Nearest,
}

fn get_required_layers() -> Vec<&'static std::ffi::CStr> {
    if ENABLE_VALIDATION {
        vec![c"VK_LAYER_KHRONOS_validation"]
    } else {
        vec![]
    }
}

fn check_required_layers(entry: &ash::Entry) -> Result<(), anyhow::Error> {
    let required_layers = get_required_layers();
    let available_layers = unsafe { entry.enumerate_instance_layer_properties()? };

    for required_layer in required_layers {
        let mut found = false;
        for prop in &available_layers {
            let layer_name = vk_str_bytes(&prop.layer_name);
            if layer_name == required_layer.to_bytes() {
                found = true;
                break;
            }
        }

        if !found {
            let required_layer = required_layer.to_string_lossy();
            anyhow::bail!("missing required layer: {required_layer}");
        }
    }

    Ok(())
}

fn check_required_extensions(entry: &ash::Entry) -> Result<(), anyhow::Error> {
    let mut required_extensions = vec![ash::khr::surface::NAME, platform::OS_SURFACE_EXT];

    required_extensions.push(ash::ext::debug_utils::NAME);

    let available_extensions = unsafe { entry.enumerate_instance_extension_properties(None)? };

    for required_ext in &required_extensions {
        let mut found = false;
        for prop in &available_extensions {
            let ext_name: Vec<u8> = vk_str_bytes(&prop.extension_name);
            if ext_name == required_ext.to_bytes() {
                found = true;
                break;
            }
        }

        if !found {
            let required_layer = required_ext.to_string_lossy();
            anyhow::bail!("missing required extension: {required_layer}");
        }
    }

    Ok(())
}

/// trims a null-terminated c string from vulkan to only include
/// non-null bytes for comparison with CStr constants
fn vk_str_bytes(vk_str: &[c_char]) -> Vec<u8> {
    vk_str
        .iter()
        .map(|byte| *byte as u8)
        .take_while(|byte| *byte != b'\0')
        .collect()
}

struct QueueFamilyIndices {
    graphics: u32,
    presentation: u32,
    graphics_queue_count: u32,
}

impl QueueFamilyIndices {
    fn find(
        instance: &ash::Instance,
        surface_ext: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Option<Self>, anyhow::Error> {
        let queue_families =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

        let mut graphics = None;
        let mut presentation = None;

        for (i, family) in queue_families.iter().enumerate() {
            // NOTE this also implies vk::QueueFlags::TRANSFER
            if family.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                debug_assert!(
                    family.queue_flags.contains(vk::QueueFlags::COMPUTE),
                    "Graphics queue family must also support compute"
                );
                graphics = Some(i as u32);
            }

            let supports_presentation = unsafe {
                surface_ext.get_physical_device_surface_support(
                    physical_device,
                    i as u32,
                    surface,
                )?
            };
            if supports_presentation {
                presentation = Some(i as u32)
            }
        }

        let indices = match (graphics, presentation) {
            (Some(graphics), Some(presentation)) => {
                let graphics_queue_count = queue_families[graphics as usize].queue_count;
                Some(Self {
                    graphics,
                    presentation,
                    graphics_queue_count,
                })
            }
            _ => None,
        };

        Ok(indices)
    }
}

const REQUIRED_DEVICE_EXTENSIONS: [&CStr; 1] = [
    // always required
    vk::KHR_SWAPCHAIN_NAME,
    // NOTE shader draw parameters (required by slang's generated spirv after 2025.10)
    // is core in 1.1; the feature bit is enabled via PhysicalDeviceVulkan11Features
];

fn choose_physical_device(
    instance: &ash::Instance,
    surface_ext: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> anyhow::Result<(
    vk::PhysicalDevice,
    QueueFamilyIndices,
    vk::PhysicalDeviceProperties,
)> {
    let physical_devices: Vec<vk::PhysicalDevice> =
        unsafe { instance.enumerate_physical_devices()? };

    // this corresponds to the tutorial's 'isDeviceSuitable'
    let mut devices_with_indices_and_props = vec![];
    for physical_device in physical_devices {
        let indices = QueueFamilyIndices::find(instance, surface_ext, surface, physical_device)?;
        let Some(indices) = indices else {
            continue;
        };

        let supports_extensions =
            check_device_extension_support(instance, physical_device, &REQUIRED_DEVICE_EXTENSIONS)?;
        if !supports_extensions {
            continue;
        }

        let swapchain_support =
            SwapChainSupportDetails::query(surface_ext, surface, physical_device)?;
        let swapchain_adequate =
            !swapchain_support.formats.is_empty() && !swapchain_support.present_modes.is_empty();
        if !swapchain_adequate {
            continue;
        }

        let mut vulkan_11_features = vk::PhysicalDeviceVulkan11Features::default();
        let mut vulkan_12_features = vk::PhysicalDeviceVulkan12Features::default();
        let mut vulkan_13_features = vk::PhysicalDeviceVulkan13Features::default();
        let mut features2 = vk::PhysicalDeviceFeatures2::default()
            .push_next(&mut vulkan_11_features)
            .push_next(&mut vulkan_12_features)
            .push_next(&mut vulkan_13_features);
        unsafe { instance.get_physical_device_features2(physical_device, &mut features2) };
        let features = features2.features;

        let missing_features: Vec<&str> = [
            (features.sampler_anisotropy, "samplerAnisotropy"),
            (
                vulkan_11_features.shader_draw_parameters,
                "shaderDrawParameters",
            ),
            (vulkan_12_features.timeline_semaphore, "timelineSemaphore"),
            (
                vulkan_12_features.buffer_device_address,
                "bufferDeviceAddress",
            ),
            (vulkan_13_features.dynamic_rendering, "dynamicRendering"),
            (vulkan_13_features.synchronization2, "synchronization2"),
        ]
        .into_iter()
        .filter(|(supported, _name)| *supported != vk::TRUE)
        .map(|(_supported, name)| name)
        .collect();

        let props = unsafe { instance.get_physical_device_properties(physical_device) };

        if !missing_features.is_empty() {
            log::warn!(
                "skipping device {}: missing required features: {}",
                device_name_as_string(props),
                missing_features.join(", ")
            );
            continue;
        }

        devices_with_indices_and_props.push((physical_device, indices, props));
    }

    devices_with_indices_and_props.sort_by_key(|(_physical_device, _indices, props)| {
        match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => 0,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
            vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
            vk::PhysicalDeviceType::CPU => 3,
            vk::PhysicalDeviceType::OTHER => 4,
            _ => 5,
        }
    });

    let Some(chosen_device) = devices_with_indices_and_props.into_iter().next() else {
        anyhow::bail!(
            "no suitable graphics device available \
             (requires Vulkan 1.3 with dynamicRendering, synchronization2, \
             timelineSemaphore, and bufferDeviceAddress)"
        );
    };

    #[cfg(debug_assertions)]
    {
        let chosen_device_name = device_name_as_string(chosen_device.2);
        log::trace!("using device: {chosen_device_name}");
    }

    Ok(chosen_device)
}

fn device_name_as_string(props: vk::PhysicalDeviceProperties) -> String {
    let device_name_bytes: Vec<u8> = props
        .device_name
        .into_iter()
        .filter(|&i| i != 0)
        .map(|i| i as u8)
        .collect();

    String::from_utf8_lossy(&device_name_bytes).to_string()
}

const PREFERRED_SURFACE_FORMAT: vk::SurfaceFormatKHR = vk::SurfaceFormatKHR {
    format: vk::Format::B8G8R8A8_SRGB,
    color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
};

fn choose_swap_surface_format(swapchain: &SwapChainSupportDetails) -> vk::SurfaceFormatKHR {
    if swapchain.formats.contains(&PREFERRED_SURFACE_FORMAT) {
        return PREFERRED_SURFACE_FORMAT;
    }

    swapchain.fallback_format
}

fn choose_swap_present_mode(available_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    if available_modes.contains(&vk::PresentModeKHR::MAILBOX) {
        // burns battery on mobile, good otherwise
        return vk::PresentModeKHR::MAILBOX;
    }

    // aka vsync; guaranteed to be supported
    vk::PresentModeKHR::FIFO
}

fn check_device_extension_support(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    required_extensions: &[&'static CStr],
) -> Result<bool, anyhow::Error> {
    let mut required_extensions: BTreeSet<Vec<u8>> = required_extensions
        .iter()
        .map(|&cstr| cstr.to_bytes().to_owned())
        .collect();

    let device_ext_props =
        unsafe { instance.enumerate_device_extension_properties(physical_device)? };
    for prop in device_ext_props {
        let bytes = vk_str_bytes(&prop.extension_name);
        required_extensions.remove(&bytes);
    }

    Ok(required_extensions.is_empty())
}

fn choose_swap_extent(window: &Window, capabilities: &vk::SurfaceCapabilitiesKHR) -> vk::Extent2D {
    // u32::MAX is used as a sentinel value that means 'refer to the bounds'
    if capabilities.current_extent.width != u32::MAX {
        return capabilities.current_extent;
    }

    let (sdl_width, sdl_height) = window.size_in_pixels();

    let width = sdl_width.clamp(
        capabilities.min_image_extent.width,
        capabilities.max_image_extent.width,
    );

    let height = sdl_height.clamp(
        capabilities.min_image_extent.height,
        capabilities.max_image_extent.height,
    );

    vk::Extent2D { width, height }
}

struct CreatedSwapchain {
    swapchain: vk::SwapchainKHR,
    image_format: vk::Format,
    image_extent: vk::Extent2D,
}

fn create_swapchain(
    window: &Window,
    swapchain_device_ext: &ash::khr::swapchain::Device,
    surface_ext: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    physical_device: vk::PhysicalDevice,
    queue_family_indices: &QueueFamilyIndices,
) -> Result<CreatedSwapchain, anyhow::Error> {
    let swapchain_support = SwapChainSupportDetails::query(surface_ext, surface, physical_device)?;

    let surface_format = choose_swap_surface_format(&swapchain_support);
    let present_mode = choose_swap_present_mode(&swapchain_support.present_modes);
    let image_extent = choose_swap_extent(window, &swapchain_support.capabilities);

    // the number of images in the swapchain
    // going too low can result in the application blocking on the GPU
    let desired_image_count = swapchain_support.capabilities.min_image_count + 1;
    let max_image_count = swapchain_support.capabilities.max_image_count;
    // 0 is a sentinel value meaning no maximum
    let max_image_count = if max_image_count == 0 {
        u32::MAX
    } else {
        max_image_count
    };
    let image_count = desired_image_count.clamp(0, max_image_count);

    let create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(surface_format.format)
        .image_color_space(surface_format.color_space)
        .image_extent(image_extent)
        .image_array_layers(1) // only not one for stereoscopic 3D (VR?)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST); // TRANSFER_DST for render scaling blit

    let create_info_indices = [
        queue_family_indices.graphics,
        queue_family_indices.presentation,
    ];
    let create_info = if queue_family_indices.graphics != queue_family_indices.presentation {
        // different queue families; the uncommon case
        // the tutorial recommends avoiding concurrent sharing mode if possible
        // but this involves the ownership portion of the vulkan API
        create_info
            .image_sharing_mode(vk::SharingMode::CONCURRENT)
            .queue_family_indices(&create_info_indices)
    } else {
        // same queue family; the common case
        create_info.image_sharing_mode(vk::SharingMode::EXCLUSIVE)
    };

    let create_info = create_info
        // no flip / rotation on swapchain images
        .pre_transform(swapchain_support.capabilities.current_transform)
        // for window transparency
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_mode)
        .clipped(true)
        // used during resizing & similar swapchain recreations
        .old_swapchain(vk::SwapchainKHR::null());

    let swapchain = unsafe { swapchain_device_ext.create_swapchain(&create_info, None)? };

    Ok(CreatedSwapchain {
        swapchain,
        image_format: surface_format.format,
        image_extent,
    })
}

fn create_logical_device(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    indices: &QueueFamilyIndices,
) -> Result<ash::Device, anyhow::Error> {
    let unique_queue_families = BTreeSet::from([indices.graphics, indices.presentation]);

    let mut queue_create_infos = vec![];
    let queue_priorities_single = [1.0];
    let queue_priorities_dual = [1.0, 1.0];
    for index in unique_queue_families {
        // Request 2 queues from the graphics family when available (for async compute)
        let priorities = if index == indices.graphics && indices.graphics_queue_count >= 2 {
            &queue_priorities_dual[..]
        } else {
            &queue_priorities_single[..]
        };
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(index)
            .queue_priorities(priorities);

        queue_create_infos.push(queue_create_info);
    }

    let mut features = vk::PhysicalDeviceFeatures::default()
        .sampler_anisotropy(true)
        .sample_rate_shading(ENABLE_SAMPLE_SHADING);
    if cfg!(debug_assertions) {
        // features used by shader println
        features = features
            .fragment_stores_and_atomics(true)
            .vertex_pipeline_stores_and_atomics(true)
            .shader_int64(true);
    }

    // required by slang's generated spirv after 2025.10
    //   the feature is required by the 2024 roadmap
    //   https://registry.khronos.org/vulkan/specs/latest/html/vkspec.html#profile-features-roadmap-2024
    let mut vulkan_11_features =
        vk::PhysicalDeviceVulkan11Features::default().shader_draw_parameters(true);

    let mut vulkan_12_features = vk::PhysicalDeviceVulkan12Features::default()
        .timeline_semaphore(true)
        .buffer_device_address(true);
    if cfg!(debug_assertions) {
        // features used by shader println
        vulkan_12_features = vulkan_12_features
            .vulkan_memory_model(true)
            .vulkan_memory_model_device_scope(true)
            .storage_buffer8_bit_access(true);
    }

    let mut vulkan_13_features = vk::PhysicalDeviceVulkan13Features::default()
        .dynamic_rendering(true)
        .synchronization2(true);

    let mut features2 = vk::PhysicalDeviceFeatures2::default()
        .features(features)
        .push_next(&mut vulkan_11_features)
        .push_next(&mut vulkan_12_features)
        .push_next(&mut vulkan_13_features);

    let enabled_extension_names: Vec<_> = REQUIRED_DEVICE_EXTENSIONS
        .iter()
        .map(|cstr| cstr.as_ptr())
        .collect();

    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_extension_names(&enabled_extension_names)
        .push_next(&mut features2);

    let device = unsafe { instance.create_device(physical_device, &create_info, None)? };

    Ok(device)
}

struct SwapChainSupportDetails {
    capabilities: vk::SurfaceCapabilitiesKHR,
    formats: Vec<vk::SurfaceFormatKHR>,
    fallback_format: vk::SurfaceFormatKHR,
    present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapChainSupportDetails {
    fn query(
        surface_ext: &ash::khr::surface::Instance,
        surface: vk::SurfaceKHR,
        physical_device: vk::PhysicalDevice,
    ) -> Result<Self, anyhow::Error> {
        let capabilities = unsafe {
            surface_ext.get_physical_device_surface_capabilities(physical_device, surface)?
        };

        let formats =
            unsafe { surface_ext.get_physical_device_surface_formats(physical_device, surface)? };
        let fallback_format = formats
            .first()
            .copied()
            .expect("physical device had no surface formats");

        let present_modes = unsafe {
            surface_ext.get_physical_device_surface_present_modes(physical_device, surface)?
        };

        Ok(Self {
            capabilities,
            formats,
            fallback_format,
            present_modes,
        })
    }
}

fn create_swapchain_image_views(
    device: &ash::Device,
    image_format: vk::Format,
    swapchain_images: &[vk::Image],
) -> Result<Vec<vk::ImageView>, anyhow::Error> {
    let mut swapchain_image_views = Vec::with_capacity(swapchain_images.len());
    for &image in swapchain_images {
        let image_view =
            create_image_view(device, image, image_format, vk::ImageAspectFlags::COLOR, 1)?;
        swapchain_image_views.push(image_view);
    }

    Ok(swapchain_image_views)
}

/// usage: read_shader_spv("triangle.vert.spv");
#[expect(unused)]
fn read_shader_spv(shader_name: &str) -> Result<Vec<u32>, anyhow::Error> {
    let shader_path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "shaders",
        "compiled",
        shader_name,
    ]
    .iter()
    .collect();

    let mut spv_file = BufReader::new(File::open(&shader_path)?);
    let vk_bytes = ash::util::read_spv(&mut spv_file)?;

    Ok(vk_bytes)
}

fn create_graphics_pipeline(
    device: &ash::Device,
    color_format: vk::Format,
    depth_format: Option<vk::Format>,
    msaa_samples: vk::SampleCountFlags,
    pipeline_layout: &ShaderPipelineLayout,
    vertex_binding_descriptions: &[vk::VertexInputBindingDescription],
    vertex_attribute_descriptions: &[vk::VertexInputAttributeDescription],
    depth_test_enable: bool,
    blend_enable: bool,
) -> Result<vk::Pipeline, anyhow::Error> {
    let vert_shader_spv = &pipeline_layout.vertex_shader.spv_bytes;
    let frag_shader_spv = &pipeline_layout.fragment_shader.spv_bytes;

    let vert_create_info = vk::ShaderModuleCreateInfo::default().code(vert_shader_spv);
    let frag_create_info = vk::ShaderModuleCreateInfo::default().code(frag_shader_spv);

    let vert_shader = unsafe { device.create_shader_module(&vert_create_info, None)? };
    let frag_shader = unsafe { device.create_shader_module(&frag_create_info, None)? };

    let vert_create_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::VERTEX)
        .module(vert_shader)
        .name(&pipeline_layout.vertex_shader.entry_point_name);
    let frag_create_info = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::FRAGMENT)
        .module(frag_shader)
        .name(&pipeline_layout.fragment_shader.entry_point_name);
    let stages = [vert_create_info, frag_create_info];

    let dynamic_states = vec![vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    let vertex_input_state = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(vertex_binding_descriptions)
        .vertex_attribute_descriptions(vertex_attribute_descriptions);

    let input_assembly_state = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        .primitive_restart_enable(false);

    // relying on dynamic state to fill these in during draw
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);

    let rasterization_state = vk::PipelineRasterizationStateCreateInfo::default()
        .depth_clamp_enable(false)
        .rasterizer_discard_enable(false)
        .polygon_mode(vk::PolygonMode::FILL)
        .line_width(1.0)
        .cull_mode(vk::CullModeFlags::BACK)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .depth_bias_enable(false);

    let multisample_state = vk::PipelineMultisampleStateCreateInfo::default()
        .sample_shading_enable(ENABLE_SAMPLE_SHADING)
        .min_sample_shading(if ENABLE_SAMPLE_SHADING { 0.2 } else { 0.0 })
        .rasterization_samples(msaa_samples);

    // color blend per attached framebuffer
    let color_blend_attachment = vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(blend_enable)
        .color_blend_op(vk::BlendOp::ADD)
        .alpha_blend_op(vk::BlendOp::ADD)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .src_alpha_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_write_mask(vk::ColorComponentFlags::RGBA);

    let color_attachments = [color_blend_attachment];
    // global color blending
    let color_blend_state = vk::PipelineColorBlendStateCreateInfo::default()
        .logic_op_enable(false)
        .attachments(&color_attachments);

    let depth_stencil_state = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(depth_test_enable)
        .depth_write_enable(true)
        .depth_compare_op(vk::CompareOp::LESS)
        .depth_bounds_test_enable(false)
        .stencil_test_enable(false);

    let color_attachment_formats = [color_format];
    let mut rendering_info = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(&color_attachment_formats)
        .depth_attachment_format(depth_format.unwrap_or(vk::Format::UNDEFINED));

    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input_state)
        .input_assembly_state(&input_assembly_state)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization_state)
        .multisample_state(&multisample_state)
        .color_blend_state(&color_blend_state)
        .dynamic_state(&dynamic_state)
        .layout(pipeline_layout.pipeline_layout)
        .depth_stencil_state(&depth_stencil_state)
        .push_next(&mut rendering_info);

    let graphics_pipelines = unsafe {
        device
            .create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
            .map_err(|e| anyhow::anyhow!("failed to create graphics pipelines: {e:?}"))?
    };
    let graphics_pipeline = graphics_pipelines[0];

    unsafe { device.destroy_shader_module(frag_shader, None) };
    unsafe { device.destroy_shader_module(vert_shader, None) };

    Ok(graphics_pipeline)
}

fn create_command_pool(
    device: &ash::Device,
    queue_family_indicies: &QueueFamilyIndices,
) -> Result<vk::CommandPool, anyhow::Error> {
    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(queue_family_indicies.graphics);

    let command_pool = unsafe { device.create_command_pool(&pool_info, None)? };

    Ok(command_pool)
}

fn create_command_buffers(
    device: &ash::Device,
    command_pool: vk::CommandPool,
) -> Result<[vk::CommandBuffer; MAX_FRAMES_IN_FLIGHT], anyhow::Error> {
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(MAX_FRAMES_IN_FLIGHT as u32);

    let buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };

    Ok(buffers.try_into().unwrap())
}

fn create_sync_objects(
    device: &ash::Device,
    swapchain_images: &[vk::Image],
) -> Result<
    (
        [vk::Semaphore; PRE_WAIT_RING_LEN],
        Vec<vk::Semaphore>,
        vk::Semaphore,
        vk::Semaphore,
    ),
    anyhow::Error,
> {
    let mut image_available: [Option<vk::Semaphore>; PRE_WAIT_RING_LEN] =
        [const { None }; PRE_WAIT_RING_LEN];
    #[expect(clippy::needless_range_loop)]
    for i in 0..PRE_WAIT_RING_LEN {
        image_available[i] = Some(unsafe { device.create_semaphore(&Default::default(), None)? });
    }
    let image_available = image_available.map(Option::unwrap);

    let mut render_finished = Vec::with_capacity(swapchain_images.len());
    for _image in swapchain_images {
        let semaphore = unsafe { device.create_semaphore(&Default::default(), None)? };
        render_finished.push(semaphore);
    }

    let frame_timeline = create_timeline_semaphore(device)?;
    let compute_timeline = create_timeline_semaphore(device)?;

    Ok((
        image_available,
        render_finished,
        frame_timeline,
        compute_timeline,
    ))
}

fn create_timeline_semaphore(device: &ash::Device) -> Result<vk::Semaphore, anyhow::Error> {
    let mut type_info = vk::SemaphoreTypeCreateInfo::default()
        .semaphore_type(vk::SemaphoreType::TIMELINE)
        .initial_value(0);
    let create_info = vk::SemaphoreCreateInfo::default().push_next(&mut type_info);
    Ok(unsafe { device.create_semaphore(&create_info, None)? })
}

fn create_vertex_buffer<V: GPUWrite>(
    allocator: &vk_mem::Allocator,
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    vertices: &[V],
) -> Result<(vk::Buffer, vk_mem::Allocation), anyhow::Error> {
    let buffer_size = std::mem::size_of_val(vertices) as u64;

    let (staging_buffer, mut staging_buffer_memory) = create_memory_buffer(
        allocator,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        BufferMemory::Staging,
    )?;

    unsafe { write_to_gpu_buffer(allocator, &mut staging_buffer_memory, vertices)? };

    let (vertex_buffer, vertex_buffer_memory) = create_memory_buffer(
        allocator,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
        BufferMemory::DeviceLocal,
    )?;

    copy_memory_buffer(
        device,
        command_pool,
        staging_buffer,
        vertex_buffer,
        buffer_size,
        graphics_queue,
    )?;

    unsafe {
        allocator.destroy_buffer(staging_buffer, &mut staging_buffer_memory);
    }

    Ok((vertex_buffer, vertex_buffer_memory))
}

fn create_index_buffer(
    allocator: &vk_mem::Allocator,
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    indices: &[u32],
) -> Result<(vk::Buffer, vk_mem::Allocation), anyhow::Error> {
    let buffer_size = std::mem::size_of_val(indices) as u64;
    let (staging_buffer, mut staging_buffer_memory) = create_memory_buffer(
        allocator,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        BufferMemory::Staging,
    )?;

    unsafe { write_to_gpu_buffer(allocator, &mut staging_buffer_memory, indices)? };

    let (index_buffer, index_buffer_memory) = create_memory_buffer(
        allocator,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER,
        BufferMemory::DeviceLocal,
    )?;

    copy_memory_buffer(
        device,
        command_pool,
        staging_buffer,
        index_buffer,
        buffer_size,
        graphics_queue,
    )?;

    unsafe {
        allocator.destroy_buffer(staging_buffer, &mut staging_buffer_memory);
    }

    Ok((index_buffer, index_buffer_memory))
}

fn copy_memory_buffer(
    device: &ash::Device,
    command_pool: vk::CommandPool,
    src_buffer: vk::Buffer,
    dst_buffer: vk::Buffer,
    size: vk::DeviceSize,
    graphics_queue: vk::Queue,
) -> Result<(), anyhow::Error> {
    // NOTE it would be better to have a second command pool for transfers,
    // that also uses 'create transient'
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    let regions = [vk::BufferCopy::default().size(size)];
    unsafe { device.cmd_copy_buffer(command_buffer, src_buffer, dst_buffer, &regions) };

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

/// How a buffer's memory will be accessed, translated to VMA allocation flags.
#[derive(Clone, Copy)]
pub(super) enum BufferMemory {
    /// GPU-only memory (vertex/index buffer targets)
    DeviceLocal,
    /// CPU-written upload source, transiently mapped and destroyed after the copy
    Staging,
    /// CPU-written per-frame buffer, persistently mapped (uniform/storage buffers)
    PersistentlyMapped,
    /// GPU-written, CPU-read, persistently mapped (picking readback)
    Readback,
}

impl BufferMemory {
    fn allocation_create_info(self) -> vk_mem::AllocationCreateInfo {
        use vk_mem::{AllocationCreateFlags as Flags, MemoryUsage};

        let mut info = vk_mem::AllocationCreateInfo {
            usage: MemoryUsage::AutoPreferDevice,
            ..Default::default()
        };

        match self {
            Self::DeviceLocal => {}
            Self::Staging => {
                info.usage = MemoryUsage::AutoPreferHost;
                info.flags = Flags::HOST_ACCESS_SEQUENTIAL_WRITE;
            }
            Self::PersistentlyMapped => {
                info.usage = MemoryUsage::Auto;
                info.flags = Flags::HOST_ACCESS_SEQUENTIAL_WRITE | Flags::MAPPED;
            }
            Self::Readback => {
                info.usage = MemoryUsage::AutoPreferHost;
                info.flags = Flags::HOST_ACCESS_RANDOM | Flags::MAPPED;
            }
        }

        // Mapped writes never flush, so require coherent memory for all host access
        if !matches!(self, Self::DeviceLocal) {
            info.required_flags = vk::MemoryPropertyFlags::HOST_COHERENT;
        }

        info
    }
}

pub(super) fn create_memory_buffer(
    allocator: &vk_mem::Allocator,
    buffer_size: vk::DeviceSize,
    buffer_usage: vk::BufferUsageFlags,
    memory: BufferMemory,
) -> Result<(vk::Buffer, vk_mem::Allocation), anyhow::Error> {
    let buffer_create_info = vk::BufferCreateInfo::default()
        .size(buffer_size)
        .usage(buffer_usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);

    let allocation_create_info = memory.allocation_create_info();
    let (buffer, allocation) =
        unsafe { allocator.create_buffer(&buffer_create_info, &allocation_create_info)? };

    Ok((buffer, allocation))
}

fn descriptor_pool_sizes(
    sets_across_frames: u32,
    total_counts: &DescriptorCounts,
) -> Vec<vk::DescriptorPoolSize> {
    [
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(sets_across_frames * total_counts.uniform_buffers),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(sets_across_frames * total_counts.combined_texture_samplers),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(sets_across_frames * total_counts.sampled_images),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(sets_across_frames * total_counts.storage_images),
    ]
    .into_iter()
    .filter(|s| s.descriptor_count != 0)
    .collect()
}

fn create_descriptor_pool_from_layouts(
    device: &ash::Device,
    descriptor_set_layouts: &[(ash::vk::DescriptorSetLayout, DescriptorCounts)],
) -> Result<vk::DescriptorPool, anyhow::Error> {
    let descriptor_sets_per_frame = descriptor_set_layouts.len() as u32;
    let sets_across_frames = descriptor_sets_per_frame * PRE_WAIT_RING_LEN as u32;

    let total_counts: DescriptorCounts = descriptor_set_layouts.iter().map(|tup| tup.1).sum();

    let pool_sizes = descriptor_pool_sizes(sets_across_frames, &total_counts);

    let pool_create_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(sets_across_frames);

    let pool = unsafe { device.create_descriptor_pool(&pool_create_info, None)? };

    Ok(pool)
}

fn create_descriptor_pool(
    device: &ash::Device,
    pipeline_layout: &ShaderPipelineLayout,
) -> Result<vk::DescriptorPool, anyhow::Error> {
    let descriptor_sets_per_frame = pipeline_layout.descriptor_set_layouts.len() as u32;
    let sets_across_frames = descriptor_sets_per_frame * PRE_WAIT_RING_LEN as u32;

    let total_counts: DescriptorCounts = pipeline_layout
        .descriptor_set_layouts
        .iter()
        .map(|tup| tup.1)
        .sum();

    let pool_sizes = descriptor_pool_sizes(sets_across_frames, &total_counts);

    let pool_create_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(sets_across_frames);

    let pool = unsafe { device.create_descriptor_pool(&pool_create_info, None)? };

    Ok(pool)
}

#[derive(Debug)]
pub enum LayoutDescription {
    Uniform(UniformBufferDescription),
    Texture(TextureDescription),
    StorageImage(StorageImageDescription),
}

#[derive(Debug)]
pub struct UniformBufferDescription {
    pub size: u64,
    pub binding: u32,
    // the number of descriptors in the descriptor set
    pub descriptor_count: u32,
}

#[derive(Debug)]
pub struct TextureDescription {
    pub binding: u32,
    // the number of descriptors in the descriptor set
    pub descriptor_count: u32,
    // true for SAMPLED_IMAGE (separate texture), false for COMBINED_IMAGE_SAMPLER
    pub sampled_image_only: bool,
}

#[derive(Debug)]
pub struct StorageImageDescription {
    pub layout: vk::ImageLayout,
    pub binding: u32,
    pub descriptor_count: u32,
}

fn create_descriptor_sets(
    device: &ash::Device,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layouts: &[vk::DescriptorSetLayout],
    uniform_buffers_in_layout_frame_order: &[&[RawUniformBuffer; PRE_WAIT_RING_LEN]],
    textures: &[&Texture],
    storage_images: &[&storage_texture::StorageTexture],
    layout_bindings: Vec<Vec<LayoutDescription>>,
) -> Result<Vec<vk::DescriptorSet>, anyhow::Error> {
    // this vec and the resulting vec of descriptor sets are arranged like this:
    // [
    //     frame_0_set_0_binding_0,
    //     frame_0_set_0_binding_1,
    //     frame_0_set_1_binding_0,
    //     frame_0_set_1_binding_1,
    //     frame_1_set_0_binding_0,
    //     frame_1_set_0_binding_1,
    //     frame_1_set_1_binding_0,
    //     frame_1_set_1_binding_1,
    // ]
    let mut set_layouts = vec![];
    for _frame in 0..PRE_WAIT_RING_LEN {
        for &descriptor_set_layout in descriptor_set_layouts {
            // i = frame * descriptor_set_layouts.len() + layout_offset;
            set_layouts.push(descriptor_set_layout);
        }
    }
    let alloc_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&set_layouts);
    let descriptor_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };

    for frame in 0..PRE_WAIT_RING_LEN {
        let mut uniform_buffer_index = 0;
        let mut texture_index = 0;
        let mut storage_image_index = 0;

        #[expect(clippy::needless_range_loop)]
        for layout_offset in 0..descriptor_set_layouts.len() {
            let ds = frame * descriptor_set_layouts.len() + layout_offset;
            let dst_set = descriptor_sets[ds];
            let layout_descriptions = &layout_bindings[layout_offset];

            for description in layout_descriptions {
                match description {
                    LayoutDescription::Uniform(uniform_buffer_description) => {
                        let raw_uniform_buffers_by_frame =
                            uniform_buffers_in_layout_frame_order[uniform_buffer_index];
                        let uniform_buffer = raw_uniform_buffers_by_frame[frame].buffer;

                        let buffer_info = vk::DescriptorBufferInfo::default()
                            .offset(0)
                            .buffer(uniform_buffer)
                            .range(uniform_buffer_description.size);
                        let buffer_info = [buffer_info];
                        let uniform_buffer_write = vk::WriteDescriptorSet::default()
                            .dst_set(dst_set)
                            .dst_binding(uniform_buffer_description.binding)
                            .dst_array_element(0)
                            .descriptor_type(vk::DescriptorType::UNIFORM_BUFFER)
                            .descriptor_count(uniform_buffer_description.descriptor_count)
                            .buffer_info(&buffer_info);

                        let writes = [uniform_buffer_write];
                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                        uniform_buffer_index += 1;
                    }

                    LayoutDescription::Texture(texture_description) => {
                        let texture = textures[texture_index];

                        let descriptor_type = if texture_description.sampled_image_only {
                            vk::DescriptorType::SAMPLED_IMAGE
                        } else {
                            vk::DescriptorType::COMBINED_IMAGE_SAMPLER
                        };

                        let image_info = vk::DescriptorImageInfo::default()
                            .image_layout(texture.image_layout)
                            .image_view(texture.image_view)
                            .sampler(texture.sampler);
                        let image_info = [image_info];
                        let image_write = vk::WriteDescriptorSet::default()
                            .dst_set(dst_set)
                            .dst_binding(texture_description.binding)
                            .dst_array_element(0)
                            .descriptor_type(descriptor_type)
                            .descriptor_count(texture_description.descriptor_count)
                            .image_info(&image_info);

                        let writes = [image_write];
                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                        texture_index += 1;
                    }

                    LayoutDescription::StorageImage(storage_image_description) => {
                        let storage_image = storage_images[storage_image_index];

                        let image_info = vk::DescriptorImageInfo::default()
                            .image_layout(storage_image_description.layout)
                            .image_view(storage_image.image_view);
                        let image_info = [image_info];
                        let image_write = vk::WriteDescriptorSet::default()
                            .dst_set(dst_set)
                            .dst_binding(storage_image_description.binding)
                            .dst_array_element(0)
                            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
                            .descriptor_count(storage_image_description.descriptor_count)
                            .image_info(&image_info);

                        let writes = [image_write];
                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                        storage_image_index += 1;
                    }
                }
            }
        }
    }

    Ok(descriptor_sets)
}

const TEXTURE_IMAGE_FORMAT: ash::vk::Format = ash::vk::Format::R8G8B8A8_SRGB;

/// The memory layout of a texture format,
/// in terms of the block unit used for size and alignment calculations.
/// For uncompressed formats, a 'block' is a single texel.
#[derive(Debug, Clone, Copy)]
pub struct FormatBlockInfo {
    pub block_bytes: u32,
    /// texels per block horizontally; 1 for uncompressed formats
    pub block_width: u32,
    /// texels per block vertically; 1 for uncompressed formats
    pub block_height: u32,
}

/// The whitelist of texture formats supported for pre-baked mip uploads.
/// Block-compressed formats (eg. BC7) can be added here as needed.
pub fn format_block_info(format: vk::Format) -> Option<FormatBlockInfo> {
    match format {
        vk::Format::R8G8B8A8_SRGB | vk::Format::R8G8B8A8_UNORM => Some(FormatBlockInfo {
            block_bytes: 4,
            block_width: 1,
            block_height: 1,
        }),

        _ => None,
    }
}

fn create_texture(
    source_file_name: String,
    input_image: &image::DynamicImage,
    allocator: &vk_mem::Allocator,
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    physical_device_properties: vk::PhysicalDeviceProperties,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    texture_filter: TextureFilter,
) -> anyhow::Result<Texture> {
    let (texture_image, texture_image_memory, mip_levels) = create_texture_image(
        input_image,
        allocator,
        instance,
        device,
        physical_device,
        command_pool,
        graphics_queue,
    )?;

    let texture_image_view = create_image_view(
        device,
        texture_image,
        TEXTURE_IMAGE_FORMAT,
        vk::ImageAspectFlags::COLOR,
        mip_levels,
    )?;

    let texture_sampler =
        create_texture_sampler(device, physical_device_properties, texture_filter)?;

    Ok(Texture {
        source_file_name,
        image: texture_image,
        image_ownership: texture::ImageOwnership::Owned(texture_image_memory),
        mip_levels,
        image_view: texture_image_view,
        sampler: texture_sampler,
        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    })
}

fn create_texture_image(
    image: &image::DynamicImage,
    allocator: &vk_mem::Allocator,
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
) -> Result<(vk::Image, vk_mem::Allocation, u32), anyhow::Error> {
    let bytes = image.to_rgba8().into_raw();
    debug_assert!(
        bytes.len() == (image.width() * image.height() * 4) as usize,
        "expected rgba bytes size"
    );

    let mip_levels = image.width().max(image.height()).ilog2() + 1;

    let buffer_size = bytes.len() as u64;
    let (staging_buffer, mut staging_buffer_memory) = create_memory_buffer(
        allocator,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        BufferMemory::Staging,
    )?;

    unsafe { write_to_gpu_buffer(allocator, &mut staging_buffer_memory, &bytes)? };

    let extent = vk::Extent2D::default()
        .width(image.width())
        .height(image.height());
    let image_options = ImageOptions {
        extent,
        format: TEXTURE_IMAGE_FORMAT,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::SAMPLED
            | vk::ImageUsageFlags::TRANSFER_SRC, // for mipmap
        mip_levels,
        msaa_samples: vk::SampleCountFlags::TYPE_1,
    };
    let (vk_image, image_memory) = create_vk_image(allocator, image_options)?;

    transition_image_layout(
        device,
        command_pool,
        graphics_queue,
        vk_image,
        TEXTURE_IMAGE_FORMAT,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        mip_levels,
    )?;

    let image_subresource = vk::ImageSubresourceLayers::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .mip_level(0)
        .base_array_layer(0)
        .layer_count(1);
    let region = vk::BufferImageCopy::default()
        .buffer_offset(0)
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(image_subresource)
        .image_offset(vk::Offset3D::default())
        .image_extent(extent.into());

    copy_buffer_to_image(
        device,
        command_pool,
        graphics_queue,
        staging_buffer,
        vk_image,
        &[region],
    )?;

    generate_mipmaps(
        device,
        command_pool,
        graphics_queue,
        vk_image,
        (extent.width as i32, extent.height as i32),
        mip_levels,
        instance,
        physical_device,
        TEXTURE_IMAGE_FORMAT,
    )?;

    unsafe {
        allocator.destroy_buffer(staging_buffer, &mut staging_buffer_memory);
    }

    Ok((vk_image, image_memory, mip_levels))
}

fn create_texture_from_mips(
    source_file_name: String,
    format: vk::Format,
    extent: vk::Extent2D,
    mip_data: &[&[u8]],
    allocator: &vk_mem::Allocator,
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    physical_device_properties: vk::PhysicalDeviceProperties,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    texture_filter: TextureFilter,
) -> anyhow::Result<Texture> {
    let format_properties =
        unsafe { instance.get_physical_device_format_properties(physical_device, format) };
    let mut required_features =
        vk::FormatFeatureFlags::SAMPLED_IMAGE | vk::FormatFeatureFlags::TRANSFER_DST;
    if texture_filter == TextureFilter::Linear {
        required_features |= vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR;
    }
    if !format_properties
        .optimal_tiling_features
        .contains(required_features)
    {
        anyhow::bail!("format {format:?} does not support sampling on this device");
    }

    let (texture_image, texture_image_memory) = create_texture_image_from_mips(
        format,
        extent,
        mip_data,
        allocator,
        device,
        command_pool,
        graphics_queue,
    )?;

    let mip_levels = mip_data.len() as u32;
    let texture_image_view = create_image_view(
        device,
        texture_image,
        format,
        vk::ImageAspectFlags::COLOR,
        mip_levels,
    )?;

    let texture_sampler =
        create_texture_sampler(device, physical_device_properties, texture_filter)?;

    Ok(Texture {
        source_file_name,
        image: texture_image,
        image_ownership: texture::ImageOwnership::Owned(texture_image_memory),
        mip_levels,
        image_view: texture_image_view,
        sampler: texture_sampler,
        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
    })
}

fn create_texture_image_from_mips(
    format: vk::Format,
    extent: vk::Extent2D,
    mip_data: &[&[u8]],
    allocator: &vk_mem::Allocator,
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
) -> Result<(vk::Image, vk_mem::Allocation), anyhow::Error> {
    anyhow::ensure!(!mip_data.is_empty(), "expected at least one mip level");
    let mip_levels = mip_data.len() as u32;
    anyhow::ensure!(
        mip_levels <= extent.width.max(extent.height).ilog2() + 1,
        "too many mip levels for extent"
    );

    let block = format_block_info(format)
        .ok_or_else(|| anyhow::anyhow!("unsupported texture format: {format:?}"))?;

    // vkCmdCopyBufferToImage requires each region's bufferOffset to be
    // a multiple of the texel block size and of 4
    let offset_alignment = (block.block_bytes as u64).max(4);
    let mut level_offsets = Vec::with_capacity(mip_data.len());
    let mut buffer_size: u64 = 0;
    for level in mip_data {
        let offset = buffer_size.next_multiple_of(offset_alignment);
        level_offsets.push(offset);
        buffer_size = offset + level.len() as u64;
    }

    let (staging_buffer, mut staging_buffer_memory) = create_memory_buffer(
        allocator,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        BufferMemory::Staging,
    )?;

    unsafe {
        let mapped_dst = allocator.map_memory(&mut staging_buffer_memory)?;
        for (level, &offset) in mip_data.iter().zip(&level_offsets) {
            std::ptr::copy_nonoverlapping(
                level.as_ptr(),
                mapped_dst.add(offset as usize),
                level.len(),
            );
        }
        allocator.unmap_memory(&mut staging_buffer_memory);
    }

    let image_options = ImageOptions {
        extent,
        format,
        tiling: vk::ImageTiling::OPTIMAL,
        // no TRANSFER_SRC: the mip levels are uploaded directly, not blitted
        usage: vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        mip_levels,
        msaa_samples: vk::SampleCountFlags::TYPE_1,
    };
    let (vk_image, image_memory) = create_vk_image(allocator, image_options)?;

    transition_image_layout(
        device,
        command_pool,
        graphics_queue,
        vk_image,
        format,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        mip_levels,
    )?;

    let regions: Vec<vk::BufferImageCopy> = level_offsets
        .iter()
        .enumerate()
        .map(|(i, &offset)| {
            let image_subresource = vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .mip_level(i as u32)
                .base_array_layer(0)
                .layer_count(1);

            let mip_extent = vk::Extent3D {
                width: (extent.width >> i).max(1),
                height: (extent.height >> i).max(1),
                depth: 1,
            };

            vk::BufferImageCopy::default()
                .buffer_offset(offset)
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_subresource(image_subresource)
                .image_offset(vk::Offset3D::default())
                .image_extent(mip_extent)
        })
        .collect();

    copy_buffer_to_image(
        device,
        command_pool,
        graphics_queue,
        staging_buffer,
        vk_image,
        &regions,
    )?;

    transition_image_layout(
        device,
        command_pool,
        graphics_queue,
        vk_image,
        format,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        mip_levels,
    )?;

    unsafe {
        allocator.destroy_buffer(staging_buffer, &mut staging_buffer_memory);
    }

    Ok((vk_image, image_memory))
}

#[derive(Clone, Copy)]
pub(super) struct ImageOptions {
    extent: vk::Extent2D,
    format: vk::Format,
    tiling: vk::ImageTiling,
    usage: vk::ImageUsageFlags,
    mip_levels: u32,
    msaa_samples: vk::SampleCountFlags,
}

pub(super) fn create_vk_image(
    allocator: &vk_mem::Allocator,
    options: ImageOptions,
) -> Result<(vk::Image, vk_mem::Allocation), anyhow::Error> {
    let image_create_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .extent(options.extent.into())
        .mip_levels(options.mip_levels)
        .array_layers(1)
        .format(options.format)
        .tiling(options.tiling)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .usage(options.usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .samples(options.msaa_samples);

    let allocation_create_info = vk_mem::AllocationCreateInfo {
        usage: vk_mem::MemoryUsage::AutoPreferDevice,
        ..Default::default()
    };
    let (vk_image, allocation) =
        unsafe { allocator.create_image(&image_create_info, &allocation_create_info)? };

    Ok((vk_image, allocation))
}

fn begin_single_time_commands(
    device: &ash::Device,
    command_pool: vk::CommandPool,
) -> Result<vk::CommandBuffer, anyhow::Error> {
    let command_buffer_allocate_info = vk::CommandBufferAllocateInfo::default()
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_pool(command_pool)
        .command_buffer_count(1);
    let command_buffers =
        unsafe { device.allocate_command_buffers(&command_buffer_allocate_info)? };
    let command_buffer = command_buffers[0];

    let begin_info =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    unsafe { device.begin_command_buffer(command_buffer, &begin_info)? };

    Ok(command_buffer)
}

fn end_single_time_commands(
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    command_buffer: vk::CommandBuffer,
) -> Result<(), anyhow::Error> {
    let command_buffers = [command_buffer];

    unsafe { device.end_command_buffer(command_buffer)? };

    let command_buffer_infos =
        [vk::CommandBufferSubmitInfo::default().command_buffer(command_buffer)];
    let submit_info = vk::SubmitInfo2::default().command_buffer_infos(&command_buffer_infos);
    let submits = [submit_info];
    unsafe {
        device.queue_submit2(graphics_queue, &submits, vk::Fence::null())?;
        device.device_wait_idle()?;
    }
    unsafe { device.free_command_buffers(command_pool, &command_buffers) };

    Ok(())
}

fn cmd_barrier2(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image_barriers: &[vk::ImageMemoryBarrier2],
) {
    let dependency_info = vk::DependencyInfo::default().image_memory_barriers(image_barriers);
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dependency_info) };
}

fn transition_image_layout(
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    image: vk::Image,
    format: vk::Format,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
    mip_levels: u32,
) -> Result<(), anyhow::Error> {
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    let subresource_range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .base_mip_level(0)
        .level_count(mip_levels)
        .base_array_layer(0)
        .layer_count(1);
    let mut barrier = vk::ImageMemoryBarrier2::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(subresource_range);

    match (old_layout, new_layout) {
        (vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => {
            barrier.src_stage_mask = vk::PipelineStageFlags2::NONE;
            barrier.src_access_mask = vk::AccessFlags2::NONE;

            barrier.dst_stage_mask = vk::PipelineStageFlags2::ALL_TRANSFER;
            barrier.dst_access_mask = vk::AccessFlags2::TRANSFER_WRITE;
        }

        (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL) => {
            barrier.src_stage_mask = vk::PipelineStageFlags2::ALL_TRANSFER;
            barrier.src_access_mask = vk::AccessFlags2::TRANSFER_WRITE;

            barrier.dst_stage_mask = vk::PipelineStageFlags2::FRAGMENT_SHADER;
            barrier.dst_access_mask = vk::AccessFlags2::SHADER_READ;
        }

        (vk::ImageLayout::UNDEFINED, vk::ImageLayout::GENERAL) => {
            barrier.src_stage_mask = vk::PipelineStageFlags2::NONE;
            barrier.src_access_mask = vk::AccessFlags2::NONE;

            barrier.dst_stage_mask = vk::PipelineStageFlags2::COMPUTE_SHADER;
            barrier.dst_access_mask = vk::AccessFlags2::NONE;
        }

        (vk::ImageLayout::UNDEFINED, vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL) => {
            barrier.subresource_range.aspect_mask = vk::ImageAspectFlags::DEPTH;

            if has_stencil_component(format) {
                barrier.subresource_range.aspect_mask |= vk::ImageAspectFlags::STENCIL;
            }

            barrier.src_stage_mask = vk::PipelineStageFlags2::NONE;
            barrier.src_access_mask = vk::AccessFlags2::NONE;

            barrier.dst_stage_mask = vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS;
            barrier.dst_access_mask = vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE;
        }

        transition => {
            anyhow::bail!("layout transition: {transition:?} not supported");
        }
    }

    // https://docs.vulkan.org/spec/latest/chapters/synchronization.html#synchronization-access-types-supported
    cmd_barrier2(device, command_buffer, &[barrier]);

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

fn copy_buffer_to_image(
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    buffer: vk::Buffer,
    image: vk::Image,
    regions: &[vk::BufferImageCopy],
) -> Result<(), anyhow::Error> {
    let command_buffer = begin_single_time_commands(device, command_pool)?;

    unsafe {
        device.cmd_copy_buffer_to_image(
            command_buffer,
            buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            regions,
        )
    };

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

pub(super) fn create_image_view(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
    aspect_mask: vk::ImageAspectFlags,
    mip_levels: u32,
) -> Result<vk::ImageView, anyhow::Error> {
    let components = vk::ComponentMapping::default()
        // NOTE these are the default
        .r(vk::ComponentSwizzle::IDENTITY)
        .g(vk::ComponentSwizzle::IDENTITY)
        .b(vk::ComponentSwizzle::IDENTITY)
        .a(vk::ComponentSwizzle::IDENTITY);

    let subresource_range = vk::ImageSubresourceRange::default()
        .aspect_mask(aspect_mask)
        .base_mip_level(0)
        .level_count(mip_levels)
        .base_array_layer(0)
        .layer_count(1);

    let create_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .components(components)
        .subresource_range(subresource_range);

    let image_view = unsafe { device.create_image_view(&create_info, None)? };

    Ok(image_view)
}

fn create_texture_sampler(
    device: &ash::Device,
    physical_device_properties: vk::PhysicalDeviceProperties,
    texture_filter: TextureFilter,
) -> Result<vk::Sampler, anyhow::Error> {
    let filter = match texture_filter {
        TextureFilter::Linear => vk::Filter::LINEAR,
        TextureFilter::Nearest => vk::Filter::NEAREST,
    };
    let max_anisotropy = physical_device_properties.limits.max_sampler_anisotropy;
    let create_info = vk::SamplerCreateInfo::default()
        .mag_filter(filter)
        .min_filter(filter)
        .address_mode_u(vk::SamplerAddressMode::REPEAT)
        .address_mode_v(vk::SamplerAddressMode::REPEAT)
        .address_mode_w(vk::SamplerAddressMode::REPEAT)
        .anisotropy_enable(true)
        .max_anisotropy(max_anisotropy)
        .border_color(vk::BorderColor::INT_OPAQUE_BLACK)
        .unnormalized_coordinates(false)
        .compare_enable(false)
        .compare_op(vk::CompareOp::ALWAYS)
        .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
        .mip_lod_bias(0.0)
        .min_lod(0.0)
        .max_lod(vk::LOD_CLAMP_NONE);

    let sampler = unsafe { device.create_sampler(&create_info, None)? };

    Ok(sampler)
}

fn create_depth_buffer_image(
    allocator: &vk_mem::Allocator,
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    swapchain_extent: vk::Extent2D,
    msaa_samples: vk::SampleCountFlags,
) -> Result<(vk::Image, vk_mem::Allocation, vk::ImageView), anyhow::Error> {
    let depth_format = find_depth_format(instance, physical_device);

    let mip_levels = 1;

    let image_options = ImageOptions {
        extent: swapchain_extent,
        format: depth_format,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
        mip_levels,
        msaa_samples,
    };

    let (depth_image, depth_image_memory) = create_vk_image(allocator, image_options)?;

    let depth_image_view = create_image_view(
        device,
        depth_image,
        depth_format,
        vk::ImageAspectFlags::DEPTH,
        mip_levels,
    )?;

    transition_image_layout(
        device,
        command_pool,
        graphics_queue,
        depth_image,
        depth_format,
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        mip_levels,
    )?;

    Ok((depth_image, depth_image_memory, depth_image_view))
}

fn find_supported_format(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    candidates: &[vk::Format],
    tiling: vk::ImageTiling,
    features: vk::FormatFeatureFlags,
) -> Option<vk::Format> {
    for &format in candidates {
        let format_properties =
            unsafe { instance.get_physical_device_format_properties(physical_device, format) };

        if tiling == vk::ImageTiling::LINEAR
            && (format_properties.linear_tiling_features & features) == features
        {
            return Some(format);
        }

        if tiling == vk::ImageTiling::OPTIMAL
            && (format_properties.optimal_tiling_features & features) == features
        {
            return Some(format);
        }
    }

    None
}

fn find_depth_format(instance: &ash::Instance, physical_device: vk::PhysicalDevice) -> vk::Format {
    let candidates = [
        vk::Format::D32_SFLOAT,
        vk::Format::D32_SFLOAT_S8_UINT,
        vk::Format::D24_UNORM_S8_UINT,
    ];
    let tiling = vk::ImageTiling::OPTIMAL;
    let features = vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT;

    find_supported_format(instance, physical_device, &candidates, tiling, features)
        .expect("no supported depth format available")
}

fn has_stencil_component(format: vk::Format) -> bool {
    [
        vk::Format::D32_SFLOAT_S8_UINT,
        vk::Format::D24_UNORM_S8_UINT,
    ]
    .contains(&format)
}

fn generate_mipmaps(
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    image: vk::Image,
    tex_extent: (i32, i32),
    mip_levels: u32,
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    format: vk::Format,
) -> Result<(), anyhow::Error> {
    let format_properties =
        unsafe { instance.get_physical_device_format_properties(physical_device, format) };
    let linear_blit_support = format_properties
        .optimal_tiling_features
        .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR);
    if !linear_blit_support {
        anyhow::bail!("no linear blitting support");
    }

    let command_buffer = begin_single_time_commands(device, command_pool)?;

    // base reused barrier values
    let subresource_range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .base_array_layer(0)
        .layer_count(1)
        .level_count(1);
    let mut barrier = vk::ImageMemoryBarrier2::default()
        .image(image)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(subresource_range);

    // record blit commands
    let mut mip_width = tex_extent.0;
    let mut mip_height = tex_extent.1;
    for i in 1..mip_levels {
        barrier.subresource_range.base_mip_level = i - 1;
        barrier.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
        barrier.new_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
        // level 0 is written by a copy, levels above by the previous blit
        barrier.src_stage_mask = vk::PipelineStageFlags2::ALL_TRANSFER;
        barrier.src_access_mask = vk::AccessFlags2::TRANSFER_WRITE;
        barrier.dst_stage_mask = vk::PipelineStageFlags2::BLIT;
        barrier.dst_access_mask = vk::AccessFlags2::TRANSFER_READ;

        cmd_barrier2(device, command_buffer, &[barrier]);

        let src_subresource = vk::ImageSubresourceLayers::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(i - 1)
            .base_array_layer(0)
            .layer_count(1);
        let dst_subresource = vk::ImageSubresourceLayers::default()
            .aspect_mask(vk::ImageAspectFlags::COLOR)
            .mip_level(i)
            .base_array_layer(0)
            .layer_count(1);
        let blit = vk::ImageBlit::default()
            .src_offsets([
                vk::Offset3D::default(),
                vk::Offset3D::default().x(mip_width).y(mip_height).z(1),
            ])
            .src_subresource(src_subresource)
            .dst_offsets([
                vk::Offset3D::default(),
                vk::Offset3D::default()
                    .x(if mip_width > 1 { mip_width / 2 } else { 1 })
                    .y(if mip_height > 1 { mip_height / 2 } else { 1 })
                    .z(1),
            ])
            .dst_subresource(dst_subresource);

        unsafe {
            let regions = [blit];
            device.cmd_blit_image(
                command_buffer,
                image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &regions,
                vk::Filter::LINEAR,
            )
        };

        barrier.old_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
        barrier.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        barrier.src_stage_mask = vk::PipelineStageFlags2::BLIT;
        barrier.src_access_mask = vk::AccessFlags2::TRANSFER_READ;
        barrier.dst_stage_mask = vk::PipelineStageFlags2::FRAGMENT_SHADER;
        barrier.dst_access_mask = vk::AccessFlags2::SHADER_READ;

        cmd_barrier2(device, command_buffer, &[barrier]);

        if mip_width > 1 {
            mip_width /= 2;
        }

        if mip_height > 1 {
            mip_height /= 2;
        }
    }

    barrier.subresource_range.base_mip_level = mip_levels - 1;
    barrier.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
    barrier.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
    barrier.src_stage_mask = vk::PipelineStageFlags2::ALL_TRANSFER;
    barrier.src_access_mask = vk::AccessFlags2::TRANSFER_WRITE;
    barrier.dst_stage_mask = vk::PipelineStageFlags2::FRAGMENT_SHADER;
    barrier.dst_access_mask = vk::AccessFlags2::SHADER_READ;

    cmd_barrier2(device, command_buffer, &[barrier]);

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

fn get_max_usable_sample_count(
    physical_device_properties: vk::PhysicalDeviceProperties,
    max_msaa_samples: MaxMSAASamples,
) -> vk::SampleCountFlags {
    let vk::PhysicalDeviceLimits {
        framebuffer_color_sample_counts,
        framebuffer_depth_sample_counts,
        ..
    } = physical_device_properties.limits;
    let counts = framebuffer_color_sample_counts & framebuffer_depth_sample_counts;

    let descending_options: &[vk::SampleCountFlags] = match max_msaa_samples {
        MaxMSAASamples::Max8 => &[
            vk::SampleCountFlags::TYPE_8,
            vk::SampleCountFlags::TYPE_4,
            vk::SampleCountFlags::TYPE_2,
        ],
        MaxMSAASamples::Max4 => &[vk::SampleCountFlags::TYPE_4, vk::SampleCountFlags::TYPE_2],
        MaxMSAASamples::Max2 => &[vk::SampleCountFlags::TYPE_2],
    };

    for option in descending_options {
        if counts.contains(*option) {
            return *option;
        }
    }

    // NOTE this will trigger a validation error;
    // supposed to not use resolve attachment setup at all if not using msaa
    vk::SampleCountFlags::TYPE_1
}

fn create_color_image(
    allocator: &vk_mem::Allocator,
    device: &ash::Device,
    swapchain_extent: vk::Extent2D,
    color_format: vk::Format,
    msaa_samples: vk::SampleCountFlags,
) -> Result<(vk::Image, vk_mem::Allocation, vk::ImageView), anyhow::Error> {
    let mip_levels = 1;
    let image_options = ImageOptions {
        extent: swapchain_extent,
        format: color_format,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::TRANSIENT_ATTACHMENT | vk::ImageUsageFlags::COLOR_ATTACHMENT,
        mip_levels,
        msaa_samples,
    };

    let (color_image, color_image_memory) = create_vk_image(allocator, image_options)?;

    let color_image_view = create_image_view(
        device,
        color_image,
        color_format,
        vk::ImageAspectFlags::COLOR,
        mip_levels,
    )?;

    Ok((color_image, color_image_memory, color_image_view))
}

struct ShaderPipelineLayout {
    vertex_shader: PrecompiledShader,
    fragment_shader: PrecompiledShader,

    // NOTE the renderer is expected to clean up these fields correctly
    // they need special handling during hot reload
    pipeline_layout: ash::vk::PipelineLayout,
    descriptor_set_layouts: Vec<(ash::vk::DescriptorSetLayout, DescriptorCounts)>,
}

impl ShaderPipelineLayout {
    #[cfg(debug_assertions)]
    fn create_from_atlas(
        device: &ash::Device,
        shader: &dyn ShaderAtlasEntry,
    ) -> Result<Self, anyhow::Error> {
        let shaders::ReflectedShader {
            vertex_shader,
            fragment_shader,
            reflection_json,
        } = shaders::dev_compile_slang_shaders(shader.source_file_name())?;

        let vertex_shader = PrecompiledShader {
            spv_bytes: vertex_shader.spv_bytes()?,
            entry_point_name: vertex_shader.entry_point_name,
        };

        let fragment_shader = PrecompiledShader {
            spv_bytes: fragment_shader.spv_bytes()?,
            entry_point_name: fragment_shader.entry_point_name,
        };

        let (pipeline_layout, descriptor_set_layouts) =
            unsafe { reflection_json.pipeline_layout.vk_create(device)? };

        Ok(ShaderPipelineLayout {
            vertex_shader,
            fragment_shader,
            pipeline_layout,
            descriptor_set_layouts,
        })
    }

    #[cfg(not(debug_assertions))]
    fn create_from_atlas(
        device: &ash::Device,
        shader: &dyn ShaderAtlasEntry,
    ) -> Result<Self, anyhow::Error> {
        let precompiled = shader.precompiled_shaders();

        let (pipeline_layout, descriptor_set_layouts) =
            unsafe { shader.pipeline_layout().vk_create(device)? };

        Ok(ShaderPipelineLayout {
            vertex_shader: precompiled.vert,
            fragment_shader: precompiled.frag,
            pipeline_layout,
            descriptor_set_layouts,
        })
    }
}

pub(crate) struct ComputeShaderPipelineLayout {
    compute_shader: PrecompiledShader,

    // NOTE the renderer is expected to clean up these fields correctly
    // they need special handling during hot reload
    pipeline_layout: ash::vk::PipelineLayout,
    descriptor_set_layouts: Vec<(ash::vk::DescriptorSetLayout, DescriptorCounts)>,
}

impl ComputeShaderPipelineLayout {
    #[cfg(debug_assertions)]
    fn create_from_atlas(
        device: &ash::Device,
        shader: &dyn ComputeShaderAtlasEntry,
    ) -> Result<Self, anyhow::Error> {
        let shaders::ReflectedComputeShader {
            compute_shader,
            reflection_json,
        } = shaders::dev_compile_slang_compute_shaders(shader.source_file_name())?;

        let compute_shader = PrecompiledShader {
            spv_bytes: compute_shader.spv_bytes()?,
            entry_point_name: compute_shader.entry_point_name,
        };

        let (pipeline_layout, descriptor_set_layouts) =
            unsafe { reflection_json.pipeline_layout.vk_create(device)? };

        Ok(ComputeShaderPipelineLayout {
            compute_shader,
            pipeline_layout,
            descriptor_set_layouts,
        })
    }

    #[cfg(not(debug_assertions))]
    fn create_from_atlas(
        device: &ash::Device,
        shader: &dyn ComputeShaderAtlasEntry,
    ) -> Result<Self, anyhow::Error> {
        let precompiled = shader.precompiled_compute_shader();

        let (pipeline_layout, descriptor_set_layouts) =
            unsafe { shader.pipeline_layout().vk_create(device)? };

        Ok(ComputeShaderPipelineLayout {
            compute_shader: precompiled,
            pipeline_layout,
            descriptor_set_layouts,
        })
    }
}

impl shaders::json::ReflectedDescriptorSetLayout {
    unsafe fn vk_create(
        &self,
        device: &ash::Device,
    ) -> Result<vk::DescriptorSetLayout, vk::Result> {
        let binding_ranges: Vec<_> = self.binding_ranges.iter().map(|b| b.to_vk()).collect();
        let create_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&binding_ranges);

        unsafe { device.create_descriptor_set_layout(&create_info, None) }
    }
}

impl shaders::json::ReflectedDescriptorSetLayoutBinding {
    fn to_vk(&self) -> vk::DescriptorSetLayoutBinding<'static> {
        vk::DescriptorSetLayoutBinding::default()
            .stage_flags(self.stage_flags.to_vk())
            .binding(self.binding)
            .descriptor_count(self.descriptor_count)
            .descriptor_type(self.descriptor_type.to_vk())
    }
}

impl shaders::json::ReflectedBindingType {
    fn to_vk(self) -> vk::DescriptorType {
        match self {
            Self::Sampler => vk::DescriptorType::SAMPLER,
            Self::Texture => vk::DescriptorType::SAMPLED_IMAGE,
            Self::ConstantBuffer => vk::DescriptorType::UNIFORM_BUFFER,
            Self::CombinedTextureSampler => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            Self::StorageImage => vk::DescriptorType::STORAGE_IMAGE,
        }
    }
}

impl shaders::json::ReflectedPipelineLayout {
    unsafe fn vk_create(
        &self,
        device: &ash::Device,
    ) -> Result<
        (
            vk::PipelineLayout,
            Vec<(vk::DescriptorSetLayout, DescriptorCounts)>,
        ),
        vk::Result,
    > {
        let mut descriptor_set_layouts = Vec::with_capacity(self.descriptor_set_layouts.len());
        for reflected_set_layout in &self.descriptor_set_layouts {
            let counts = DescriptorCounts::from_descriptor_set_layout(reflected_set_layout);
            let created_set_layout = unsafe { reflected_set_layout.vk_create(device)? };
            descriptor_set_layouts.push((created_set_layout, counts));
        }

        let push_constant_ranges: Vec<_> = self
            .push_constant_ranges
            .iter()
            .map(|r| r.to_vk())
            .collect();

        let set_layouts: Vec<_> = descriptor_set_layouts.iter().map(|t| t.0).collect();
        let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
            .set_layouts(&set_layouts)
            .push_constant_ranges(&push_constant_ranges);

        let pipeline_layout =
            unsafe { device.create_pipeline_layout(&pipeline_layout_info, None)? };

        Ok((pipeline_layout, descriptor_set_layouts))
    }
}

// how many descriptors there are of each kind in a set layout, for creating the pool
#[derive(Debug, Clone, Copy)]
struct DescriptorCounts {
    uniform_buffers: u32,
    combined_texture_samplers: u32,
    sampled_images: u32,
    storage_images: u32,
}

impl std::iter::Sum for DescriptorCounts {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        let mut sum = Self::ZERO;
        for counts in iter {
            sum = sum + counts;
        }

        sum
    }
}

impl std::ops::Add for DescriptorCounts {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            uniform_buffers: self.uniform_buffers + rhs.uniform_buffers,
            combined_texture_samplers: self.combined_texture_samplers
                + rhs.combined_texture_samplers,
            sampled_images: self.sampled_images + rhs.sampled_images,
            storage_images: self.storage_images + rhs.storage_images,
        }
    }
}

impl DescriptorCounts {
    const ZERO: Self = Self {
        uniform_buffers: 0,
        combined_texture_samplers: 0,
        sampled_images: 0,
        storage_images: 0,
    };

    fn from_descriptor_set_layout(set_layout: &ReflectedDescriptorSetLayout) -> Self {
        let mut uniform_buffers = 0;
        let mut combined_texture_samplers = 0;
        let mut sampled_images = 0;
        let mut storage_images = 0;
        for binding in &set_layout.binding_ranges {
            match binding.descriptor_type {
                shaders::json::ReflectedBindingType::ConstantBuffer => {
                    uniform_buffers += 1;
                }
                shaders::json::ReflectedBindingType::CombinedTextureSampler => {
                    combined_texture_samplers += 1;
                }
                shaders::json::ReflectedBindingType::StorageImage => {
                    storage_images += 1;
                }
                shaders::json::ReflectedBindingType::Sampler => {
                    // Separate sampler — not currently used, counted for pool allocation
                }
                shaders::json::ReflectedBindingType::Texture => {
                    sampled_images += 1;
                }
            }
        }

        Self {
            uniform_buffers,
            combined_texture_samplers,
            sampled_images,
            storage_images,
        }
    }
}

impl shaders::json::ReflectedPushConstantRange {
    fn to_vk(&self) -> vk::PushConstantRange {
        vk::PushConstantRange::default()
            .stage_flags(self.stage_flags.to_vk())
            .offset(self.offset)
            .size(self.size)
    }
}

impl shaders::json::ReflectedStageFlags {
    fn to_vk(self) -> vk::ShaderStageFlags {
        match self {
            Self::Vertex => vk::ShaderStageFlags::VERTEX,
            Self::Fragment => vk::ShaderStageFlags::FRAGMENT,
            Self::Compute => vk::ShaderStageFlags::COMPUTE,
            Self::All => vk::ShaderStageFlags::ALL,
            Self::Empty => vk::ShaderStageFlags::empty(),
        }
    }
}

/// the interface a game uses to update gpu resources during a renderer draw call
pub struct Gpu<'f> {
    ring_slot: usize,
    uniform_buffers: &'f mut UniformBufferStorage,
    storage_buffers: &'f mut StorageBufferStorage,
}

impl<'f> Gpu<'f> {
    pub fn write_uniform<T>(&mut self, uniform_buffer: &mut UniformBufferHandle<T>, data: T) {
        let mapped_mem = self
            .uniform_buffers
            .get_mapped_mem_for_frame(uniform_buffer, self.ring_slot);

        *mapped_mem = data;
    }

    pub fn write_storage<T>(&mut self, storage_buffer: &mut StorageBufferHandle<T>, data: &[T]) {
        debug_assert!(data.len() <= storage_buffer.len() as usize);
        let len_to_copy = data.len().min(storage_buffer.len() as usize);

        let mapped_mem = self
            .storage_buffers
            .get_mapped_mem_for_frame(storage_buffer, self.ring_slot);

        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), mapped_mem, len_to_copy);
        }
    }

    pub fn sort_storage_by<T, F>(&mut self, storage_buffer: &mut StorageBufferHandle<T>, compare: F)
    where
        F: FnMut(&T, &T) -> Ordering,
    {
        let mapped_mem = self
            .storage_buffers
            .get_mapped_mem_for_frame(storage_buffer, self.ring_slot);

        let len = storage_buffer.len() as usize;
        let items = unsafe { std::slice::from_raw_parts_mut(mapped_mem, len) };

        items.sort_by(compare);
    }

    pub fn write_immutable<T>(
        &mut self,
        immutable_buffer: &mut ImmutableBufferHandle<T>,
        data: &[T],
    ) {
        debug_assert!(data.len() <= immutable_buffer.len() as usize);
        let len_to_copy = data.len().min(immutable_buffer.len() as usize);

        let mapped_mem = self
            .storage_buffers
            .get_mapped_mem_for_frame_immutable(immutable_buffer, self.ring_slot);

        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), mapped_mem, len_to_copy);
        }
    }

    /// A pointer to the current frame's buffer
    pub fn addr<T>(&self, storage_buffer: &StorageBufferHandle<T>) -> Addr<T> {
        Addr::from_raw(
            self.storage_buffers
                .get_device_address_for_frame(storage_buffer, self.ring_slot),
        )
    }

    /// A pointer to the current frame's ping-pong buffer
    ///
    /// We distinguish between current and previous only for gpu-only buffers,
    /// as these are the only ones that can read the previous frame's output.
    pub fn current_addr<T>(&self, gpu_only_buffer: &GpuOnlyBufferHandle<T>) -> Addr<T> {
        Addr::from_raw(
            self.storage_buffers
                .get_device_address_for_frame_gpu_only(gpu_only_buffer, self.ring_slot),
        )
    }

    /// A pointer to the previous frame's ping-pong buffer
    ///
    /// We distinguish between current and previous only for gpu-only buffers,
    /// as these are the only ones that can read the previous frame's output.
    pub fn previous_addr<T>(&self, gpu_only_buffer: &GpuOnlyBufferHandle<T>) -> ReadAddr<T> {
        let prev_frame = (self.ring_slot + PRE_WAIT_RING_LEN - 1) % PRE_WAIT_RING_LEN;
        ReadAddr::from_raw(
            self.storage_buffers
                .get_device_address_for_frame_gpu_only(gpu_only_buffer, prev_frame),
        )
    }

    pub fn current_immutable_addr<T>(
        &self,
        immutable_buffer: &ImmutableBufferHandle<T>,
    ) -> ImmutableAddr<T> {
        ImmutableAddr::from_raw(
            self.storage_buffers
                .get_device_address_for_frame_immutable(immutable_buffer, self.ring_slot),
        )
    }
}

#[derive(PartialEq, Eq)]
enum ComputePlacement {
    /// Compute before graphics in the same command buffer
    BeforeGraphics,
    /// Compute in a separate command buffer (pipelined multi-queue)
    SeparateCommandBuffer,
}

enum PendingComputeCommand {
    Dispatch {
        pipeline_index: usize,
        group_count: [u32; 3],
    },
    Barrier {
        src_stage: vk::PipelineStageFlags2,
        dst_stage: vk::PipelineStageFlags2,
        src_access: vk::AccessFlags2,
        dst_access: vk::AccessFlags2,
    },
}

/// a one-time-use reference to the renderer,
/// for making a frame's single draw call
pub struct FrameRenderer<'f> {
    renderer: &'f mut Renderer,
    pending_compute: Vec<PendingComputeCommand>,
}

#[derive(thiserror::Error, Debug)]
pub enum DrawError {
    #[error("error drawing frame: {0}")]
    DrawError(#[from] anyhow::Error),
}

impl<'f> FrameRenderer<'f> {
    pub(super) fn new(renderer: &'f mut Renderer) -> Self {
        Self {
            renderer,
            pending_compute: vec![],
        }
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.renderer.aspect_ratio
    }

    pub fn window_resolution(&self) -> Vec2 {
        Vec2::new(self.renderer.width, self.renderer.height)
    }

    /// Returns the internal render resolution (may be lower than display with render scaling)
    pub fn render_resolution(&self) -> Vec2 {
        Vec2::new(
            self.renderer.render_extent.width as f32,
            self.renderer.render_extent.height as f32,
        )
    }

    /// Returns the current render scale (0.25 to 1.0)
    pub fn render_scale(&self) -> f32 {
        self.renderer.render_scale
    }

    pub fn dispatch(&mut self, pipeline: &PipelineHandle<Compute>, x: u32, y: u32, z: u32) {
        self.pending_compute.push(PendingComputeCommand::Dispatch {
            pipeline_index: pipeline.index(),
            group_count: [x, y, z],
        });
    }

    pub fn memory_barrier(
        &mut self,
        src_stage: vk::PipelineStageFlags2,
        dst_stage: vk::PipelineStageFlags2,
        src_access: vk::AccessFlags2,
        dst_access: vk::AccessFlags2,
    ) {
        self.pending_compute.push(PendingComputeCommand::Barrier {
            src_stage,
            dst_stage,
            src_access,
            dst_access,
        });
    }

    pub fn draw_indexed(
        self,
        pipeline_handle: &PipelineHandle<DrawIndexed>,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        let index_count = match &self
            .renderer
            .renderer_pipeline(pipeline_handle)
            .vertex_pipeline_config
        {
            VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs) => vi_bufs.index_count,
            _ => panic!("unexpected draw_indexed call for non-index pipeline"),
        };

        let draw_call = DrawCallConfig::IndexCount(index_count);

        self.draw_frame(pipeline_handle, draw_call, None, gpu_update)
    }

    pub fn draw_vertex_count(
        self,
        pipeline_handle: &PipelineHandle<DrawVertexCount>,
        vertex_count: u32,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        let draw_call = DrawCallConfig::VertexCount(vertex_count);
        self.draw_frame(pipeline_handle, draw_call, None, gpu_update)
    }

    pub fn draw_vertex_count_with_picking(
        self,
        main_pipeline: &PipelineHandle<DrawVertexCount>,
        vertex_count: u32,
        picking_pipeline: &PickingPipelineHandle,
        mouse_position: [f32; 2],
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        let render_scale = self.renderer.render_scale;
        let mouse_pixel = [
            (mouse_position[0] * render_scale) as u32,
            (mouse_position[1] * render_scale) as u32,
        ];
        let picking_config = PickingDrawConfig {
            picking_handle: PickingPipelineHandle {
                index: picking_pipeline.index,
            },
            mouse_pixel,
        };
        let draw_call = DrawCallConfig::VertexCount(vertex_count);
        self.draw_frame(main_pipeline, draw_call, Some(picking_config), gpu_update)
    }

    pub fn picked_object_id(&self) -> u32 {
        self.renderer.last_picked_object_id
    }

    fn draw_frame<D>(
        self,
        pipeline_handle: &PipelineHandle<D>,
        draw_call: DrawCallConfig,
        picking_config: Option<PickingDrawConfig>,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        self.renderer
            .draw_frame(
                pipeline_handle,
                draw_call,
                picking_config,
                self.pending_compute,
                gpu_update,
            )
            .map_err(DrawError::DrawError)
    }
}

#[derive(Debug, Clone, Copy)]
enum DrawCallConfig {
    VertexCount(u32),
    IndexCount(u32),
}

struct PickingDrawConfig {
    picking_handle: PickingPipelineHandle,
    mouse_pixel: [u32; 2],
}
