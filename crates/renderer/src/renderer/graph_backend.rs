//! Renderer implementation of the graph's backend contracts.

use mltrs_render_graph::backend::{
    BackendTypes, BindingLookup, BufferAddressKind, IndexedIndirectArgs,
};

use super::pipeline::PipelineIndex;
use super::{DrawIndexedIndirectCommand, MAX_FRAMES_IN_FLIGHT, Renderer};

impl BindingLookup for Renderer {
    fn buffer_address(&self, kind: BufferAddressKind, index: usize) -> u64 {
        match kind {
            BufferAddressKind::Current => self
                .storage_buffers
                .device_address_by_index(index, self.flight_slot),
            BufferAddressKind::Previous => {
                let previous = (self.flight_slot + MAX_FRAMES_IN_FLIGHT - 1) % MAX_FRAMES_IN_FLIGHT;

                self.storage_buffers
                    .device_address_by_index(index, previous)
            }
            BufferAddressKind::Singleton => self.singleton_buffers.device_address_by_index(index),
        }
    }
}

impl mltrs_render_graph::backend::PreparationBackend for Renderer {
    fn max_image_dimension_2d(&self) -> u32 {
        self.physical_device_properties
            .limits
            .max_image_dimension2_d
    }

    fn prepare_image(
        &mut self,
        width: u32,
        height: u32,
        format: mltrs_render_graph::backend::GraphFormat,
    ) -> anyhow::Result<(mltrs_render_graph::backend::PhysicalImage, Self::Resource)> {
        use super::ToVk;
        let storage = self.create_storage_texture(width, height, format.to_vk())?;
        self.clear_storage_texture(&storage)?;
        let sampled = self.storage_texture_as_sampled(&storage)?;
        let image = mltrs_render_graph::backend::PhysicalImage {
            storage: storage.bindless_handle(),
            sampled: sampled.bindless_handle(),
        };

        Ok((image, (storage, sampled)))
    }
}

impl BackendTypes for Renderer {
    type IndirectCommand = DrawIndexedIndirectCommand;
    type Resource = (
        super::storage_texture::StorageTextureHandle,
        super::texture::TextureHandle,
    );
}

impl mltrs_render_graph::backend::FrameLookup for Renderer {
    fn uniform_live(&self, index: usize) -> bool {
        self.uniform_buffers.contains(index)
    }
    fn storage_live(&self, index: usize) -> bool {
        self.storage_buffers.contains(index)
    }
    fn singleton_live(&self, index: usize) -> bool {
        self.singleton_buffers.contains(index)
    }
    fn whole_index_count(&self, pipeline: usize) -> u32 {
        use super::pipeline::{GraphicsPipelineIndex, VertexPipelineConfig};
        match &self
            .pipelines
            .get_by_index(GraphicsPipelineIndex::from_raw(pipeline))
            .vertex_pipeline_config
        {
            VertexPipelineConfig::VertexAndIndexBuffers(buffers) => buffers.index_count,
            VertexPipelineConfig::SharedMesh(index) => self.meshes[index.raw()].index_count,
            VertexPipelineConfig::VertexCount => {
                unreachable!("unexpected indexed draw call for non-index pipeline")
            }
        }
    }
}

