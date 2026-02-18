use ash::vk;

use super::{
    ImageOptions, MAX_FRAMES_IN_FLIGHT, create_image_view, create_memory_buffer, create_vk_image,
};

pub(super) const PICKING_FORMAT: vk::Format = vk::Format::R32_UINT;

pub(super) struct PickingResources {
    pub render_pass: vk::RenderPass,
    pub images: [vk::Image; MAX_FRAMES_IN_FLIGHT],
    pub image_memories: [vk::DeviceMemory; MAX_FRAMES_IN_FLIGHT],
    pub image_views: [vk::ImageView; MAX_FRAMES_IN_FLIGHT],
    pub framebuffers: [vk::Framebuffer; MAX_FRAMES_IN_FLIGHT],
    pub readback_buffers: [vk::Buffer; MAX_FRAMES_IN_FLIGHT],
    pub readback_memories: [vk::DeviceMemory; MAX_FRAMES_IN_FLIGHT],
    pub readback_mapped: [*mut u32; MAX_FRAMES_IN_FLIGHT],
}

pub(super) fn create_picking_render_pass(
    device: &ash::Device,
) -> Result<vk::RenderPass, anyhow::Error> {
    let color_attachment = vk::AttachmentDescription::default()
        .format(PICKING_FORMAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL);

    let color_attachment_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let color_attachment_refs = [color_attachment_ref];
    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachment_refs);

    let subpass_dep = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);

    let attachments = [color_attachment];
    let subpasses = [subpass];
    let dependencies = [subpass_dep];
    let render_pass_create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);

    let render_pass = unsafe { device.create_render_pass(&render_pass_create_info, None)? };

    Ok(render_pass)
}

pub(super) fn create_picking_images(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
    render_extent: vk::Extent2D,
) -> Result<
    (
        [vk::Image; MAX_FRAMES_IN_FLIGHT],
        [vk::DeviceMemory; MAX_FRAMES_IN_FLIGHT],
        [vk::ImageView; MAX_FRAMES_IN_FLIGHT],
    ),
    anyhow::Error,
> {
    let image_options = ImageOptions {
        extent: render_extent,
        format: PICKING_FORMAT,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        memory_properties: vk::MemoryPropertyFlags::DEVICE_LOCAL,
        mip_levels: 1,
        msaa_samples: vk::SampleCountFlags::TYPE_1,
    };

    let mut images = [vk::Image::null(); MAX_FRAMES_IN_FLIGHT];
    let mut memories = [vk::DeviceMemory::null(); MAX_FRAMES_IN_FLIGHT];
    let mut views = [vk::ImageView::null(); MAX_FRAMES_IN_FLIGHT];

    for i in 0..MAX_FRAMES_IN_FLIGHT {
        let (image, memory) = create_vk_image(instance, device, physical_device, image_options)?;
        let view = create_image_view(
            device,
            image,
            PICKING_FORMAT,
            vk::ImageAspectFlags::COLOR,
            1,
        )?;
        images[i] = image;
        memories[i] = memory;
        views[i] = view;
    }

    Ok((images, memories, views))
}

pub(super) fn create_picking_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    image_views: &[vk::ImageView; MAX_FRAMES_IN_FLIGHT],
    render_extent: vk::Extent2D,
) -> Result<[vk::Framebuffer; MAX_FRAMES_IN_FLIGHT], anyhow::Error> {
    let mut framebuffers = [vk::Framebuffer::null(); MAX_FRAMES_IN_FLIGHT];

    for i in 0..MAX_FRAMES_IN_FLIGHT {
        let attachments = [image_views[i]];

        let framebuffer_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(render_extent.width)
            .height(render_extent.height)
            .layers(1);

        framebuffers[i] = unsafe { device.create_framebuffer(&framebuffer_info, None)? };
    }

    Ok(framebuffers)
}

pub(super) fn create_picking_readback_buffers(
    instance: &ash::Instance,
    device: &ash::Device,
    physical_device: vk::PhysicalDevice,
) -> Result<
    (
        [vk::Buffer; MAX_FRAMES_IN_FLIGHT],
        [vk::DeviceMemory; MAX_FRAMES_IN_FLIGHT],
        [*mut u32; MAX_FRAMES_IN_FLIGHT],
    ),
    anyhow::Error,
> {
    let mut buffers = [vk::Buffer::null(); MAX_FRAMES_IN_FLIGHT];
    let mut memories = [vk::DeviceMemory::null(); MAX_FRAMES_IN_FLIGHT];
    let mut mapped = [std::ptr::null_mut::<u32>(); MAX_FRAMES_IN_FLIGHT];

    for i in 0..MAX_FRAMES_IN_FLIGHT {
        let (buffer, memory) = create_memory_buffer(
            instance,
            device,
            physical_device,
            4, // sizeof(u32)
            vk::BufferUsageFlags::TRANSFER_DST,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        let ptr = unsafe { device.map_memory(memory, 0, 4, Default::default())? };

        buffers[i] = buffer;
        memories[i] = memory;
        mapped[i] = ptr as *mut u32;
    }

    Ok((buffers, memories, mapped))
}

impl PickingResources {
    pub fn init(
        instance: &ash::Instance,
        device: &ash::Device,
        physical_device: vk::PhysicalDevice,
        render_extent: vk::Extent2D,
    ) -> Result<Self, anyhow::Error> {
        let render_pass = create_picking_render_pass(device)?;
        let (images, image_memories, image_views) =
            create_picking_images(instance, device, physical_device, render_extent)?;
        let framebuffers =
            create_picking_framebuffers(device, render_pass, &image_views, render_extent)?;
        let (readback_buffers, readback_memories, readback_mapped) =
            create_picking_readback_buffers(instance, device, physical_device)?;

        Ok(Self {
            render_pass,
            images,
            image_memories,
            image_views,
            framebuffers,
            readback_buffers,
            readback_memories,
            readback_mapped,
        })
    }

    pub fn recreate_images(
        &mut self,
        instance: &ash::Instance,
        device: &ash::Device,
        physical_device: vk::PhysicalDevice,
        render_extent: vk::Extent2D,
    ) -> Result<(), anyhow::Error> {
        unsafe {
            for i in 0..MAX_FRAMES_IN_FLIGHT {
                device.destroy_framebuffer(self.framebuffers[i], None);
                device.destroy_image_view(self.image_views[i], None);
                device.destroy_image(self.images[i], None);
                device.free_memory(self.image_memories[i], None);
            }
        }

        let (images, image_memories, image_views) =
            create_picking_images(instance, device, physical_device, render_extent)?;
        let framebuffers =
            create_picking_framebuffers(device, self.render_pass, &image_views, render_extent)?;

        self.images = images;
        self.image_memories = image_memories;
        self.image_views = image_views;
        self.framebuffers = framebuffers;

        Ok(())
    }

    pub fn destroy(&self, device: &ash::Device) {
        unsafe {
            for i in 0..MAX_FRAMES_IN_FLIGHT {
                device.destroy_framebuffer(self.framebuffers[i], None);
                device.destroy_image_view(self.image_views[i], None);
                device.destroy_image(self.images[i], None);
                device.free_memory(self.image_memories[i], None);
                device.destroy_buffer(self.readback_buffers[i], None);
                device.free_memory(self.readback_memories[i], None);
            }
            device.destroy_render_pass(self.render_pass, None);
        }
    }
}
