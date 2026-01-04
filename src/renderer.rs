#![allow(clippy::type_complexity, clippy::too_many_arguments)]

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString, c_char};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use ash::vk;
use sdl3::sys::vulkan::SDL_Vulkan_DestroySurface;
use sdl3::video::Window;

use crate::shaders;
use crate::shaders::atlas::{PrecompiledShader, ShaderAtlasEntry};

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

pub mod pipeline;
pub use pipeline::*;

/// enables both the validation layer and debug utils logging
const ENABLE_VALIDATION: bool = cfg!(debug_assertions);
/// applies MSAA-like sampling within textures
const ENABLE_SAMPLE_SHADING: bool = false;

const MAX_FRAMES_IN_FLIGHT: usize = 2;

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
    render_pass: vk::RenderPass,
    swapchain_framebuffers: Vec<vk::Framebuffer>,
    color_image: vk::Image,
    color_image_memory: vk::DeviceMemory,
    color_image_view: vk::ImageView,
    depth_image: vk::Image,
    depth_image_memory: vk::DeviceMemory,
    depth_image_view: vk::ImageView,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    /// image semaphores indexed by current_frame
    image_available: Vec<vk::Semaphore>,
    /// render finished semaphores indexed by image_index
    render_finished: Vec<vk::Semaphore>,
    /// frame fences indexed by current frame
    frames_in_flight: Vec<vk::Fence>,
    /// looping index
    current_frame: usize,

    pipelines: PipelineStorage,
    textures: TextureStorage,
    uniform_buffers: UniformBufferStorage,
    storage_buffers: StorageBufferStorage,
}