impl mltrs_render_graph::backend::FrameBackend for super::FrameRenderer<'_> {
    type Backend = Renderer;
    type Error = super::DrawError;
    fn lookup(&self) -> &dyn mltrs_render_graph::backend::FrameLookup {
        self.renderer
    }
    fn submit(
        mut self,
        batch: mltrs_render_graph::commands::CommandBatch,
        on_submitted: impl FnOnce(),
    ) -> Result<(), Self::Error> {
        use super::pipeline::{ComputePipelineIndex, GraphicsPipelineIndex, PickingPipelineHandle};
        use mltrs_render_graph::commands::DrawCallConfig as Logical;
        fn push(
            value: &mltrs_render_graph::commands::PushConstantBytes,
        ) -> super::PushConstantBytes {
            let src = value.as_slice();
            let mut bytes = [0; 128];
            bytes[..src.len()].copy_from_slice(src);
            super::PushConstantBytes {
                bytes,
                len: src.len() as u32,
            }
        }
        // Validate every destination before queueing work or entering the infallible
        // post-wait callback. Slot IDs/types/capacities are untrusted public inputs.
        let writes = validate_uploads(&batch, |uniform, index| {
            let gpu = &self.renderer;
            if uniform {
                gpu.uniform_buffers.upload_target(index, gpu.flight_slot)
            } else {
                gpu.storage_buffers.upload_target(index, gpu.flight_slot)
            }
        })?;
        for (pipeline, groups, constants) in batch.dispatches() {
            self.queue_dispatch_raw(
                ComputePipelineIndex::from_raw(*pipeline),
                *groups,
                constants.as_ref().map(push),
            );
        }
        for draw in batch.draws() {
            let draw_call = match draw.draw_call() {
                Logical::VertexCount(count) => super::DrawCallConfig::VertexCount(count),
                Logical::IndexCount(count) => super::DrawCallConfig::IndexCount(count),
                Logical::IndexRange {
                    first_index,
                    index_count,
                } => super::DrawCallConfig::IndexRange {
                    first_index,
                    index_count,
                },
                Logical::IndexedIndirect(request) => {
                    let (buffer, byte_size) = self
                        .renderer
                        .storage_buffers
                        .live_buffer(request.buffer(), self.renderer.flight_slot)
                        .ok_or_else(|| {
                            super::DrawError::DrawError(anyhow::anyhow!(
                                "indirect buffer is no longer live"
                            ))
                        })?;
                    validate_indirect_layout(
                        request.element_size(),
                        request.alignment(),
                        request.stride(),
                        request.offset(),
                        request.draw_count(),
                        byte_size,
                        self.renderer
                            .physical_device_properties
                            .limits
                            .max_draw_indirect_count,
                    )
                    .map_err(super::DrawError::DrawError)?;
                    super::DrawCallConfig::IndexedIndirect {
                        buffer,
                        offset: request.offset(),
                        draw_count: request.draw_count(),
                    }
                }
            };
            self.pending_draws.push(super::PendingDrawCommand::Draw {
                pipeline_index: GraphicsPipelineIndex::from_raw(draw.pipeline_index()),
                draw_call,
                push_constants: draw.push_constants().map(push),
            });
        }
        let picking = batch.picking().map(|pick| super::PickingDrawConfig {
            picking_handle: PickingPipelineHandle {
                index: GraphicsPipelineIndex::from_raw(pick.pipeline_index()),
            },
            mouse_pixel: pick
                .position()
                .map(|v| (v * self.renderer.render_scale) as u32),
        });
        self.draw_frame_with_submission(
            picking,
            |_gpu| {
                for (src, dst) in writes {
                    // SAFETY: preflight checked the live, writable, mapped destination
                    // and its logical capacity. The consuming frame exclusively borrows
                    // Renderer, keeping these allocations alive and unchanged through
                    // the flight-slot wait. Staging owns separate source memory; byte
                    // alignment is one and MaybeUninit preserves source padding.
                    unsafe {
                        std::ptr::copy_nonoverlapping(src.as_ptr(), dst, src.len());
                    }
                }
            },
            on_submitted,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum UploadKind {
    Uniform,
    Storage,
    ReadOnly,
}

#[derive(Clone, Copy)]
pub(super) struct UploadTarget {
    pub(super) byte_size: u64,
    pub(super) mapped_mem: *mut std::ffi::c_void,
    pub(super) kind: UploadKind,
}

type ValidatedWrite<'a> = (
    &'a [std::mem::MaybeUninit<u8>],
    *mut std::mem::MaybeUninit<u8>,
);

fn validate_uploads(
    batch: &mltrs_render_graph::commands::CommandBatch,
    mut lookup: impl FnMut(bool, usize) -> Option<UploadTarget>,
) -> Result<Vec<ValidatedWrite<'_>>, super::DrawError> {
    let mut writes = Vec::new();
    let mut result = Ok(());
    batch.visit_writes(|uniform, index, src| {
        if result.is_err() {
            return;
        }

        result = (|| {
            let target = lookup(uniform, index)
                .ok_or_else(|| anyhow::anyhow!("upload buffer {index} is no longer live"))?;
            let expected = if uniform {
                UploadKind::Uniform
            } else {
                UploadKind::Storage
            };
            anyhow::ensure!(
                target.kind == expected,
                "upload buffer {index} has incompatible access kind"
            );
            anyhow::ensure!(
                !target.mapped_mem.is_null(),
                "upload buffer {index} is not mapped"
            );
            let bytes = u64::try_from(src.len())?;
            anyhow::ensure!(
                if uniform {
                    bytes == target.byte_size
                } else {
                    bytes <= target.byte_size
                },
                "upload buffer {index} payload size {bytes} does not match logical capacity {}",
                target.byte_size
            );
            writes.push((src, target.mapped_mem.cast()));

            Ok(())
        })();
    });
    result.map_err(super::DrawError::DrawError)?;

    Ok(writes)
}

