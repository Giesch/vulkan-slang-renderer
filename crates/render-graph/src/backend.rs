//! Narrow backend contracts used by graph planning.

/// Logical storage image format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphFormat {
    R32Float,
    Rgba32Float,
}

/// Physical shader metadata, independent of backend keepalive ownership.
#[derive(Clone, Copy)]
pub struct PhysicalImage {
    pub storage: crate::bindless::BindlessHandle<crate::bindless::RwTexture2D>,
    pub sampled: crate::bindless::BindlessHandle<crate::bindless::Sampler2D>,
}

/// Allocates, clears and aliases physical graph textures.
/// Partial failures retain the backend's existing registered-resource policy.
pub trait PreparationBackend: BackendTypes {
    fn max_image_dimension_2d(&self) -> u32;
    fn prepare_image(
        &mut self,
        width: u32,
        height: u32,
        format: GraphFormat,
    ) -> anyhow::Result<(PhysicalImage, Self::Resource)>;
}

/// Which backend buffer address a graph binding requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferAddressKind {
    Current,
    Previous,
    Singleton,
}

/// Read-only resource lookup borrowed while a graph frame is planned.
///
/// Implementations resolve the selected flight slot internally. Raw IDs must
/// originate from backend resource handles; they do not prove resource liveness.
pub trait BindingLookup {
    fn buffer_address(&self, kind: BufferAddressKind, index: usize) -> u64;
}

/// The initialized argument fields of an indexed-indirect record.
///
/// This safe accessor interface is not evidence of any device ABI. A backend
/// must accept only its exact associated record representation for submission.
pub trait IndexedIndirectArgs: crate::GPUWrite {
    fn index_count(&self) -> u32;
    fn instance_count(&self) -> u32;
    fn first_index(&self) -> u32;
    fn vertex_offset(&self) -> i32;
    fn first_instance(&self) -> u32;
}

/// A backend family and the exact indirect record it accepts.
pub trait BackendTypes {
    type IndirectCommand: IndexedIndirectArgs;
    type Resource;
}

/// Read-only runtime resource access; no storage or mapped pointers escape.
pub trait FrameLookup: BindingLookup {
    fn uniform_live(&self, index: usize) -> bool;
    fn storage_live(&self, index: usize) -> bool;
    fn singleton_live(&self, index: usize) -> bool;
    fn whole_index_count(&self, pipeline: usize) -> u32;
}

/// Consumes one frame. Apply writes after the flight-slot wait, preserve queued
/// work, and invoke on_submitted exactly once after submission before presentation.
/// A pre-submit error must not invoke on_submitted. Before applying any write,
/// validate every staged destination against backend-owned liveness, logical byte
/// capacity, mapping, and upload access metadata. Safe public slot constructors
/// do not prove those properties.
pub trait FrameBackend: Sized {
    type Backend: BackendTypes;
    type Error: From<anyhow::Error>;
    fn lookup(&self) -> &dyn FrameLookup;
    fn submit(
        self,
        batch: crate::commands::CommandBatch,
        on_submitted: impl FnOnce(),
    ) -> Result<(), Self::Error>;
}
