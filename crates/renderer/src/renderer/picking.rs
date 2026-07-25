use ash::vk;

use super::{
    BufferMemory, ImageOptions, MAX_FRAMES_IN_FLIGHT, create_image_view, create_memory_buffer,
    create_vk_image,
};

pub(super) const PICKING_FORMAT: vk::Format = vk::Format::R32_UINT;

pub(super) struct PickingResources {
    pub images: [vk::Image; MAX_FRAMES_IN_FLIGHT],
    pub image_memories: [vk_mem::Allocation; MAX_FRAMES_IN_FLIGHT],
    pub image_views: [vk::ImageView; MAX_FRAMES_IN_FLIGHT],
    pub readback_buffers: [vk::Buffer; MAX_FRAMES_IN_FLIGHT],
    pub readback_memories: [vk_mem::Allocation; MAX_FRAMES_IN_FLIGHT],
    pub readback_mapped: [*mut u32; MAX_FRAMES_IN_FLIGHT],
}

impl PickingResources {
    pub fn init(
        allocator: &vk_mem::Allocator,
        device: &ash::Device,
        render_extent: vk::Extent2D,
    ) -> Result<Self, anyhow::Error> {
        let (images, image_memories, image_views) =
            create_picking_images(allocator, device, render_extent)?;
        let (readback_buffers, readback_memories, readback_mapped) =
            create_picking_readback_buffers(allocator)?;

        Ok(Self {
            images,
            image_memories,
            image_views,
            readback_buffers,
            readback_memories,
            readback_mapped,
        })
    }

    pub fn recreate_images(
        &mut self,
        allocator: &vk_mem::Allocator,
        device: &ash::Device,
        render_extent: vk::Extent2D,
    ) -> Result<(), anyhow::Error> {
        unsafe {
            for i in 0..MAX_FRAMES_IN_FLIGHT {
                device.destroy_image_view(self.image_views[i], None);
                allocator.destroy_image(self.images[i], &mut self.image_memories[i]);
            }
        }

        let (images, image_memories, image_views) =
            create_picking_images(allocator, device, render_extent)?;

        self.images = images;
        self.image_memories = image_memories;
        self.image_views = image_views;

        Ok(())
    }

    pub fn destroy(mut self, allocator: &vk_mem::Allocator, device: &ash::Device) {
        unsafe {
            for i in 0..MAX_FRAMES_IN_FLIGHT {
                device.destroy_image_view(self.image_views[i], None);
                allocator.destroy_image(self.images[i], &mut self.image_memories[i]);
                allocator.destroy_buffer(self.readback_buffers[i], &mut self.readback_memories[i]);
            }
        }
    }
}

fn create_picking_images(
    allocator: &vk_mem::Allocator,
    device: &ash::Device,
    render_extent: vk::Extent2D,
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
        format: PICKING_FORMAT,
        tiling: vk::ImageTiling::OPTIMAL,
        usage: vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
        mip_levels: 1,
        msaa_samples: vk::SampleCountFlags::TYPE_1,
    };

    let results: [_; MAX_FRAMES_IN_FLIGHT] = (0..MAX_FRAMES_IN_FLIGHT)
        .map(|_| -> anyhow::Result<_> {
            let (image, memory) = create_vk_image(allocator, image_options)?;
            let view = create_image_view(
                device,
                image,
                PICKING_FORMAT,
                vk::ImageAspectFlags::COLOR,
                1,
            )?;
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

fn create_picking_readback_buffers(
    allocator: &vk_mem::Allocator,
) -> Result<
    (
        [vk::Buffer; MAX_FRAMES_IN_FLIGHT],
        [vk_mem::Allocation; MAX_FRAMES_IN_FLIGHT],
        [*mut u32; MAX_FRAMES_IN_FLIGHT],
    ),
    anyhow::Error,
> {
    let results: [_; MAX_FRAMES_IN_FLIGHT] = (0..MAX_FRAMES_IN_FLIGHT)
        .map(|_| -> anyhow::Result<_> {
            let (buffer, memory) = create_memory_buffer(
                allocator,
                4, // sizeof(u32)
                vk::BufferUsageFlags::TRANSFER_DST,
                BufferMemory::Readback,
            )?;

            let ptr = allocator.get_allocation_info(&memory).mapped_data;

            Ok((buffer, memory, ptr as *mut u32))
        })
        .collect::<Result<Vec<_>, _>>()?
        .try_into()
        .unwrap();

    let buffers = results.map(|(buffer, _, _)| buffer);
    let memories = results.map(|(_, memory, _)| memory);
    let mapped = results.map(|(_, _, ptr)| ptr);

    Ok((buffers, memories, mapped))
}
