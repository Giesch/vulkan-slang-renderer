//! Typed errors for the renderer operations a caller could react to.
//!
//! Setup and swapchain failures (`Renderer::init`, `recreate_swapchain`,
//! `on_resize`, frame submission) are reported as `eros::Result<T>` (AnyError)
//! with context, because crashing is the only sensible response to them.
//! Everything below is part of a typed set returned by a resource-creation,
//! readback, or graph path. `ash::vk::Result` is a member of most sets as-is.

use ash::vk;

pub use super::gpu_read::{
    GpuByteLengthMismatch, GpuReadErrors, InvalidGpuEnumTag, InvalidReadbackRange,
};

/// Which bindless descriptor heap ran out of slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindlessHeapKind {
    Texture,
    StorageImage,
}

impl std::fmt::Display for BindlessHeapKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Texture => "texture",
            Self::StorageImage => "storage image",
        })
    }
}

/// Every slot of a bindless heap is taken. Slots are never released.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("bindless {kind} heap is full ({capacity} slots, and slots are never released)")]
pub struct BindlessHeapFull {
    pub kind: BindlessHeapKind,
    pub capacity: u32,
}

/// RGBA8 pixel data did not describe a valid image.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRgbaPixels {
    #[error("expected a non-empty image, got {width}x{height}")]
    Empty { width: u32, height: u32 },
    #[error("image dimensions overflow: {width}x{height}")]
    DimensionsOverflow { width: u32, height: u32 },
    #[error("expected {expected} rgba bytes for {width}x{height}, got {actual}")]
    ByteLength {
        width: u32,
        height: u32,
        expected: usize,
        actual: usize,
    },
}

/// The mip level data passed to a texture constructor does not fit its extent and format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidMipLevels {
    #[error("expected at least one mip level")]
    Empty,
    #[error("too many mip levels for extent: {levels} levels, at most {max} for {width}x{height}")]
    TooMany {
        levels: usize,
        max: u32,
        width: u32,
        height: u32,
    },
    #[error("mip level {level} has {bytes} bytes, expected {expected} for {format:?}")]
    ByteLength {
        level: usize,
        bytes: usize,
        expected: usize,
        format: vk::Format,
    },
}

/// The texture format has no block layout the renderer knows how to upload.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported texture format: {format:?}")]
pub struct UnsupportedTextureFormat {
    pub format: vk::Format,
}

/// A singleton buffer was created from an empty slice.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a singleton buffer needs at least one element; vulkan cannot allocate an empty buffer")]
pub struct EmptySingletonBuffer;

/// `create_mesh` was given no vertices or no indices.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("create_mesh requires non-empty vertex and index data")]
pub struct EmptyMeshData {
    pub vertices: usize,
    pub indices: usize,
}

/// A pipeline's `.with_vertices()` data was empty.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("pipeline for {shader} was given empty vertex data by `.with_vertices()`")]
pub struct EmptyPipelineVertexData {
    pub shader: String,
}

/// The reflected bindless heap set index does not directly follow the reflected sets.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "bindless heap set index {heap_set} does not follow the {reflected_sets} reflected \
     descriptor set(s); appending it would leave a gap"
)]
pub struct BindlessHeapSetGap {
    pub heap_set: u32,
    pub reflected_sets: usize,
}

/// Debug builds only: compiling a shader from its slang source failed.
///
/// Debug builds compile every pipeline's shader from source (for hot reload),
/// so the slang diagnostics surface here. Release builds use the embedded SPIR-V
/// and never return this.
#[derive(Debug, thiserror::Error)]
#[error("failed to compile shader {shader} from source")]
pub struct ShaderSourceCompileFailed {
    pub shader: String,
    #[source]
    pub source: eros::StdError<mltrs_slang_reflection::PrepareShaderErrors>,
}

/// Errors from building a pipeline layout from a shader atlas entry.
///
/// `ShaderSourceCompileFailed` and `std::io::Error` (reading the freshly
/// compiled SPIR-V) occur only in debug builds.
pub type PipelineLayoutErrors = (
    ShaderSourceCompileFailed,
    std::io::Error,
    BindlessHeapSetGap,
    vk::Result,
);

/// Errors from [`Renderer::create_pipeline`](super::Renderer::create_pipeline).
pub type PipelineErrors = (
    EmptyPipelineVertexData,
    ShaderSourceCompileFailed,
    std::io::Error,
    BindlessHeapSetGap,
    vk::Result,
);