fn validate_indirect_layout(
    size: usize,
    alignment: usize,
    stride: usize,
    offset: u64,
    count: u32,
    byte_size: u64,
    max_count: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        size == size_of::<DrawIndexedIndirectCommand>()
            && alignment == align_of::<DrawIndexedIndirectCommand>()
            && stride == size,
        "indirect record layout does not match the backend"
    );
    anyhow::ensure!(
        offset.is_multiple_of(alignment as u64)
            && offset.is_multiple_of(4)
            && stride.is_multiple_of(4),
        "indirect offset or stride is misaligned"
    );
    // multiDrawIndirect is required and enabled during device creation.
    anyhow::ensure!(
        count > 0 && count <= max_count,
        "indirect draw count exceeds device restrictions"
    );
    let end = u64::from(count - 1)
        .checked_mul(stride as u64)
        .and_then(|bytes| bytes.checked_add(size as u64))
        .and_then(|bytes| offset.checked_add(bytes));
    anyhow::ensure!(
        end.is_some_and(|end| end <= byte_size),
        "indirect draw exceeds buffer allocation"
    );
    Ok(())
}

impl IndexedIndirectArgs for DrawIndexedIndirectCommand {
    fn index_count(&self) -> u32 {
        self.index_count
    }

    fn instance_count(&self) -> u32 {
        self.instance_count
    }

    fn first_index(&self) -> u32 {
        self.first_index
    }

    fn vertex_offset(&self) -> i32 {
        self.vertex_offset
    }

    fn first_instance(&self) -> u32 {
        self.first_instance
    }
}

#[cfg(test)]
mod upload_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indirect_buffer_layout_ranges() {
        let valid = |offset, count, bytes, max| {
            validate_indirect_layout(20, 4, 20, offset, count, bytes, max)
        };
        assert!(valid(20, 2, 60, 2).is_ok());
        assert!(valid(20, 2, 59, 2).is_err());
        assert!(valid(2, 1, 60, 2).is_err());
        assert!(valid(0, 0, 60, 2).is_err());
        assert!(valid(0, 3, 60, 2).is_err());
        assert!(valid(u64::MAX - 3, 1, u64::MAX, 2).is_err());
        assert!(validate_indirect_layout(24, 4, 24, 0, 1, 60, 2).is_err());
        assert!(validate_indirect_layout(20, 8, 20, 0, 1, 60, 2).is_err());
        assert!(validate_indirect_layout(20, 4, 24, 0, 1, 60, 2).is_err());
    }
}