impl Renderer {
    pub fn init(window: Window) -> Result<Self, anyhow::Error> {
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
        if ENABLE_VALIDATION {
            enabled_extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
        }

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

        let msaa_samples = get_max_usable_sample_count(physical_device_properties);

        let graphics_queue = unsafe { device.get_device_queue(queue_family_indices.graphics, 0) };
        let presentation_queue =
            unsafe { device.get_device_queue(queue_family_indices.presentation, 0) };

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
        let (image_available, render_finished, frames_in_flight) =
            create_sync_objects(&device, &swapchain_images)?;

        let render_pass = create_render_pass(
            &instance,
            physical_device,
            &device,
            image_format,
            msaa_samples,
        )?;

        let command_pool = create_command_pool(&device, &queue_family_indices)?;
        let command_buffers = create_command_buffers(&device, command_pool)?;

        let (color_image, color_image_memory, color_image_view) = create_color_image(
            &instance,
            &device,
            physical_device,
            image_extent,
            image_format,
            msaa_samples,
        )?;

        let (depth_image, depth_image_memory, depth_image_view) = create_depth_buffer_image(
            &instance,
            &device,
            physical_device,
            command_pool,
            graphics_queue,
            image_extent,
            msaa_samples,
        )?;

        let swapchain_framebuffers = create_framebuffers(
            &device,
            render_pass,
            &swapchain_image_views,
            image_extent,
            depth_image_view,
            color_image_view,
        )?;

        let pipelines = PipelineStorage::new();
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
            graphics_queue,
            presentation_queue,
            swapchain_device_ext,
            msaa_samples,
            image_format,
            image_extent,
            swapchain,
            swapchain_images,
            swapchain_image_views,
            render_pass,
            swapchain_framebuffers,
            color_image,
            color_image_memory,
            color_image_view,
            depth_image,
            depth_image_memory,
            depth_image_view,
            command_pool,
            command_buffers,
            image_available,
            render_finished,
            frames_in_flight,
            current_frame: 0,

            pipelines,
            textures,
            uniform_buffers,
            storage_buffers,
        })
    }

    fn renderer_pipeline<D>(&self, handle: &PipelineHandle<D>) -> &RendererPipeline {
        self.pipelines.get(handle)
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
            self.device.destroy_image(texture.image, None);
            self.device.free_memory(texture.image_memory, None);
        }
    }

    pub fn create_uniform_buffer<T: GPUWrite>(&mut self) -> anyhow::Result<UniformBufferHandle<T>> {
        let buffer_size = std::mem::size_of::<T>() as u64;

        let mut buffers_per_frame = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            let (buffer, device_mem) = create_memory_buffer(
                &self.instance,
                &self.device,
                self.physical_device,
                buffer_size,
                vk::BufferUsageFlags::UNIFORM_BUFFER,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )?;

            let mapped_mem = unsafe {
                self.device
                    .map_memory(device_mem, 0, buffer_size, Default::default())?
            };

            let raw_uniform_buffer = RawUniformBuffer {
                buffer,
                device_mem,
                mapped_mem,
            };

            buffers_per_frame.push(raw_uniform_buffer);
        }

        let handle = self.uniform_buffers.add(buffers_per_frame);

        Ok(handle)
    }

    pub fn drop_uniform_buffer<T>(&mut self, uniform_buffer: UniformBufferHandle<T>) {
        let buffers_per_frame = self.uniform_buffers.take(uniform_buffer);
        for raw_uniform_buffer in buffers_per_frame {
            self.destroy_uniform_buffer(raw_uniform_buffer);
        }
    }

    fn destroy_uniform_buffer(&mut self, uniform_buffer: RawUniformBuffer) {
        unsafe {
            self.device.destroy_buffer(uniform_buffer.buffer, None);
            self.device.free_memory(uniform_buffer.device_mem, None);
        }
    }

    pub fn create_storage_buffer<T: GPUWrite>(
        &mut self,
        len: u32,
    ) -> anyhow::Result<StorageBufferHandle<T>> {
        let buffer_size = (len as usize * std::mem::size_of::<T>()) as u64;

        let mut buffers_per_frame = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            let (buffer, device_mem) = create_memory_buffer(
                &self.instance,
                &self.device,
                self.physical_device,
                buffer_size,
                vk::BufferUsageFlags::STORAGE_BUFFER,
                vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            )?;

            let mapped_mem = unsafe {
                self.device
                    .map_memory(device_mem, 0, buffer_size, Default::default())?
            };

            let raw_storage_buffer = RawStorageBuffer {
                buffer,
                device_mem,
                mapped_mem,
            };

            buffers_per_frame.push(raw_storage_buffer);
        }

        let handle = self.storage_buffers.add(buffers_per_frame, len);

        Ok(handle)
    }

    pub fn drop_storage_buffer<T>(&mut self, storage_buffer: StorageBufferHandle<T>) {
        let buffers_per_frame = self.storage_buffers.take(storage_buffer);
        for raw_storage_buffer in buffers_per_frame {
            self.destroy_storage_buffer(raw_storage_buffer);
        }
    }

    fn destroy_storage_buffer(&mut self, storage_buffer: RawStorageBuffer) {
        unsafe {
            self.device.destroy_buffer(storage_buffer.buffer, None);
            self.device.free_memory(storage_buffer.device_mem, None);
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

    /// NOTE call this only after draining gpu commands
    pub fn drop_pipeline<D>(&mut self, pipeline_handle: PipelineHandle<D>) {
        let pipeline = self.pipelines.take(pipeline_handle);
        self.destroy_pipeline(pipeline);
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

            match &pipeline.vertex_pipeline_config {
                VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs) => {
                    self.device.destroy_buffer(vi_bufs.index_buffer, None);
                    self.device.free_memory(vi_bufs.index_buffer_memory, None);

                    self.device.destroy_buffer(vi_bufs.vertex_buffer, None);
                    self.device.free_memory(vi_bufs.vertex_buffer_memory, None);
                }

                VertexPipelineConfig::VertexCount => {}
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
            self.render_pass,
            self.msaa_samples,
            &pipeline_layout,
            &config.shader.vertex_binding_descriptions(),
            &config.shader.vertex_attribute_descriptions(),
            !config.disable_depth_test,
        )?;

        let vertex_pipeline_config = match &config.vertex_config {
            VertexConfig::VertexAndIndexBuffers(vertices, indices) => {
                let (vertex_buffer, vertex_buffer_memory) = create_vertex_buffer(
                    &self.instance,
                    &self.device,
                    self.physical_device,
                    self.command_pool,
                    self.graphics_queue,
                    vertices,
                )?;

                let (index_buffer, index_buffer_memory) = create_index_buffer(
                    &self.instance,
                    &self.device,
                    self.physical_device,
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

        let uniform_buffers_in_layout_frame_order: Vec<&[RawUniformBuffer]> = config
            .uniform_buffer_handles
            .iter()
            .map(|raw_handle| self.uniform_buffers.get_raw(raw_handle))
            .collect();

        let storage_buffers_in_layout_frame_order: Vec<&[RawStorageBuffer]> = config
            .storage_buffer_handles
            .iter()
            .map(|raw_handle| self.storage_buffers.get_raw(raw_handle))
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
            &storage_buffers_in_layout_frame_order,
            &textures,
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

    fn record_command_buffer<D>(
        &mut self,
        pipeline_handle: &PipelineHandle<D>,
        image_index: u32,
        draw_call: DrawCallConfig,
    ) -> Result<(), anyhow::Error> {
        let command_buffer = self.command_buffers[self.current_frame];

        let begin_info = vk::CommandBufferBeginInfo::default();
        unsafe {
            self.device
                .begin_command_buffer(command_buffer, &begin_info)?;
        }

        let framebuffer = self.swapchain_framebuffers[image_index as usize];
        let render_area = vk::Rect2D::default()
            .offset(vk::Offset2D::default())
            .extent(self.image_extent);
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
        // NOTE this must match the order of the attachments
        let clear_values = [clear_color, clear_depth_stencil];
        let render_pass_begin = vk::RenderPassBeginInfo::default()
            .render_pass(self.render_pass)
            .framebuffer(framebuffer)
            .render_area(render_area)
            .clear_values(&clear_values);

        // BEGIN RENDER PASS
        unsafe {
            self.device.cmd_begin_render_pass(
                command_buffer,
                &render_pass_begin,
                vk::SubpassContents::INLINE,
            );
        }

        unsafe {
            self.device.cmd_bind_pipeline(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                self.renderer_pipeline(pipeline_handle).pipeline,
            );
        }

        let viewport = vk::Viewport::default()
            .x(0.0)
            .y(0.0)
            .width(self.image_extent.width as f32)
            .height(self.image_extent.height as f32)
            .min_depth(0.0)
            .max_depth(1.0);
        let viewports = [viewport];
        unsafe { self.device.cmd_set_viewport(command_buffer, 0, &viewports) };

        let scissor = vk::Rect2D::default()
            .offset(vk::Offset2D::default())
            .extent(self.image_extent);
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

        // END RENDER PASS
        unsafe { self.device.cmd_end_render_pass(command_buffer) };

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
            .nth(self.current_frame)
            .unwrap()
    }

    fn draw_frame<D>(
        &mut self,
        pipeline_handle: &PipelineHandle<D>,
        draw_call: DrawCallConfig,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), anyhow::Error> {
        self.total_frames += 1;
        #[cfg(debug_assertions)]
        self.check_for_shader_recompile(pipeline_handle)?;

        let command_buffer = self.command_buffers[self.current_frame];

        let fences = [self.frames_in_flight[self.current_frame]];
        unsafe { self.device.wait_for_fences(&fences, true, u64::MAX)? };

        let (image_index, swapchain_was_submoptimal_on_image_acquire) = unsafe {
            match self.swapchain_device_ext.acquire_next_image(
                self.swapchain,
                u64::MAX,
                self.image_available[self.current_frame],
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

        let mut gpu = Gpu {
            current_frame: self.current_frame,
            uniform_buffers: &mut self.uniform_buffers,
            storage_buffers: &mut self.storage_buffers,
        };
        gpu_update(&mut gpu);

        // NOTE only reset fences if we're submitting work
        //   ie, after early returns
        unsafe { self.device.reset_fences(&fences)? };

        unsafe {
            self.device
                .reset_command_buffer(command_buffer, Default::default())?;
        }
        self.record_command_buffer(pipeline_handle, image_index, draw_call)?;

        let wait_semaphores = [self.image_available[self.current_frame]];
        let wait_dst_stage_mask = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal_semaphores = [self.render_finished[image_index as usize]];
        let submit_command_buffers = [command_buffer];
        let submit_info = vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_dst_stage_mask)
            .command_buffers(&submit_command_buffers)
            .signal_semaphores(&signal_semaphores);
        unsafe {
            self.device.queue_submit(
                self.graphics_queue,
                &[submit_info],
                self.frames_in_flight[self.current_frame],
            )?;
        }

        let swapchains = [self.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
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

        self.current_frame = (self.current_frame + 1) % MAX_FRAMES_IN_FLIGHT;

        if swapchain_was_submoptimal_on_image_acquire {
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

        self.cleanup_swapchain();
        unsafe {
            self.device.destroy_image_view(self.depth_image_view, None);
            self.device.destroy_image(self.depth_image, None);
            self.device.free_memory(self.depth_image_memory, None);
        }
        unsafe {
            self.device.destroy_image_view(self.color_image_view, None);
            self.device.destroy_image(self.color_image, None);
            self.device.free_memory(self.color_image_memory, None);
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

        self.swapchain_images =
            unsafe { self.swapchain_device_ext.get_swapchain_images(swapchain)? };

        self.swapchain_image_views =
            create_swapchain_image_views(&self.device, self.image_format, &self.swapchain_images)?;

        let (depth_image, depth_image_memory, depth_image_view) = create_depth_buffer_image(
            &self.instance,
            &self.device,
            self.physical_device,
            self.command_pool,
            self.graphics_queue,
            self.image_extent,
            self.msaa_samples,
        )?;
        self.depth_image = depth_image;
        self.depth_image_memory = depth_image_memory;
        self.depth_image_view = depth_image_view;

        let (color_image, color_image_memory, color_image_view) = create_color_image(
            &self.instance,
            &self.device,
            self.physical_device,
            self.image_extent,
            self.image_format,
            self.msaa_samples,
        )?;
        self.color_image = color_image;
        self.color_image_memory = color_image_memory;
        self.color_image_view = color_image_view;

        self.swapchain_framebuffers = create_framebuffers(
            &self.device,
            // NOTE for some monitor changes,
            // you'd need to recreate the renderpass as well
            self.render_pass,
            &self.swapchain_image_views,
            image_extent,
            self.depth_image_view,
            self.color_image_view,
        )?;

        Ok(())
    }

    fn cleanup_swapchain(&mut self) {
        unsafe {
            for framebuffer in &self.swapchain_framebuffers {
                self.device.destroy_framebuffer(*framebuffer, None);
            }
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
        for i in to_remove {
            self.old_pipelines.swap_remove(i);
        }

        // recompile shaders if necessary
        let edit_events = self.shader_changes.events()?;
        if !edit_events.is_empty() {
            info!("recompiling shaders...");
            self.try_shader_recompile(pipeline_handle, &edit_events)?;
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
            self.render_pass,
            self.msaa_samples,
            &render_pipeline_mut.layout,
            &render_pipeline_mut.shader.vertex_binding_descriptions(),
            &render_pipeline_mut.shader.vertex_attribute_descriptions(),
            !render_pipeline_mut.disable_depth_test,
        )?;

        info!("finished recompiling shaders");

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
}

impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            for fence in &self.frames_in_flight {
                self.device.destroy_fence(*fence, None);
            }
            for semaphore in &self.render_finished {
                self.device.destroy_semaphore(*semaphore, None);
            }
            for semaphore in &self.image_available {
                self.device.destroy_semaphore(*semaphore, None);
            }

            self.device.destroy_command_pool(self.command_pool, None);

            self.device.destroy_image_view(self.depth_image_view, None);
            self.device.destroy_image(self.depth_image, None);
            self.device.free_memory(self.depth_image_memory, None);

            self.device.destroy_image_view(self.color_image_view, None);
            self.device.destroy_image(self.color_image, None);
            self.device.free_memory(self.color_image_memory, None);

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

            self.device.destroy_render_pass(self.render_pass, None);

            self.cleanup_swapchain();

            for texture in self.textures.take_all() {
                self.destroy_texture(texture);
            }
            for pipeline in self.pipelines.take_all() {
                self.destroy_pipeline(pipeline);
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

    if ENABLE_VALIDATION {
        required_extensions.push(ash::ext::debug_utils::NAME);
    }

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
            (Some(graphics), Some(presentation)) => Some(Self {
                graphics,
                presentation,
            }),
            _ => None,
        };

        Ok(indices)
    }
}

const REQUIRED_DEVICE_EXTENSIONS: [&CStr; 2] = [
    // always required
    vk::KHR_SWAPCHAIN_NAME,
    // required by slang's generated spirv after 2025.10
    //   the feature is required by the 2024 roadmap
    //   https://registry.khronos.org/vulkan/specs/latest/html/vkspec.html#profile-features-roadmap-2024
    // it might be more correct to use physical device features 2 for this
    vk::KHR_SHADER_DRAW_PARAMETERS_NAME,
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

        let features = unsafe { instance.get_physical_device_features(physical_device) };
        if features.sampler_anisotropy != vk::TRUE {
            continue;
        }

        let props = unsafe { instance.get_physical_device_properties(physical_device) };

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
        anyhow::bail!("no graphics device availble");
    };

    #[cfg(debug_assertions)]
    {
        let chosen_device_name = device_name_as_string(chosen_device.2);
        log::trace!("using device: {chosen_device_name}");
    }

    Ok(chosen_device)
}

#[cfg(debug_assertions)]
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
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT); // this would be a memory op instead, if post-processing

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
    let queue_priorities = [1.0];
    for index in unique_queue_families {
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(index)
            .queue_priorities(&queue_priorities);

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

    let enabled_extension_names: Vec<_> = REQUIRED_DEVICE_EXTENSIONS
        .iter()
        .map(|cstr| cstr.as_ptr())
        .collect();

    #[cfg(not(debug_assertions))]
    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_features(&features)
        .enabled_extension_names(&enabled_extension_names);

    #[cfg(debug_assertions)]
    let mut create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_features(&features)
        .enabled_extension_names(&enabled_extension_names);
    // features used by shader println
    #[cfg(debug_assertions)]
    let mut timeline_semaphore_features =
        vk::PhysicalDeviceTimelineSemaphoreFeatures::default().timeline_semaphore(true);
    #[cfg(debug_assertions)]
    let mut memory_model_features = vk::PhysicalDeviceVulkanMemoryModelFeatures::default()
        .vulkan_memory_model(true)
        .vulkan_memory_model_device_scope(true);
    #[cfg(debug_assertions)]
    let mut buffer_device_address_features =
        vk::PhysicalDeviceBufferDeviceAddressFeatures::default().buffer_device_address(true);
    #[cfg(debug_assertions)]
    let mut eight_bit_storage_features =
        vk::PhysicalDevice8BitStorageFeatures::default().storage_buffer8_bit_access(true);
    #[cfg(debug_assertions)]
    {
        create_info = create_info
            .push_next(&mut timeline_semaphore_features)
            .push_next(&mut memory_model_features)
            .push_next(&mut buffer_device_address_features)
            .push_next(&mut eight_bit_storage_features);
    }

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

fn create_render_pass(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: &ash::Device,
    swapchain_format: vk::Format,
    msaa_samples: vk::SampleCountFlags,
) -> Result<vk::RenderPass, anyhow::Error> {
    let color_attachment = vk::AttachmentDescription::default()
        .format(swapchain_format)
        .samples(msaa_samples)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let color_attachment_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let depth_format = find_depth_format(instance, physical_device);
    let depth_attachment = vk::AttachmentDescription::default()
        .format(depth_format)
        .samples(msaa_samples)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::DONT_CARE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    let depth_attachment_ref = vk::AttachmentReference::default()
        .attachment(1)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    let color_attachment_resolve = vk::AttachmentDescription::default()
        .format(swapchain_format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::DONT_CARE)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR);

    let color_attachment_resolve_ref = vk::AttachmentReference::default()
        .attachment(2)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let color_attachment_refs = [color_attachment_ref];
    let resolve_attachment_refs = [color_attachment_resolve_ref];
    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        // NOTE the index in this array is the one referred to by
        // 'layout(location = 0) out' in the frag shader
        .color_attachments(&color_attachment_refs)
        .depth_stencil_attachment(&depth_attachment_ref)
        .resolve_attachments(&resolve_attachment_refs);

    // NOTE an alternative to doing this would be to
    // change the wait stages of image_available to include TOP_OF_PIPE
    // https://vulkan-tutorial.com/en/Drawing_a_triangle/Drawing/Rendering_and_presentation
    let subpass_dep = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
        )
        .src_access_mask(
            vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
                | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        )
        .dst_stage_mask(
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
        )
        .dst_access_mask(
            vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        );

    let attachments = [color_attachment, depth_attachment, color_attachment_resolve];
    let subpasses = [subpass];
    let dependencies = [subpass_dep];
    let render_pass_create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    let render_pass = unsafe { device.create_render_pass(&render_pass_create_info, None)? };

    Ok(render_pass)
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
    render_pass: vk::RenderPass,
    msaa_samples: vk::SampleCountFlags,
    pipeline_layout: &ShaderPipelineLayout,
    vertex_binding_descriptions: &[vk::VertexInputBindingDescription],
    vertex_attribute_descriptions: &[vk::VertexInputAttributeDescription],
    depth_test_enable: bool,
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
        .blend_enable(true)
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
        .render_pass(render_pass)
        .subpass(0)
        .depth_stencil_state(&depth_stencil_state);

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

fn create_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    swapchain_image_views: &[vk::ImageView],
    image_extent: vk::Extent2D,
    depth_image_view: vk::ImageView,
    color_image_view: vk::ImageView,
) -> Result<Vec<vk::Framebuffer>, anyhow::Error> {
    let mut framebuffers = Vec::with_capacity(swapchain_image_views.len());

    for &swapchain_image_view in swapchain_image_views {
        let attachments = [color_image_view, depth_image_view, swapchain_image_view];

        let framebuffer_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(image_extent.width)
            .height(image_extent.height)
            .layers(1);

        let framebuffer = unsafe { device.create_framebuffer(&framebuffer_info, None)? };

        framebuffers.push(framebuffer);
    }

    Ok(framebuffers)
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
) -> Result<Vec<vk::CommandBuffer>, anyhow::Error> {
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(MAX_FRAMES_IN_FLIGHT as u32);

    let buffers = unsafe { device.allocate_command_buffers(&alloc_info)? };

    Ok(buffers)
}

fn create_sync_objects(
    device: &ash::Device,
    swapchain_images: &[vk::Image],
) -> Result<(Vec<vk::Semaphore>, Vec<vk::Semaphore>, Vec<vk::Fence>), anyhow::Error> {
    let mut image_available = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    for _frame in 0..MAX_FRAMES_IN_FLIGHT {
        let semaphore = unsafe { device.create_semaphore(&Default::default(), None)? };
        image_available.push(semaphore);
    }

    let mut render_finished = Vec::with_capacity(swapchain_images.len());
    for _image in swapchain_images {
        let semaphore = unsafe { device.create_semaphore(&Default::default(), None)? };
        render_finished.push(semaphore);
    }

    let mut frames_in_flight = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    for _frame in 0..MAX_FRAMES_IN_FLIGHT {
        let fence_create_info =
            vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
        let fence = unsafe { device.create_fence(&fence_create_info, None)? };
        frames_in_flight.push(fence);
    }

    Ok((image_available, render_finished, frames_in_flight))
}

fn create_vertex_buffer<V: GPUWrite>(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    vertices: &[V],
) -> Result<(vk::Buffer, vk::DeviceMemory), anyhow::Error> {
    let buffer_size = std::mem::size_of_val(vertices) as u64;

    let (staging_buffer, staging_buffer_memory) = create_memory_buffer(
        instance,
        device,
        physical_device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;

    unsafe { write_to_gpu_buffer(device, staging_buffer_memory, vertices)? };

    let (vertex_buffer, vertex_buffer_memory) = create_memory_buffer(
        instance,
        device,
        physical_device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::VERTEX_BUFFER,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
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
        device.destroy_buffer(staging_buffer, None);
        device.free_memory(staging_buffer_memory, None);
    }

    Ok((vertex_buffer, vertex_buffer_memory))
}

fn create_index_buffer(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    indices: &[u32],
) -> Result<(vk::Buffer, vk::DeviceMemory), anyhow::Error> {
    let buffer_size = std::mem::size_of_val(indices) as u64;
    let (staging_buffer, staging_buffer_memory) = create_memory_buffer(
        instance,
        device,
        physical_device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;

    unsafe { write_to_gpu_buffer(device, staging_buffer_memory, indices)? };

    let (index_buffer, index_buffer_memory) = create_memory_buffer(
        instance,
        device,
        physical_device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::INDEX_BUFFER,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
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
        device.destroy_buffer(staging_buffer, None);
        device.free_memory(staging_buffer_memory, None);
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

fn create_memory_buffer(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    buffer_size: vk::DeviceSize,
    buffer_usage: vk::BufferUsageFlags,
    memory_property_flags: vk::MemoryPropertyFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), anyhow::Error> {
    let buffer_create_info = vk::BufferCreateInfo::default()
        .size(buffer_size)
        .usage(buffer_usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe { device.create_buffer(&buffer_create_info, None)? };

    let memory_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

    let mem_type_index = find_memory_type_index(
        instance,
        physical_device,
        memory_requirements.memory_type_bits,
        memory_property_flags,
    )?;
    let allocate_info = vk::MemoryAllocateInfo::default()
        .allocation_size(memory_requirements.size)
        .memory_type_index(mem_type_index as u32);
    let buffer_memory = unsafe { device.allocate_memory(&allocate_info, None)? };

    unsafe {
        device.bind_buffer_memory(buffer, buffer_memory, 0)?;
    };

    Ok((buffer, buffer_memory))
}

fn find_memory_type_index(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    memory_type_bits: u32,
    required_properties: vk::MemoryPropertyFlags,
) -> Result<usize, anyhow::Error> {
    let memory_properties =
        unsafe { instance.get_physical_device_memory_properties(physical_device) };

    for (i, mem_type) in memory_properties.memory_types.iter().enumerate() {
        let matches_type_filter = (memory_type_bits & (1 << i)) != 0;
        let has_required_properties =
            (mem_type.property_flags & required_properties) == required_properties;

        if matches_type_filter && has_required_properties {
            return Ok(i);
        }
    }

    Err(anyhow::anyhow!("failed to find suitable memory type"))
}

fn create_descriptor_pool(
    device: &ash::Device,
    pipeline_layout: &ShaderPipelineLayout,
) -> Result<vk::DescriptorPool, anyhow::Error> {
    let descriptor_sets_per_frame = pipeline_layout.descriptor_set_layouts.len() as u32;
    let sets_across_frames = descriptor_sets_per_frame * MAX_FRAMES_IN_FLIGHT as u32;

    let total_counts: DescriptorCounts = pipeline_layout
        .descriptor_set_layouts
        .iter()
        .map(|tup| tup.1)
        .sum();

    let uniform_buffer_pool_size = vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::UNIFORM_BUFFER)
        .descriptor_count(sets_across_frames * total_counts.uniform_buffers);
    let storage_buffer_pool_size = vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(sets_across_frames * total_counts.storage_buffers);
    let sampler_pool_size = vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
        .descriptor_count(sets_across_frames * total_counts.combined_texture_samplers);

    let pool_sizes = [
        uniform_buffer_pool_size,
        storage_buffer_pool_size,
        sampler_pool_size,
    ];
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
    Storage(StorageBufferDescription),
}

#[derive(Debug)]
pub struct UniformBufferDescription {
    pub size: u64,
    pub binding: u32,
    // the number of descriptors in the descriptor set
    pub descriptor_count: u32,
}

#[derive(Debug)]
pub struct StorageBufferDescription {
    pub size: u64,
    pub binding: u32,
    // the number of descriptors in the descriptor set
    pub descriptor_count: u32,
}

#[derive(Debug)]
pub struct TextureDescription {
    pub layout: vk::ImageLayout,
    pub binding: u32,
    // the number of descriptors in the descriptor set
    pub descriptor_count: u32,
}

fn create_descriptor_sets(
    device: &ash::Device,
    descriptor_pool: vk::DescriptorPool,
    descriptor_set_layouts: &[vk::DescriptorSetLayout],
    uniform_buffers_in_layout_frame_order: &[&[RawUniformBuffer]],
    storage_buffers_in_layout_frame_order: &[&[RawStorageBuffer]],
    textures: &[&Texture],
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
    for _frame in 0..MAX_FRAMES_IN_FLIGHT {
        for &descriptor_set_layout in descriptor_set_layouts {
            // i = frame * descriptor_set_layouts.len() + layout_offset;
            set_layouts.push(descriptor_set_layout);
        }
    }
    let alloc_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&set_layouts);
    let descriptor_sets = unsafe { device.allocate_descriptor_sets(&alloc_info)? };

    for frame in 0..MAX_FRAMES_IN_FLIGHT {
        let mut uniform_buffer_index = 0;
        let mut storage_buffer_index = 0;
        let mut texture_index = 0;

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

                    LayoutDescription::Storage(storage_buffer_description) => {
                        let raw_storage_buffers_by_frame =
                            storage_buffers_in_layout_frame_order[storage_buffer_index];
                        let storage_buffer: vk::Buffer = raw_storage_buffers_by_frame[frame].buffer;

                        // "If range is not equal to VK_WHOLE_SIZE, range must be less than or equal to the size of buffer minus offset"
                        // https://docs.vulkan.org/refpages/latest/refpages/source/VkDescriptorBufferInfo.html
                        let mem_reqs =
                            unsafe { device.get_buffer_memory_requirements(storage_buffer) };
                        let buffer_info = vk::DescriptorBufferInfo::default()
                            .offset(0)
                            .buffer(storage_buffer)
                            .range(mem_reqs.size);
                        let buffer_info = [buffer_info];
                        let storage_buffer_write = vk::WriteDescriptorSet::default()
                            .dst_set(dst_set)
                            .dst_binding(storage_buffer_description.binding)
                            .dst_array_element(0)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .descriptor_count(storage_buffer_description.descriptor_count)
                            .buffer_info(&buffer_info);

                        let writes = [storage_buffer_write];
                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                        storage_buffer_index += 1;
                    }

                    LayoutDescription::Texture(texture_description) => {
                        let texture = textures[texture_index];

                        let image_info = vk::DescriptorImageInfo::default()
                            .image_layout(texture_description.layout)
                            .image_view(texture.image_view)
                            .sampler(texture.sampler);
                        let image_info = [image_info];
                        let image_write = vk::WriteDescriptorSet::default()
                            .dst_set(dst_set)
                            .dst_binding(texture_description.binding)
                            .dst_array_element(0)
                            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                            .descriptor_count(texture_description.descriptor_count)
                            .image_info(&image_info);

                        let writes = [image_write];
                        unsafe { device.update_descriptor_sets(&writes, &[]) };
                        texture_index += 1;
                    }
                }
            }
        }
    }

    Ok(descriptor_sets)
}

const TEXTURE_IMAGE_FORMAT: ash::vk::Format = ash::vk::Format::R8G8B8A8_SRGB;

fn create_texture(
    source_file_name: String,
    input_image: &image::DynamicImage,
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
        image_memory: texture_image_memory,
        mip_levels,
        image_view: texture_image_view,
        sampler: texture_sampler,
    })
}

fn create_texture_image(
    image: &image::DynamicImage,
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
) -> Result<(vk::Image, vk::DeviceMemory, u32), anyhow::Error> {
    let bytes = image.to_rgba8().into_raw();
    debug_assert!(
        bytes.len() == (image.width() * image.height() * 4) as usize,
        "expected rgba bytes size"
    );

    let mip_levels = image.width().max(image.height()).ilog2() + 1;

    let buffer_size = bytes.len() as u64;
    let (staging_buffer, staging_buffer_memory) = create_memory_buffer(
        instance,
        device,
        physical_device,
        buffer_size,
        vk::BufferUsageFlags::TRANSFER_SRC,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;

    unsafe { write_to_gpu_buffer(device, staging_buffer_memory, &bytes)? };

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
        memory_properties: vk::MemoryPropertyFlags::DEVICE_LOCAL,
        mip_levels,
        msaa_samples: vk::SampleCountFlags::TYPE_1,
    };
    let (vk_image, image_memory) =
        create_vk_image(instance, device, physical_device, image_options)?;

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

    copy_buffer_to_image(
        device,
        command_pool,
        graphics_queue,
        staging_buffer,
        vk_image,
        extent,
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
        device.destroy_buffer(staging_buffer, None);
        device.free_memory(staging_buffer_memory, None);
    }

    Ok((vk_image, image_memory, mip_levels))
}

struct ImageOptions {
    extent: vk::Extent2D,
    format: vk::Format,
    tiling: vk::ImageTiling,
    usage: vk::ImageUsageFlags,
    memory_properties: vk::MemoryPropertyFlags,
    mip_levels: u32,
    msaa_samples: vk::SampleCountFlags,
}

fn create_vk_image(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    options: ImageOptions,
) -> Result<(vk::Image, vk::DeviceMemory), anyhow::Error> {
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

    let vk_image = unsafe { device.create_image(&image_create_info, None)? };

    let memory_requirements = unsafe { device.get_image_memory_requirements(vk_image) };
    let memory_type_index = find_memory_type_index(
        instance,
        physical_device,
        memory_requirements.memory_type_bits,
        options.memory_properties,
    )?;
    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(memory_requirements.size)
        .memory_type_index(memory_type_index as u32);
    let image_memory = unsafe { device.allocate_memory(&alloc_info, None)? };
    unsafe { device.bind_image_memory(vk_image, image_memory, 0)? };

    Ok((vk_image, image_memory))
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

    let submit_info = vk::SubmitInfo::default().command_buffers(&command_buffers);
    let submits = [submit_info];
    unsafe {
        device.queue_submit(graphics_queue, &submits, vk::Fence::null())?;
        device.device_wait_idle()?;
    }
    unsafe { device.free_command_buffers(command_pool, &command_buffers) };

    Ok(())
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
    let mut barrier = vk::ImageMemoryBarrier::default()
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(subresource_range);

    let src_stage_mask: vk::PipelineStageFlags;
    let dst_stage_mask: vk::PipelineStageFlags;

    match (old_layout, new_layout) {
        (vk::ImageLayout::UNDEFINED, vk::ImageLayout::TRANSFER_DST_OPTIMAL) => {
            barrier.src_access_mask = Default::default();
            barrier.dst_access_mask = vk::AccessFlags::TRANSFER_WRITE;

            src_stage_mask = vk::PipelineStageFlags::TOP_OF_PIPE;
            dst_stage_mask = vk::PipelineStageFlags::TRANSFER;
        }

        (vk::ImageLayout::TRANSFER_DST_OPTIMAL, vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL) => {
            barrier.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
            barrier.dst_access_mask = vk::AccessFlags::SHADER_READ;

            src_stage_mask = vk::PipelineStageFlags::TRANSFER;
            dst_stage_mask = vk::PipelineStageFlags::FRAGMENT_SHADER;
        }

        (vk::ImageLayout::UNDEFINED, vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL) => {
            barrier.subresource_range.aspect_mask = vk::ImageAspectFlags::DEPTH;

            if has_stencil_component(format) {
                barrier.subresource_range.aspect_mask |= vk::ImageAspectFlags::STENCIL;
            }

            barrier.src_access_mask = Default::default();
            barrier.dst_access_mask = vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE;

            src_stage_mask = vk::PipelineStageFlags::TOP_OF_PIPE;
            dst_stage_mask = vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS;
        }

        transition => {
            anyhow::bail!("layout transition: {transition:?} not supported");
        }
    }

    // https://docs.vulkan.org/spec/latest/chapters/synchronization.html#synchronization-access-types-supported
    let image_barriers = [barrier];
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            src_stage_mask,
            dst_stage_mask,
            Default::default(),
            &[],
            &[],
            &image_barriers,
        )
    };

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

fn copy_buffer_to_image(
    device: &ash::Device,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    buffer: vk::Buffer,
    image: vk::Image,
    extent: vk::Extent2D,
) -> Result<(), anyhow::Error> {
    let command_buffer = begin_single_time_commands(device, command_pool)?;

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

    unsafe {
        let regions = [region];
        device.cmd_copy_buffer_to_image(
            command_buffer,
            buffer,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &regions,
        )
    };

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

fn create_image_view(
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
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    command_pool: vk::CommandPool,
    graphics_queue: vk::Queue,
    swapchain_extent: vk::Extent2D,
    msaa_samples: vk::SampleCountFlags,
) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView), anyhow::Error> {
    let depth_format = find_depth_format(instance, physical_device);

    let mip_levels = 1;

    let image_options = ImageOptions {
        extent: swapchain_extent,
        format: depth_format,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
        memory_properties: vk::MemoryPropertyFlags::DEVICE_LOCAL,
        mip_levels,
        msaa_samples,
    };

    let (depth_image, depth_image_memory) =
        create_vk_image(instance, device, physical_device, image_options)?;

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
    let mut barrier = vk::ImageMemoryBarrier::default()
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
        barrier.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
        barrier.dst_access_mask = vk::AccessFlags::TRANSFER_READ;

        unsafe {
            let image_memory_barriers = [barrier];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
                Default::default(),
                &[],
                &[],
                &image_memory_barriers,
            )
        };

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
        barrier.src_access_mask = vk::AccessFlags::TRANSFER_READ;
        barrier.dst_access_mask = vk::AccessFlags::SHADER_READ;
        unsafe {
            let image_memory_barriers = &[barrier];
            device.cmd_pipeline_barrier(
                command_buffer,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                Default::default(),
                &[],
                &[],
                image_memory_barriers,
            )
        };

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
    barrier.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
    barrier.dst_access_mask = vk::AccessFlags::SHADER_READ;

    let image_memory_barriers = [barrier];
    unsafe {
        device.cmd_pipeline_barrier(
            command_buffer,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            Default::default(),
            &[],
            &[],
            &image_memory_barriers,
        )
    };

    end_single_time_commands(device, command_pool, graphics_queue, command_buffer)?;

    Ok(())
}

fn get_max_usable_sample_count(
    physical_device_properties: vk::PhysicalDeviceProperties,
) -> vk::SampleCountFlags {
    let vk::PhysicalDeviceLimits {
        framebuffer_color_sample_counts,
        framebuffer_depth_sample_counts,
        ..
    } = physical_device_properties.limits;
    let counts = framebuffer_color_sample_counts & framebuffer_depth_sample_counts;

    // NOTE; it may be better to choose less than the maximum available
    // for performance reasons
    let descending_options = [
        vk::SampleCountFlags::TYPE_64,
        vk::SampleCountFlags::TYPE_32,
        vk::SampleCountFlags::TYPE_16,
        vk::SampleCountFlags::TYPE_8,
        vk::SampleCountFlags::TYPE_4,
        vk::SampleCountFlags::TYPE_2,
    ];

    for option in descending_options {
        if counts.contains(option) {
            return option;
        }
    }

    // NOTE this will trigger a validation error;
    // supposed to not use resolve attachment setup at all if not using msaa
    vk::SampleCountFlags::TYPE_1
}

fn create_color_image(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    swapchain_extent: vk::Extent2D,
    color_format: vk::Format,
    msaa_samples: vk::SampleCountFlags,
) -> Result<(vk::Image, vk::DeviceMemory, vk::ImageView), anyhow::Error> {
    let mip_levels = 1;
    let image_options = ImageOptions {
        extent: swapchain_extent,
        format: color_format,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::TRANSIENT_ATTACHMENT | vk::ImageUsageFlags::COLOR_ATTACHMENT,
        memory_properties: vk::MemoryPropertyFlags::DEVICE_LOCAL,
        mip_levels,
        msaa_samples,
    };

    let (color_image, color_image_memory) =
        create_vk_image(instance, device, physical_device, image_options)?;

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
            Self::StorageBuffer => vk::DescriptorType::STORAGE_BUFFER,
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
    storage_buffers: u32,
    combined_texture_samplers: u32,
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
            storage_buffers: self.storage_buffers + rhs.storage_buffers,
            combined_texture_samplers: self.combined_texture_samplers
                + rhs.combined_texture_samplers,
        }
    }
}