/// Errors from the texture constructors that take pixel or mip data.
pub type TextureErrors = (
    InvalidMipLevels,
    UnsupportedTextureFormat,
    BindlessHeapFull,
    vk::Result,
);

/// Errors from creating a texture that only needs a heap slot and Vulkan objects.
pub type HeapResourceErrors = (BindlessHeapFull, vk::Result);

/// Errors from `create_singleton_buffer`.
pub type SingletonBufferErrors = (std::num::TryFromIntError, EmptySingletonBuffer, vk::Result);

/// Errors from `create_indirect_buffer`.
pub type IndirectBufferErrors = (std::num::TryFromIntError, vk::Result);

/// Errors from `create_mesh`.
pub type MeshErrors = (EmptyMeshData, vk::Result);

/// A readback dispatch's group count was zero or above the device limit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidDispatchGroupCount {
    #[error("GPU readback dispatch group count is zero: {group_count:?}")]
    Zero { group_count: [u32; 3] },
    #[error("GPU readback dispatch group count {group_count:?} exceeds device limits {limits:?}")]
    ExceedsLimits {
        group_count: [u32; 3],
        limits: [u32; 3],
    },
}

/// The readback buffer has no host mapping.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("GPU readback buffer is not mapped")]
pub struct ReadbackBufferUnmapped;

/// Errors from [`Renderer::dispatch_readback`](super::Renderer::dispatch_readback).
pub type ReadbackErrors = (
    InvalidDispatchGroupCount,
    InvalidReadbackRange,
    ReadbackBufferUnmapped,
    vk::Result,
    GpuByteLengthMismatch,
    InvalidGpuEnumTag,
);

/// A graph upload was rejected before any write, submit, or commit.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UploadRejected {
    #[error("upload buffer {index} is no longer live")]
    NotLive { index: usize },
    #[error("upload buffer {index} has incompatible access kind")]
    IncompatibleKind { index: usize },
    #[error("upload buffer {index} is not mapped")]
    Unmapped { index: usize },
    #[error(
        "upload buffer {index} payload size {bytes} does not match logical capacity {capacity}"
    )]
    SizeMismatch {
        index: usize,
        bytes: usize,
        capacity: u64,
    },
}

/// A graph indirect draw was rejected before any work was queued.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndirectRejected {
    #[error("indirect buffer is no longer live")]
    BufferNotLive,
    #[error("indirect record layout does not match the backend")]
    LayoutMismatch,
    #[error("indirect offset or stride is misaligned")]
    Misaligned,
    #[error("indirect draw count {count} exceeds device restrictions (1..={max})")]
    DrawCountOutOfRange { count: u32, max: u32 },
    #[error("indirect draw exceeds buffer allocation")]
    ExceedsAllocation,
}

/// The renderer's [`FrameBackend::Error`](mltrs_render_graph::backend::FrameBackend::Error):
/// why a prepared graph's frame was not drawn.
///
/// `Upload` and `Indirect` are rejected before anything is written, queued,
/// or submitted. `Frame` is a failure in frame submission itself (Vulkan,
/// swapchain recreation, debug-build shader hot reload); its source is the
/// AnyError union with its context and backtrace.
#[derive(Debug, thiserror::Error)]
pub enum DrawError {
    #[error(transparent)]
    Upload(#[from] UploadRejected),
    #[error(transparent)]
    Indirect(#[from] IndirectRejected),
    #[error("failed to draw frame")]
    Frame(#[source] eros::StdError),
}

impl From<eros::ErrorUnion> for DrawError {
    fn from(error: eros::ErrorUnion) -> Self {
        Self::Frame(error.into_std_error())
    }
}

/// The renderer's [`PreparationBackend::PrepareError`](mltrs_render_graph::backend::PreparationBackend::PrepareError):
/// allocating one graph texture failed.
pub type GraphImageError = eros::StdError<HeapResourceErrors>;

/// What [`PreparedRenderGraph::execute`](mltrs_render_graph::PreparedRenderGraph::execute)
/// returns for a [`FrameRenderer`](super::FrameRenderer): the graph's own
/// per-frame errors plus the renderer's [`DrawError`].
pub type GraphExecuteErrors = mltrs_render_graph::error::ExecuteErrors<DrawError>;
