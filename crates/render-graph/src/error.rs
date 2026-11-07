//! Typed render-graph errors, one small type per failure kind.
//!
//! Each graph stage returns an [`eros::Result`] over a named error set:
//!
//! - [`RenderGraph::new`](crate::RenderGraph::new): [`NewErrors`]
//! - [`RenderGraph::prepare`](crate::RenderGraph::prepare): [`PrepareErrors`]
//! - [`GraphNode::plan`](crate::GraphNode::plan): [`PlanErrors`]
//! - [`PreparedRenderGraph::execute`](crate::PreparedRenderGraph::execute):
//!   [`ExecuteErrors`]
//!
//! Backend failures the graph cannot name (allocation, device, submission)
//! travel as the backend's own associated error type, the last member of the
//! prepare and execute sets.

use crate::runtime::{GraphError, validation_message};

/// [`RenderGraph::new`](crate::RenderGraph::new) found one or more logical
/// problems. Every problem is aggregated into this one error.
#[derive(Debug, thiserror::Error)]
#[error("{}", validation_message(&self.problems))]
pub struct GraphValidationFailed {
    pub(crate) problems: Vec<GraphError>,
}

impl GraphValidationFailed {
    /// One human-readable line per logical problem, in detection order.
    pub fn problems(&self) -> impl ExactSizeIterator<Item = String> + '_ {
        self.problems.iter().map(ToString::to_string)
    }
}

/// One fixed-size texture whose extent exceeds the device's maximum 2D image
/// dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OversizedTexture {
    pub texture: String,
    pub width: u32,
    pub height: u32,
}

/// [`RenderGraph::prepare`](crate::RenderGraph::prepare) found textures larger
/// than the device allows. Checked before any allocation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", oversized_message(*.max, .textures))]
pub struct TextureExtentsExceedDevice {
    /// The device's `maxImageDimension2D`.
    pub max: u32,
    pub textures: Vec<OversizedTexture>,
}

fn oversized_message(max: u32, textures: &[OversizedTexture]) -> String {
    let mut message = String::from("render graph preparation failed:");
    for t in textures {
        message.push_str(&format!(
            "\n  - texture {} extent {}x{} exceeds the device maximum {max}",
            t.texture, t.width, t.height
        ));
    }

    message
}

/// A uniform buffer the graph captured at construction is no longer live.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("render graph: uniform buffer slot {slot} was dropped")]
pub struct UniformBufferDropped {
    pub slot: usize,
}

/// A storage or singleton buffer the graph captured at construction is no
/// longer live.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("render graph: {name} ({kind}, slot {slot}) was dropped")]
pub struct BufferDropped {
    /// The buffer's declared graph name.
    pub name: String,
    /// The buffer family: `GpuUpdatedFlight`, `Singleton`, `Immutable` or `Storage`.
    pub kind: &'static str,
    pub slot: usize,
}

/// A runtime-length node list received a frame `Vec` of a different length.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("render graph: {nodes} nodes received {values} frame values")]
pub struct FrameValueCountMismatch {
    pub nodes: usize,
    pub values: usize,
}

/// A per-frame upload has more elements than its storage buffer holds.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "render graph: upload to storage buffer {buffer} has {len} elements; capacity is {capacity}"
)]
pub struct UploadExceedsCapacity {
    pub buffer: usize,
    pub len: usize,
    pub capacity: u32,
}

/// A dynamic node's per-frame uniform bytes do not match its parameter block size.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "render graph: uniform buffer slot {slot} received {received} bytes; \
     its parameter block is {expected} bytes"
)]
pub struct UniformSizeMismatch {
    pub slot: usize,
    pub received: usize,
    pub expected: u32,
}

/// A dynamic node's index range does not fit its pipeline's index source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "render graph: index range [{first_index}, {first_index} + {index_count}) \
     is out of bounds (index count {whole})"
)]
pub struct IndexRangeOutOfBounds {
    pub first_index: u32,
    pub index_count: u32,
    /// The pipeline's whole index count.
    pub whole: u32,
}

/// Errors from [`RenderGraph::new`](crate::RenderGraph::new).
pub type NewErrors = (GraphValidationFailed,);

/// Errors from [`RenderGraph::prepare`](crate::RenderGraph::prepare); `E` is
/// the backend's [`PreparationBackend::PrepareError`](crate::backend::PreparationBackend::PrepareError).
pub type PrepareErrors<E> = (TextureExtentsExceedDevice, E);

/// Errors from re-checking captured buffers before a frame is planned.
pub type BufferLivenessErrors = (UniformBufferDropped, BufferDropped);

/// Errors from planning one frame's commands and writes.
pub type PlanErrors = (
    FrameValueCountMismatch,
    UploadExceedsCapacity,
    UniformSizeMismatch,
    IndexRangeOutOfBounds,
);

/// Errors from [`PreparedRenderGraph::execute`](crate::PreparedRenderGraph::execute);
/// `E` is the frame backend's [`FrameBackend::Error`](crate::backend::FrameBackend::Error).
pub type ExecuteErrors<E> = (
    UniformBufferDropped,
    BufferDropped,
    FrameValueCountMismatch,
    UploadExceedsCapacity,
    UniformSizeMismatch,
    IndexRangeOutOfBounds,
    E,
);