impl DescriptorCounts {
    const ZERO: Self = Self {
        uniform_buffers: 0,
        storage_buffers: 0,
        combined_texture_samplers: 0,
    };

    fn from_descriptor_set_layout(set_layout: &ReflectedDescriptorSetLayout) -> Self {
        let mut uniform_buffers = 0;
        let mut storage_buffers = 0;
        let mut combined_texture_samplers = 0;
        for binding in &set_layout.binding_ranges {
            match binding.descriptor_type {
                shaders::json::ReflectedBindingType::ConstantBuffer => {
                    uniform_buffers += 1;
                }
                shaders::json::ReflectedBindingType::StorageBuffer => {
                    storage_buffers += 1;
                }
                shaders::json::ReflectedBindingType::CombinedTextureSampler => {
                    combined_texture_samplers += 1;
                }
                shaders::json::ReflectedBindingType::Sampler => todo!(),
                shaders::json::ReflectedBindingType::Texture => todo!(),
            }
        }

        Self {
            uniform_buffers,
            storage_buffers,
            combined_texture_samplers,
        }
    }
}

impl shaders::json::ReflectedPushConstantRange {
    fn to_vk(&self) -> vk::PushConstantRange {
        vk::PushConstantRange::default()
            .stage_flags(self.stage_flags.to_vk())
            .offset(self.size)
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
    current_frame: usize,
    uniform_buffers: &'f mut UniformBufferStorage,
    storage_buffers: &'f mut StorageBufferStorage,
}

impl<'f> Gpu<'f> {
    pub fn write_uniform<T>(&mut self, uniform_buffer: &mut UniformBufferHandle<T>, data: T) {
        let mapped_mem = self
            .uniform_buffers
            .get_mapped_mem_for_frame(uniform_buffer, self.current_frame);

        *mapped_mem = data;
    }

