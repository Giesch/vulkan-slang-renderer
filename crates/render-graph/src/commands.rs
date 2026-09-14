//! Read-only logical commands produced by validated graph execution.
use std::mem::MaybeUninit;

#[derive(Clone, Copy)]
pub struct PushConstantBytes {
    bytes: [u8; 128],
    len: usize,
}

impl PushConstantBytes {
    pub(crate) fn from_value<P: crate::PushConstantBlock>(value: &P) -> Self {
        const {
            assert!(std::mem::size_of::<P>() <= 128);
        }
        let mut bytes = [0; 128];
        unsafe {
            std::ptr::copy_nonoverlapping(
                (value as *const P).cast::<u8>(),
                bytes.as_mut_ptr(),
                std::mem::size_of::<P>(),
            );
        }
        Self {
            bytes,
            len: std::mem::size_of::<P>(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

#[derive(Clone, Copy)]
pub enum DrawCallConfig {
    VertexCount(u32),
    IndexCount(u32),
    IndexRange { first_index: u32, index_count: u32 },
    IndexedIndirect(IndirectRequest),
}

/// A typed indirect request erased only inside graph planning.
#[derive(Clone, Copy, Debug)]
pub struct IndirectRequest {
    buffer: usize,
    offset: u64,
    draw_count: u32,
    element_size: usize,
    alignment: usize,
}

impl IndirectRequest {
    pub(crate) fn new<I: crate::backend::IndexedIndirectArgs>(
        buffer: usize,
        offset: u64,
        draw_count: u32,
    ) -> Self {
        Self {
            buffer,
            offset,
            draw_count,
            element_size: size_of::<I>(),
            alignment: align_of::<I>(),
        }
    }

    pub fn buffer(self) -> usize {
        self.buffer
    }

    pub fn offset(self) -> u64 {
        self.offset
    }

    pub fn draw_count(self) -> u32 {
        self.draw_count
    }

    pub fn element_size(self) -> usize {
        self.element_size
    }

    pub fn alignment(self) -> usize {
        self.alignment
    }

    pub fn stride(self) -> usize {
        self.element_size
    }
}

pub struct PendingDrawCommand {
    pub(crate) pipeline_index: usize,
    pub(crate) draw_call: DrawCallConfig,
    pub(crate) push_constants: Option<PushConstantBytes>,
}

impl PendingDrawCommand {
    pub fn pipeline_index(&self) -> usize {
        self.pipeline_index
    }

    pub fn draw_call(&self) -> DrawCallConfig {
        self.draw_call
    }

    pub fn push_constants(&self) -> Option<&PushConstantBytes> {
        self.push_constants.as_ref()
    }
}

pub struct PickingDrawConfig {
    pub(crate) pipeline_index: usize,
    pub(crate) position: [f32; 2],
}

impl PickingDrawConfig {
    pub fn pipeline_index(&self) -> usize {
        self.pipeline_index
    }

    pub fn position(&self) -> [f32; 2] {
        self.position
    }
}

/// Constructed only by graph execution. Backends must retain prior queued work.
pub struct CommandBatch {
    pub(crate) dispatches: Vec<(usize, [u32; 3], Option<PushConstantBytes>)>,
    pub(crate) draws: Vec<PendingDrawCommand>,
    pub(crate) staged: crate::runtime::StagedWrites,
    pub(crate) picking: Option<PickingDrawConfig>,
}

impl CommandBatch {
    pub fn dispatches(&self) -> &[(usize, [u32; 3], Option<PushConstantBytes>)] {
        &self.dispatches
    }

    pub fn draws(&self) -> &[PendingDrawCommand] {
        &self.draws
    }

    pub fn picking(&self) -> Option<&PickingDrawConfig> {
        self.picking.as_ref()
    }

    /// Inspect writes for validation before submission; apply them only after the
    /// destination flight-slot wait. Padding remains uninitialized. The backend
    /// must validate actual destination capacity, mapping, and upload access;
    /// public slot metadata is not proof of a safe destination.
    pub fn visit_writes<'a>(&'a self, mut visit: impl FnMut(bool, usize, &'a [MaybeUninit<u8>])) {
        for target in &self.staged.targets {
            let (uniform, index, range) = match target {
                crate::runtime::StagedTarget::Uniform {
                    buffer_index,
                    byte_range,
                } => (true, *buffer_index, byte_range),
                crate::runtime::StagedTarget::Storage {
                    buffer_index,
                    byte_range,
                } => (false, *buffer_index, byte_range),
            };
            visit(uniform, index, &self.staged.bytes[range.clone()]);
        }
    }
}