    pub fn write_storage<T>(&mut self, storage_buffer: &mut StorageBufferHandle<T>, data: &[T]) {
        debug_assert!(data.len() <= storage_buffer.len() as usize);
        let len_to_copy = data.len().min(storage_buffer.len() as usize);

        let mapped_mem = self
            .storage_buffers
            .get_mapped_mem_for_frame(storage_buffer, self.current_frame);

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
            .get_mapped_mem_for_frame(storage_buffer, self.current_frame);

        let len = storage_buffer.len() as usize;
        let items = unsafe { std::slice::from_raw_parts_mut(mapped_mem, len) };

        items.sort_by(compare);
    }
}

/// a one-time-use reference to the renderer,
/// for making a frame's single draw call
pub struct FrameRenderer<'f>(&'f mut Renderer);

#[derive(thiserror::Error, Debug)]
pub enum DrawError {
    #[error("error drawing frame: {0}")]
    DrawError(#[from] anyhow::Error),
}

impl<'f> FrameRenderer<'f> {
    pub fn new(renderer: &'f mut Renderer) -> Self {
        Self(renderer)
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.0.aspect_ratio
    }

    pub fn window_size(&self) -> (f32, f32) {
        (self.0.width, self.0.height)
    }

    pub fn draw_indexed(
        self,
        pipeline_handle: &PipelineHandle<DrawIndexed>,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        let index_count = match &self
            .0
            .renderer_pipeline(pipeline_handle)
            .vertex_pipeline_config
        {
            VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs) => vi_bufs.index_count,
            _ => panic!("unexpected draw_indexed call for non-index pipeline"),
        };

        let draw_call = DrawCallConfig::IndexCount(index_count);

        self.draw_frame(pipeline_handle, draw_call, gpu_update)
    }

    pub fn draw_vertex_count(
        self,
        pipeline_handle: &PipelineHandle<DrawVertexCount>,
        vertex_count: u32,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        let draw_call = DrawCallConfig::VertexCount(vertex_count);
        self.draw_frame(pipeline_handle, draw_call, gpu_update)
    }

    fn draw_frame<D>(
        self,
        pipeline_handle: &PipelineHandle<D>,
        draw_call: DrawCallConfig,
        gpu_update: impl FnOnce(&mut Gpu),
    ) -> Result<(), DrawError> {
        self.0
            .draw_frame(pipeline_handle, draw_call, gpu_update)
            .map_err(DrawError::DrawError)
    }
}

#[derive(Debug, Clone, Copy)]
enum DrawCallConfig {
    VertexCount(u32),
    IndexCount(u32),
}
