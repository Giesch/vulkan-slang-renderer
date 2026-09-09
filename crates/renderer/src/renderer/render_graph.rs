//! A render graph built once at setup and executed with a tuple of per-frame
//! parameter values.
//!
//! Logical textures are versioned: a read sees the most recent write in
//! schedule order, a write produces the next version, a mutate reads and
//! writes the current version in place. The graph derives how many physical
//! images each logical texture needs and rotates them internally, including
//! across the frame boundary, so user code holds no parity state.
//!
//! Generated `*ParamsBindings` structs hold the binding references declared
//! here; the graph resolves them to concrete bindless handles and addresses
//! at execute time and performs every CPU buffer write itself.

#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "tests drive the pure compiler; phase 3a wires it into execution"
    )
)]
mod compile;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "tests construct the plain-data vocabulary; later phases lower into it"
    )
)]
mod desc;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "tests drive pure frame expansion; phase 3a wires it into execution"
    )
)]
mod expand;
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "tests read lowering's transient side tables; phase 3a consumes them"
    )
)]
mod lower;
#[cfg(test)]
mod test_desc;
mod validate;

pub use desc::GraphFormat;

use std::marker::PhantomData;

use ash::vk;

use desc::{SizeClass, TexDecl, TexUsage};
use expand::TexRunState;
use lower::{LowerCtx, LowerDrawCall, NodeAccess, PushInput, UniformInput};

use super::addr::{Addr, ImmutableAddr, ReadAddr};
use super::bindless::{BindlessHandle, RwTexture2D, Sampler2D};
use super::gpu_write::{GPUWrite, PushConstantBlock};
use super::pipeline::{
    Compute, ComputePipelineIndex, DrawIndexed, DrawIndexedIndirect, DrawIndexedIndirectCommand,
    DrawVertexCount, GraphicsPipelineIndex, NoPush, PickingPipelineHandle, PipelineHandle,
    PipelineIndex, PushBlock, VertexPipelineConfig,
};
use super::storage_buffer::{
    GpuOnlyBufferHandle, ImmutableBufferHandle, SingletonBufferHandle, StorageBufferHandle,
    StorageBufferStorage, element_byte_offset,
};
use super::storage_texture::StorageTextureHandle;
use super::texture::TextureHandle;
use super::uniform_buffer::UniformBufferHandle;
use super::{
    DrawCallConfig, DrawError, FrameRenderer, Gpu, MAX_FRAMES_IN_FLIGHT, PendingDrawCommand,
    PickingDrawConfig, PushConstantBytes, Renderer, SingletonBufferStorage, ToVk, range_in_bounds,
};

/// A logical graph texture. Physical images and versioning are graph-internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GraphTex(pub(crate) u32);

impl GraphTex {
    /// Sample the most recent version written before this node.
    pub fn read(self) -> SampledTexBinding {
        SampledTexBinding {
            inner: SampledRef::Graph(self),
        }
    }

    /// Sample the version before the most recent write. Mutations edit the
    /// current version in place, so they do not move this reference.
    pub fn read_previous(self) -> SampledTexBinding {
        SampledTexBinding {
            inner: SampledRef::GraphPrevious(self),
        }
    }

    /// Produce the next version of the texture.
    pub fn write(self) -> StorageTexBinding {
        StorageTexBinding {
            inner: StorageRef::Graph(self, StorageTexAccess::Write),
        }
    }

    /// Read-modify-write the current version in place.
    pub fn mutate(self) -> StorageTexBinding {
        StorageTexBinding {
            inner: StorageRef::Graph(self, StorageTexAccess::Mutate),
        }
    }
}

/// The logical texture declarations a graph is built from.
#[derive(Default)]
pub struct GraphResources {
    decls: Vec<TexDecl>,
}

impl GraphResources {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a logical texture. It persists across frames; each physical
    /// image is created cleared.
    #[track_caller]
    pub fn texture(&mut self, width: u32, height: u32, format: GraphFormat) -> GraphTex {
        let loc = std::panic::Location::caller();
        let tex = GraphTex(self.decls.len() as u32);
        self.decls.push(TexDecl {
            name: format!("tex{} ({}:{})", tex.0, loc.file(), loc.line()),
            size: SizeClass::Fixed(width, height),
            format,
            usage: TexUsage::Storage,
        });

        tex
    }
}

impl ToVk for GraphFormat {
    type Vk = vk::Format;
    fn to_vk(&self) -> Self::Vk {
        match self {
            Self::R32Float => vk::Format::R32_SFLOAT,
            Self::Rgba32Float => vk::Format::R32G32B32A32_SFLOAT,
        }
    }
}

/// A sampled read of a texture: a graph texture's latest version, or a fixed
/// external texture the graph does not track.
#[derive(Debug, Clone, Copy)]
pub struct SampledTexBinding {
    inner: SampledRef,
}

#[derive(Debug, Clone, Copy)]
enum SampledRef {
    Graph(GraphTex),
    GraphPrevious(GraphTex),
    External(BindlessHandle<Sampler2D>),
}

impl From<BindlessHandle<Sampler2D>> for SampledTexBinding {
    fn from(handle: BindlessHandle<Sampler2D>) -> Self {
        Self {
            inner: SampledRef::External(handle),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorageTexAccess {
    Write,
    Mutate,
}

/// A storage-image binding: a graph texture with a declared access, or a
/// fixed external storage image the graph does not track.
#[derive(Debug, Clone, Copy)]
pub struct StorageTexBinding {
    inner: StorageRef,
}

#[derive(Debug, Clone, Copy)]
enum StorageRef {
    Graph(GraphTex, StorageTexAccess),
    External(BindlessHandle<RwTexture2D>),
}

impl From<BindlessHandle<RwTexture2D>> for StorageTexBinding {
    fn from(handle: BindlessHandle<RwTexture2D>) -> Self {
        Self {
            inner: StorageRef::External(handle),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BufferBindingKind {
    Storage,
    GpuOnlyCurrent,
    GpuOnlyPrevious,
    Immutable,
    Singleton,
}

/// A type-erased buffer binding; the hazard identity is `(kind, index)`,
/// never a device address.
#[derive(Debug, Clone, Copy)]
pub struct RawBufferBinding {
    kind: BufferBindingKind,
    index: usize,
    /// byte offset of the bound element, computed while the element type was
    /// still known (the bounds check runs at binding creation)
    byte_offset: u64,
}

/// A read-write buffer pointer binding, resolved to `Addr<T>`.
pub struct BufferBinding<T> {
    raw: RawBufferBinding,
    _elem: PhantomData<fn() -> T>,
}

/// A read-only buffer pointer binding, resolved to `ReadAddr<T>`.
pub struct ReadBufferBinding<T> {
    raw: RawBufferBinding,
    _elem: PhantomData<fn() -> T>,
}

/// A GPU-never-writes buffer pointer binding, resolved to `ImmutableAddr<T>`.
pub struct ImmutableBufferBinding<T> {
    raw: RawBufferBinding,
    _elem: PhantomData<fn() -> T>,
}

// manual impls: derives would add spurious `T: ...` bounds
impl<T> Clone for BufferBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for BufferBinding<T> {}
impl<T> Clone for ReadBufferBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ReadBufferBinding<T> {}
impl<T> Clone for ImmutableBufferBinding<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ImmutableBufferBinding<T> {}

impl<T> std::fmt::Debug for BufferBinding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BufferBinding({:?})", self.raw)
    }
}
impl<T> std::fmt::Debug for ReadBufferBinding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ReadBufferBinding({:?})", self.raw)
    }
}
impl<T> std::fmt::Debug for ImmutableBufferBinding<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImmutableBufferBinding({:?})", self.raw)
    }
}

impl<T> BufferBinding<T> {
    fn new(kind: BufferBindingKind, index: usize) -> Self {
        Self {
            raw: RawBufferBinding {
                kind,
                index,
                byte_offset: 0,
            },
            _elem: PhantomData,
        }
    }

    pub fn erased(&self) -> RawBufferBinding {
        self.raw
    }
}

impl<T> ReadBufferBinding<T> {
    fn new(kind: BufferBindingKind, index: usize) -> Self {
        Self {
            raw: RawBufferBinding {
                kind,
                index,
                byte_offset: 0,
            },
            _elem: PhantomData,
        }
    }

    pub fn erased(&self) -> RawBufferBinding {
        self.raw
    }
}

impl<T> From<BufferBinding<T>> for ReadBufferBinding<T> {
    fn from(binding: BufferBinding<T>) -> Self {
        Self {
            raw: binding.raw,
            _elem: PhantomData,
        }
    }
}

impl<T> ImmutableBufferBinding<T> {
    fn new(kind: BufferBindingKind, index: usize, byte_offset: u64) -> Self {
        Self {
            raw: RawBufferBinding {
                kind,
                index,
                byte_offset,
            },
            _elem: PhantomData,
        }
    }

    pub fn erased(&self) -> RawBufferBinding {
        self.raw
    }
}

/// A `Copy` key for a uniform buffer, minted from the affine handle. The graph
/// captures slots at build time; the game keeps the handle for `drop_*`.
pub struct UniformSlot<T> {
    index: usize,
    _elem: PhantomData<fn() -> T>,
}

/// A `Copy` key for a storage buffer.
pub struct StorageSlot<T> {
    index: usize,
    len: u32,
    _elem: PhantomData<fn() -> T>,
}

/// A `Copy` key for a gpu-only buffer.
pub struct GpuOnlySlot<T> {
    index: usize,
    _elem: PhantomData<fn() -> T>,
}

/// A `Copy` key for an immutable buffer.
pub struct ImmutableSlot<T> {
    index: usize,
    len: u32,
    _elem: PhantomData<fn() -> T>,
}

/// A `Copy` key for a singleton buffer.
pub struct SingletonSlot<T> {
    index: usize,
    len: u32,
    _elem: PhantomData<fn() -> T>,
}

// manual impls: derives would add spurious `T: ...` bounds
impl<T> Clone for UniformSlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for UniformSlot<T> {}
impl<T> Clone for StorageSlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for StorageSlot<T> {}
impl<T> Clone for GpuOnlySlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for GpuOnlySlot<T> {}
impl<T> Clone for ImmutableSlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for ImmutableSlot<T> {}
impl<T> Clone for SingletonSlot<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for SingletonSlot<T> {}

impl<T> From<&UniformBufferHandle<T>> for UniformSlot<T> {
    fn from(handle: &UniformBufferHandle<T>) -> Self {
        Self {
            index: handle.index(),
            _elem: PhantomData,
        }
    }
}

impl<T> From<&StorageBufferHandle<T>> for StorageSlot<T> {
    fn from(handle: &StorageBufferHandle<T>) -> Self {
        Self {
            index: handle.index(),
            len: handle.len(),
            _elem: PhantomData,
        }
    }
}

impl<T> From<&GpuOnlyBufferHandle<T>> for GpuOnlySlot<T> {
    fn from(handle: &GpuOnlyBufferHandle<T>) -> Self {
        Self {
            index: handle.index(),
            _elem: PhantomData,
        }
    }
}

impl<T> From<&ImmutableBufferHandle<T>> for ImmutableSlot<T> {
    fn from(handle: &ImmutableBufferHandle<T>) -> Self {
        Self {
            index: handle.index(),
            len: handle.len(),
            _elem: PhantomData,
        }
    }
}

impl<T> From<&SingletonBufferHandle<T>> for SingletonSlot<T> {
    fn from(handle: &SingletonBufferHandle<T>) -> Self {
        Self {
            index: handle.index(),
            len: handle.len(),
            _elem: PhantomData,
        }
    }
}

impl<T> StorageSlot<T> {
    /// A read-write pointer to the current flight slot's buffer.
    pub fn addr(self) -> BufferBinding<T> {
        BufferBinding::new(BufferBindingKind::Storage, self.index)
    }

    /// A read-only pointer to the current flight slot's buffer.
    pub fn read_addr(self) -> ReadBufferBinding<T> {
        ReadBufferBinding::new(BufferBindingKind::Storage, self.index)
    }
}

impl<T> GpuOnlySlot<T> {
    /// A read-write pointer to the current flight slot's buffer.
    pub fn current(self) -> BufferBinding<T> {
        BufferBinding::new(BufferBindingKind::GpuOnlyCurrent, self.index)
    }

    /// A read-only pointer to the previous flight slot's buffer.
    pub fn previous(self) -> ReadBufferBinding<T> {
        ReadBufferBinding::new(BufferBindingKind::GpuOnlyPrevious, self.index)
    }
}

impl<T> ImmutableSlot<T> {
    pub fn addr(self) -> ImmutableBufferBinding<T> {
        ImmutableBufferBinding::new(BufferBindingKind::Immutable, self.index, 0)
    }

    pub fn addr_at(self, index: u32) -> ImmutableBufferBinding<T> {
        let byte_offset = element_byte_offset(index, self.len, std::mem::size_of::<T>());
        ImmutableBufferBinding::new(BufferBindingKind::Immutable, self.index, byte_offset)
    }
}

impl<T> SingletonSlot<T> {
    pub fn addr(self) -> ImmutableBufferBinding<T> {
        ImmutableBufferBinding::new(BufferBindingKind::Singleton, self.index, 0)
    }

    pub fn addr_at(self, index: u32) -> ImmutableBufferBinding<T> {
        let byte_offset = element_byte_offset(index, self.len, std::mem::size_of::<T>());
        ImmutableBufferBinding::new(BufferBindingKind::Singleton, self.index, byte_offset)
    }
}

/// One erased binding, as reported by [`GraphBindingSet::visit`].
#[derive(Debug, Clone, Copy)]
pub enum GraphBinding {
    SampledTex(SampledTexBinding),
    StorageTex(StorageTexBinding),
    Buffer(RawBufferBinding),
}

/// Implemented by generated `*ParamsBindings` structs; enumerates every
/// resource reference so the graph can track versions and hazards.
pub trait GraphBindingSet {
    fn visit(&self, f: &mut dyn FnMut(GraphBinding));
}

impl GraphBindingSet for () {
    fn visit(&self, _f: &mut dyn FnMut(GraphBinding)) {}
}

fn collect_access<B: GraphBindingSet>(bindings: &B) -> NodeAccess {
    let mut access = NodeAccess::default();
    bindings.visit(&mut |binding| match binding {
        GraphBinding::SampledTex(SampledTexBinding {
            inner: SampledRef::Graph(tex),
        }) => access.reads.push(tex.0),
        GraphBinding::SampledTex(SampledTexBinding {
            inner: SampledRef::GraphPrevious(tex),
        }) => access.prev_reads.push(tex.0),
        GraphBinding::StorageTex(StorageTexBinding {
            inner: StorageRef::Graph(tex, StorageTexAccess::Write),
        }) => access.writes.push(tex.0),
        GraphBinding::StorageTex(StorageTexBinding {
            inner: StorageRef::Graph(tex, StorageTexAccess::Mutate),
        }) => access.mutates.push(tex.0),
        // external textures and buffers take no part in version tracking
        GraphBinding::SampledTex(_) | GraphBinding::StorageTex(_) | GraphBinding::Buffer(_) => {}
    });

    access
}

fn collect_bindings<B: GraphBindingSet>(bindings: &B) -> Vec<GraphBinding> {
    let mut out = vec![];
    bindings.visit(&mut |binding| out.push(binding));

    out
}

/// Resolves bindings for one step: logical texture -> the bindless handle of
/// the physical image holding the right version, buffer binding -> the device
/// address for the current flight slot.
pub struct BindingResolver<'a> {
    tex: &'a [TexRunState],
    phys: &'a [PhysTex],
    storage_buffers: &'a StorageBufferStorage,
    singleton_buffers: &'a SingletonBufferStorage,
    flight_slot: usize,
}

impl BindingResolver<'_> {
    pub fn sampled_tex(&self, binding: SampledTexBinding) -> BindlessHandle<Sampler2D> {
        match binding.inner {
            SampledRef::Graph(tex) => {
                let phys = self.tex[tex.0 as usize].read_phys();
                self.phys[tex.0 as usize].images[phys as usize].sampled
            }
            SampledRef::GraphPrevious(tex) => {
                let phys = self.tex[tex.0 as usize].prev_phys();
                self.phys[tex.0 as usize].images[phys as usize].sampled
            }
            SampledRef::External(handle) => handle,
        }
    }

    pub fn storage_tex(&self, binding: StorageTexBinding) -> BindlessHandle<RwTexture2D> {
        match binding.inner {
            StorageRef::Graph(tex, access) => {
                let state = &self.tex[tex.0 as usize];
                let phys = match access {
                    StorageTexAccess::Write => state.write_phys(),
                    StorageTexAccess::Mutate => state.read_phys(),
                };
                self.phys[tex.0 as usize].images[phys as usize].storage
            }
            StorageRef::External(handle) => handle,
        }
    }

    pub fn buf<T>(&self, binding: BufferBinding<T>) -> Addr<T> {
        Addr::from_raw(self.buffer_address(binding.raw))
    }

    pub fn read_buf<T>(&self, binding: ReadBufferBinding<T>) -> ReadAddr<T> {
        ReadAddr::from_raw(self.buffer_address(binding.raw))
    }

    pub fn immutable_buf<T>(&self, binding: ImmutableBufferBinding<T>) -> ImmutableAddr<T> {
        ImmutableAddr::from_raw(self.buffer_address(binding.raw))
    }

    fn buffer_address(&self, raw: RawBufferBinding) -> u64 {
        let base = match raw.kind {
            BufferBindingKind::Storage
            | BufferBindingKind::GpuOnlyCurrent
            | BufferBindingKind::Immutable => self
                .storage_buffers
                .device_address_by_index(raw.index, self.flight_slot),
            BufferBindingKind::GpuOnlyPrevious => {
                let prev = (self.flight_slot + MAX_FRAMES_IN_FLIGHT - 1) % MAX_FRAMES_IN_FLIGHT;
                self.storage_buffers
                    .device_address_by_index(raw.index, prev)
            }
            BufferBindingKind::Singleton => {
                self.singleton_buffers.device_address_by_index(raw.index)
            }
        };

        base + raw.byte_offset
    }
}

/// Implemented by generated params and push-block structs: splits the GPU
/// struct into per-frame data and build-time resource bindings, and
/// reassembles it once the bindings resolve.
pub trait GraphShaderParams: Sized {
    type Data;
    type Bindings: GraphBindingSet;

    fn assemble(
        data: &Self::Data,
        bindings: &Self::Bindings,
        resolver: &BindingResolver<'_>,
    ) -> Self;
}

/// A repeat node's per-frame iteration count. A newtype, not a bare `u32`,
/// so a count cannot swap with an adjacent scalar in the params tuple.
#[derive(Debug, Clone, Copy)]
pub struct LoopCount(pub u32);

/// One node of the graph. Implemented by the node types below and by tuples
/// of nodes; games compose values, they do not implement this.
pub trait GraphNode {
    /// The per-frame value this node consumes from the params tuple.
    type Frame;

    fn lower(&self, cx: &mut LowerCtx);
    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()>;
}

/// A compute dispatch writing one uniform params buffer per frame.
pub struct ComputeNode<S: GraphShaderParams> {
    pipeline_index: ComputePipelineIndex,
    uniform: UniformSlot<S>,
    group_count: [u32; 3],
    bindings: S::Bindings,
}

pub fn dispatch<S: GraphShaderParams>(
    pipeline: &PipelineHandle<Compute, NoPush>,
    params_buffer: &UniformBufferHandle<S>,
    group_count: [u32; 3],
    bindings: S::Bindings,
) -> ComputeNode<S> {
    ComputeNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        group_count,
        bindings,
    }
}

impl<S: GraphShaderParams + GPUWrite> GraphNode for ComputeNode<S> {
    type Frame = S::Data;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.dispatch(
            self.pipeline_index.raw(),
            self.group_count,
            UniformInput {
                slot: self.uniform.index,
                gpu_size: std::mem::size_of::<S>() as u32,
                data_size: std::mem::size_of::<S::Data>() as u32,
                bindings: collect_bindings(&self.bindings),
            },
            None,
        )
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        let value = S::assemble(frame_data, &self.bindings, &cx.resolver());
        cx.stage_uniform(self.uniform, &value);
        cx.dispatches
            .push((self.pipeline_index, self.group_count, None));
        cx.apply_writes(&collect_access(&self.bindings));

        Ok(())
    }
}

/// A compute dispatch whose push block re-resolves per dispatch — inside a
/// `repeat`, that is what lets its texture references rotate per iteration.
/// The push block's data half is fixed at build time.
pub struct ComputeNodeWithPush<S: GraphShaderParams, B: GraphShaderParams + PushConstantBlock> {
    pipeline_index: ComputePipelineIndex,
    uniform: UniformSlot<S>,
    group_count: [u32; 3],
    bindings: S::Bindings,
    push_bindings: B::Bindings,
    push_data: B::Data,
}

pub fn dispatch_with_push<S, B>(
    pipeline: &PipelineHandle<Compute, PushBlock<B>>,
    params_buffer: &UniformBufferHandle<S>,
    group_count: [u32; 3],
    bindings: S::Bindings,
    push_bindings: B::Bindings,
    push_data: B::Data,
) -> ComputeNodeWithPush<S, B>
where
    S: GraphShaderParams,
    B: GraphShaderParams + PushConstantBlock,
{
    ComputeNodeWithPush {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        group_count,
        bindings,
        push_bindings,
        push_data,
    }
}

impl<S, B> GraphNode for ComputeNodeWithPush<S, B>
where
    S: GraphShaderParams + GPUWrite,
    B: GraphShaderParams + PushConstantBlock,
{
    type Frame = S::Data;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.dispatch(
            self.pipeline_index.raw(),
            self.group_count,
            UniformInput {
                slot: self.uniform.index,
                gpu_size: std::mem::size_of::<S>() as u32,
                data_size: std::mem::size_of::<S::Data>() as u32,
                bindings: collect_bindings(&self.bindings),
            },
            Some(PushInput {
                size: std::mem::size_of::<B>() as u32,
                bindings: collect_bindings(&self.push_bindings),
            }),
        )
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        let value = S::assemble(frame_data, &self.bindings, &cx.resolver());
        let push = B::assemble(&self.push_data, &self.push_bindings, &cx.resolver());
        cx.stage_uniform(self.uniform, &value);
        cx.dispatches.push((
            self.pipeline_index,
            self.group_count,
            Some(PushConstantBytes::from_value(&push)),
        ));
        cx.apply_writes(&collect_access(&self.bindings));
        cx.apply_writes(&collect_access(&self.push_bindings));

        Ok(())
    }
}

/// An opaque per-draw push payload; the wrapper keeps the byte type out of
/// the public trait signature.
pub struct GraphPushPayload {
    bytes: Option<PushConstantBytes>,
}

/// A draw node's optional push block: `()` for none, [`PushValues`] for a
/// per-draw payload resolved at plan time.
pub trait GraphPush {
    fn access(&self) -> NodeAccess;
    fn payload(&self, resolver: &BindingResolver<'_>) -> GraphPushPayload;
    fn lower_input(&self) -> Option<PushInput>;
}

impl GraphPush for () {
    fn access(&self) -> NodeAccess {
        NodeAccess::default()
    }

    fn payload(&self, _resolver: &BindingResolver<'_>) -> GraphPushPayload {
        GraphPushPayload { bytes: None }
    }

    fn lower_input(&self) -> Option<PushInput> {
        None
    }
}

/// A push block's build-time halves: resource bindings plus fixed data.
pub struct PushValues<B: GraphShaderParams + PushConstantBlock> {
    bindings: B::Bindings,
    data: B::Data,
}

pub fn push_values<B: GraphShaderParams + PushConstantBlock>(
    bindings: B::Bindings,
    data: B::Data,
) -> PushValues<B> {
    PushValues { bindings, data }
}

impl<B: GraphShaderParams + PushConstantBlock> GraphPush for PushValues<B> {
    fn access(&self) -> NodeAccess {
        collect_access(&self.bindings)
    }

    fn payload(&self, resolver: &BindingResolver<'_>) -> GraphPushPayload {
        let value = B::assemble(&self.data, &self.bindings, resolver);
        GraphPushPayload {
            bytes: Some(PushConstantBytes::from_value(&value)),
        }
    }

    fn lower_input(&self) -> Option<PushInput> {
        Some(PushInput {
            size: std::mem::size_of::<B>() as u32,
            bindings: collect_bindings(&self.bindings),
        })
    }
}

enum DrawCallKind {
    VertexCount(u32),
    /// the pipeline's whole index source; its count resolves per execute
    WholeIndexed,
    IndexRange {
        first_index: u32,
        index_count: u32,
    },
    IndexedIndirect {
        args_index: usize,
        byte_offset: u64,
        draw_count: u32,
    },
}

/// A draw writing one uniform params buffer per frame. Declaration order is
/// draw order; draw nodes follow every compute node.
pub struct DrawNode<S: GraphShaderParams, P: GraphPush = ()> {
    pipeline_index: GraphicsPipelineIndex,
    uniform: UniformSlot<S>,
    call: DrawCallKind,
    bindings: S::Bindings,
    push: P,
}

pub type DrawVertexCountNode<S> = DrawNode<S, ()>;

pub fn draw_vertex_count<S: GraphShaderParams>(
    pipeline: &PipelineHandle<DrawVertexCount, NoPush>,
    params_buffer: &UniformBufferHandle<S>,
    vertex_count: u32,
    bindings: S::Bindings,
) -> DrawNode<S, ()> {
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: DrawCallKind::VertexCount(vertex_count),
        bindings,
        push: (),
    }
}

pub fn draw_vertex_count_with_push<S, B>(
    pipeline: &PipelineHandle<DrawVertexCount, PushBlock<B>>,
    params_buffer: &UniformBufferHandle<S>,
    vertex_count: u32,
    bindings: S::Bindings,
    push: PushValues<B>,
) -> DrawNode<S, PushValues<B>>
where
    S: GraphShaderParams,
    B: GraphShaderParams + PushConstantBlock,
{
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: DrawCallKind::VertexCount(vertex_count),
        bindings,
        push,
    }
}

pub fn draw_indexed<S: GraphShaderParams>(
    pipeline: &PipelineHandle<DrawIndexed, NoPush>,
    params_buffer: &UniformBufferHandle<S>,
    bindings: S::Bindings,
) -> DrawNode<S, ()> {
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: DrawCallKind::WholeIndexed,
        bindings,
        push: (),
    }
}

pub fn draw_indexed_with_push<S, B>(
    pipeline: &PipelineHandle<DrawIndexed, PushBlock<B>>,
    params_buffer: &UniformBufferHandle<S>,
    bindings: S::Bindings,
    push: PushValues<B>,
) -> DrawNode<S, PushValues<B>>
where
    S: GraphShaderParams,
    B: GraphShaderParams + PushConstantBlock,
{
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: DrawCallKind::WholeIndexed,
        bindings,
        push,
    }
}

pub fn draw_index_range<S: GraphShaderParams>(
    pipeline: &PipelineHandle<DrawIndexed, NoPush>,
    params_buffer: &UniformBufferHandle<S>,
    first_index: u32,
    index_count: u32,
    bindings: S::Bindings,
) -> DrawNode<S, ()> {
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: DrawCallKind::IndexRange {
            first_index,
            index_count,
        },
        bindings,
        push: (),
    }
}

pub fn draw_index_range_with_push<S, B>(
    pipeline: &PipelineHandle<DrawIndexed, PushBlock<B>>,
    params_buffer: &UniformBufferHandle<S>,
    first_index: u32,
    index_count: u32,
    bindings: S::Bindings,
    push: PushValues<B>,
) -> DrawNode<S, PushValues<B>>
where
    S: GraphShaderParams,
    B: GraphShaderParams + PushConstantBlock,
{
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: DrawCallKind::IndexRange {
            first_index,
            index_count,
        },
        bindings,
        push,
    }
}

/// `assert!`, not `debug_assert!`, on the argument range: the command
/// processor fetches these records outside the descriptor model, so
/// `robustBufferAccess` does not clamp a fetch past the allocation.
fn indirect_call(
    args: ImmutableSlot<DrawIndexedIndirectCommand>,
    first_command: u32,
    draw_count: u32,
) -> DrawCallKind {
    assert!(
        draw_count > 0,
        "an indirect draw needs at least one command"
    );
    assert!(
        range_in_bounds(first_command, draw_count, args.len),
        "command range [{first_command}, {first_command} + {draw_count}) out of bounds \
         for an argument buffer of {} command(s)",
        args.len,
    );

    DrawCallKind::IndexedIndirect {
        args_index: args.index,
        byte_offset: element_byte_offset(
            first_command,
            args.len,
            std::mem::size_of::<DrawIndexedIndirectCommand>(),
        ),
        draw_count,
    }
}

pub fn draw_indexed_indirect<S: GraphShaderParams>(
    pipeline: &PipelineHandle<DrawIndexedIndirect, NoPush>,
    params_buffer: &UniformBufferHandle<S>,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
    first_command: u32,
    draw_count: u32,
    bindings: S::Bindings,
) -> DrawNode<S, ()> {
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: indirect_call(args.into(), first_command, draw_count),
        bindings,
        push: (),
    }
}

pub fn draw_indexed_indirect_with_push<S, B>(
    pipeline: &PipelineHandle<DrawIndexedIndirect, PushBlock<B>>,
    params_buffer: &UniformBufferHandle<S>,
    args: &ImmutableBufferHandle<DrawIndexedIndirectCommand>,
    first_command: u32,
    draw_count: u32,
    bindings: S::Bindings,
    push: PushValues<B>,
) -> DrawNode<S, PushValues<B>>
where
    S: GraphShaderParams,
    B: GraphShaderParams + PushConstantBlock,
{
    DrawNode {
        pipeline_index: pipeline.index(),
        uniform: params_buffer.into(),
        call: indirect_call(args.into(), first_command, draw_count),
        bindings,
        push,
    }
}

impl<S: GraphShaderParams + GPUWrite, P: GraphPush> GraphNode for DrawNode<S, P> {
    type Frame = S::Data;

    fn lower(&self, cx: &mut LowerCtx) {
        let call = match self.call {
            DrawCallKind::VertexCount(x) => LowerDrawCall::VertexCount(x),
            DrawCallKind::WholeIndexed => LowerDrawCall::WholeIndexed,
            DrawCallKind::IndexRange {
                first_index,
                index_count,
            } => LowerDrawCall::IndexRange {
                first_index,
                index_count,
            },
            DrawCallKind::IndexedIndirect {
                args_index,
                byte_offset,
                draw_count,
            } => LowerDrawCall::IndexedIndirect {
                args_index,
                byte_offset,
                draw_count,
            },
        };
        cx.draw(
            self.pipeline_index.raw(),
            call,
            UniformInput {
                slot: self.uniform.index,
                gpu_size: std::mem::size_of::<S>() as u32,
                data_size: std::mem::size_of::<S::Data>() as u32,
                bindings: collect_bindings(&self.bindings),
            },
            self.push.lower_input(),
        )
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        let value = S::assemble(frame_data, &self.bindings, &cx.resolver());
        cx.stage_uniform(self.uniform, &value);
        let push_constants = self.push.payload(&cx.resolver()).bytes;
        let draw_call = match self.call {
            DrawCallKind::VertexCount(vertex_count) => DrawCallConfig::VertexCount(vertex_count),
            DrawCallKind::WholeIndexed => {
                DrawCallConfig::IndexCount(cx.whole_index_count(self.pipeline_index))
            }
            DrawCallKind::IndexRange {
                first_index,
                index_count,
            } => {
                // debug-only: a release-build out-of-range draw renders
                // garbage silently under robustBufferAccess
                debug_assert!(
                    range_in_bounds(
                        first_index,
                        index_count,
                        cx.whole_index_count(self.pipeline_index)
                    ),
                    "index range [{first_index}, {first_index} + {index_count}) out of bounds \
                     (index count {})",
                    cx.whole_index_count(self.pipeline_index),
                );
                DrawCallConfig::IndexRange {
                    first_index,
                    index_count,
                }
            }
            DrawCallKind::IndexedIndirect {
                args_index,
                byte_offset,
                draw_count,
            } => DrawCallConfig::IndexedIndirect {
                buffer: cx
                    .renderer
                    .storage_buffers
                    .vk_buffer_by_index(args_index, cx.renderer.flight_slot),
                offset: byte_offset,
                draw_count,
            },
        };
        cx.draws.push(PendingDrawCommand::Draw {
            pipeline_index: self.pipeline_index,
            draw_call,
            push_constants,
        });

        Ok(())
    }
}

/// The per-frame cursor position for a picking node. Deliberately not
/// implemented for bare arrays: a newtype names the position at the call site.
pub trait PickingCursor: Copy {
    /// window-space position, as given to `Game::input`
    fn position(&self) -> [f32; 2];
}

/// Reads back the object id under the cursor; the result stays on
/// `FrameRenderer::picked_object_id` (a two-frame-old readback).
pub struct PickingNode<C: PickingCursor> {
    pipeline_index: GraphicsPipelineIndex,
    _cursor: PhantomData<fn() -> C>,
}

pub fn picking<C: PickingCursor>(pipeline: &PickingPipelineHandle) -> PickingNode<C> {
    PickingNode {
        pipeline_index: pipeline.index,
        _cursor: PhantomData,
    }
}

impl<C: PickingCursor> GraphNode for PickingNode<C> {
    type Frame = C;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.picking(self.pipeline_index.raw())
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        let render_scale = cx.renderer.render_scale;
        let position = frame_data.position();
        cx.picking = Some(PickingDrawConfig {
            picking_handle: PickingPipelineHandle {
                index: self.pipeline_index,
            },
            mouse_pixel: [
                (position[0] * render_scale) as u32,
                (position[1] * render_scale) as u32,
            ],
        });

        Ok(())
    }
}

/// A per-frame CPU upload into a storage buffer.
pub struct UploadNode<T> {
    slot: StorageSlot<T>,
}

pub fn upload<T: GPUWrite>(buffer: &StorageBufferHandle<T>) -> UploadNode<T> {
    UploadNode {
        slot: buffer.into(),
    }
}

impl<T: GPUWrite> GraphNode for UploadNode<T> {
    type Frame = Vec<T>;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.upload(
            self.slot.index,
            std::mem::size_of::<T>() as u32,
            self.slot.len,
        )
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        cx.stage_storage(self.slot, frame_data);

        Ok(())
    }
}

/// Replays its body per iteration; the count arrives in the params tuple as
/// a [`LoopCount`]. Zero and odd counts are legal.
pub struct RepeatNode<B> {
    body: B,
}

pub fn repeat<B: GraphNode>(body: B) -> RepeatNode<B> {
    RepeatNode { body }
}

impl<B: GraphNode> GraphNode for RepeatNode<B> {
    type Frame = (LoopCount, B::Frame);

    fn lower(&self, cx: &mut LowerCtx) {
        cx.begin_repeat();
        self.body.lower(cx);
        cx.end_repeat();
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        for _ in 0..frame_data.0.0 {
            self.body.plan(&frame_data.1, cx)?;
        }

        Ok(())
    }
}

/// Skips its body when the frame value is `None`. A skipped dispatch changes
/// no barriers: the between-dispatch barrier rule is positional.
pub struct OptionalNode<B> {
    inner: B,
}

pub fn optional<B: GraphNode>(inner: B) -> OptionalNode<B> {
    OptionalNode { inner }
}

impl<B: GraphNode> GraphNode for OptionalNode<B> {
    type Frame = Option<B::Frame>;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.begin_optional();
        self.inner.lower(cx);
        cx.end_optional();
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        match frame_data {
            Some(inner_frame) => self.inner.plan(inner_frame, cx),
            None => Ok(()),
        }
    }
}

macro_rules! impl_graph_node_for_tuple {
    ($(($n:ident, $f:tt)),+) => {
        impl<$($n: GraphNode),+> GraphNode for ($($n,)+) {
            type Frame = ($($n::Frame,)+);

            fn lower(&self, cx: &mut LowerCtx) {
                $(self.$f.lower(cx);)+
            }

            fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
                $(self.$f.plan(&frame_data.$f, cx)?;)+

                Ok(())
            }
        }
    };
}

impl_graph_node_for_tuple!((N0, 0));
impl_graph_node_for_tuple!((N0, 0), (N1, 1));
impl_graph_node_for_tuple!((N0, 0), (N1, 1), (N2, 2));
impl_graph_node_for_tuple!((N0, 0), (N1, 1), (N2, 2), (N3, 3));
impl_graph_node_for_tuple!((N0, 0), (N1, 1), (N2, 2), (N3, 3), (N4, 4));
impl_graph_node_for_tuple!((N0, 0), (N1, 1), (N2, 2), (N3, 3), (N4, 4), (N5, 5));
impl_graph_node_for_tuple!(
    (N0, 0),
    (N1, 1),
    (N2, 2),
    (N3, 3),
    (N4, 4),
    (N5, 5),
    (N6, 6)
);
impl_graph_node_for_tuple!(
    (N0, 0),
    (N1, 1),
    (N2, 2),
    (N3, 3),
    (N4, 4),
    (N5, 5),
    (N6, 6),
    (N7, 7)
);
impl_graph_node_for_tuple!(
    (N0, 0),
    (N1, 1),
    (N2, 2),
    (N3, 3),
    (N4, 4),
    (N5, 5),
    (N6, 6),
    (N7, 7),
    (N8, 8)
);
impl_graph_node_for_tuple!(
    (N0, 0),
    (N1, 1),
    (N2, 2),
    (N3, 3),
    (N4, 4),
    (N5, 5),
    (N6, 6),
    (N7, 7),
    (N8, 8),
    (N9, 9)
);
impl_graph_node_for_tuple!(
    (N0, 0),
    (N1, 1),
    (N2, 2),
    (N3, 3),
    (N4, 4),
    (N5, 5),
    (N6, 6),
    (N7, 7),
    (N8, 8),
    (N9, 9),
    (N10, 10)
);
impl_graph_node_for_tuple!(
    (N0, 0),
    (N1, 1),
    (N2, 2),
    (N3, 3),
    (N4, 4),
    (N5, 5),
    (N6, 6),
    (N7, 7),
    (N8, 8),
    (N9, 9),
    (N10, 10),
    (N11, 11)
);

#[derive(Clone, Copy)]
struct PhysImage {
    storage: BindlessHandle<RwTexture2D>,
    sampled: BindlessHandle<Sampler2D>,
}

struct PhysTex {
    images: Vec<PhysImage>,
}

#[derive(Default)]
struct StagedWrites {
    bytes: Vec<u8>,
    targets: Vec<StagedTarget>,
}

enum StagedTarget {
    Uniform {
        buffer_index: usize,
        byte_range: std::ops::Range<usize>,
    },
    Storage {
        buffer_index: usize,
        byte_range: std::ops::Range<usize>,
    },
}

impl StagedWrites {
    fn stage(&mut self, src: *const u8, len: usize) -> std::ops::Range<usize> {
        let start = self.bytes.len();
        self.bytes.reserve(len);
        unsafe {
            std::ptr::copy_nonoverlapping(src, self.bytes.as_mut_ptr().add(start), len);
            self.bytes.set_len(start + len);
        }

        start..start + len
    }

    /// Runs inside the terminal submit's update step, after the flight-slot
    /// timeline wait: the only point where mapped memory may be written.
    fn apply(&self, gpu: &mut Gpu<'_>) {
        for target in &self.targets {
            let (dst, byte_range) = match target {
                StagedTarget::Uniform {
                    buffer_index,
                    byte_range,
                } => (
                    gpu.uniform_buffers
                        .mapped_mem_by_index(*buffer_index, gpu.flight_slot),
                    byte_range,
                ),
                StagedTarget::Storage {
                    buffer_index,
                    byte_range,
                } => (
                    gpu.storage_buffers
                        .mapped_mem_by_index(*buffer_index, gpu.flight_slot),
                    byte_range,
                ),
            };
            let src = &self.bytes[byte_range.clone()];
            unsafe {
                std::ptr::copy_nonoverlapping(src.as_ptr(), dst.cast::<u8>(), src.len());
            }
        }
    }
}

/// Execute-time planning state: the working texture cursors and the command,
/// draw, and CPU-write plans accumulated by the node walk.
pub struct PlanCtx<'a> {
    tex: Vec<TexRunState>,
    phys: &'a [PhysTex],
    renderer: &'a Renderer,
    dispatches: Vec<(ComputePipelineIndex, [u32; 3], Option<PushConstantBytes>)>,
    draws: Vec<PendingDrawCommand>,
    staged: StagedWrites,
    picking: Option<PickingDrawConfig>,
}

impl PlanCtx<'_> {
    fn resolver(&self) -> BindingResolver<'_> {
        BindingResolver {
            tex: &self.tex,
            phys: self.phys,
            storage_buffers: &self.renderer.storage_buffers,
            singleton_buffers: &self.renderer.singleton_buffers,
            flight_slot: self.renderer.flight_slot,
        }
    }

    /// The index count of a pipeline's whole vertex/index source.
    fn whole_index_count(&self, pipeline_index: GraphicsPipelineIndex) -> u32 {
        match &self
            .renderer
            .pipelines
            .get_by_index(pipeline_index)
            .vertex_pipeline_config
        {
            VertexPipelineConfig::VertexAndIndexBuffers(vi_bufs) => vi_bufs.index_count,
            VertexPipelineConfig::SharedMesh(mesh_index) => {
                self.renderer.meshes[mesh_index.raw()].index_count
            }
            VertexPipelineConfig::VertexCount => {
                unreachable!("unexpected indexed draw call for non-index pipeline")
            }
        }
    }

    fn apply_writes(&mut self, access: &NodeAccess) {
        for tex in &access.writes {
            self.tex[*tex as usize].commit_write();
        }
    }

    fn stage_uniform<S>(&mut self, slot: UniformSlot<S>, value: &S) {
        let byte_range = self
            .staged
            .stage((value as *const S).cast::<u8>(), std::mem::size_of::<S>());
        self.staged.targets.push(StagedTarget::Uniform {
            buffer_index: slot.index,
            byte_range,
        });
    }

    fn stage_storage<T>(&mut self, slot: StorageSlot<T>, data: &[T]) {
        assert!(
            data.len() <= slot.len as usize,
            "render graph: upload of {} elements into a buffer of {}",
            data.len(),
            slot.len,
        );
        let byte_range = self
            .staged
            .stage(data.as_ptr().cast::<u8>(), std::mem::size_of_val(data));
        self.staged.targets.push(StagedTarget::Storage {
            buffer_index: slot.index,
            byte_range,
        });
    }
}

/// A build-once graph over a node tuple `N`. `execute` takes `N::Frame` — one
/// per-frame value per data-bearing node, in node order — performs every CPU
/// buffer write itself, and submits the frame.
pub struct RenderGraph<N: GraphNode> {
    nodes: N,
    tex: Vec<TexRunState>,
    phys: Vec<PhysTex>,
    /// the physical images and sampled aliases backing the logical textures
    _keep_alive: Vec<(StorageTextureHandle, TextureHandle)>,
}

impl<N: GraphNode> RenderGraph<N> {
    pub fn new(
        renderer: &mut Renderer,
        resources: GraphResources,
        nodes: N,
    ) -> anyhow::Result<Self> {
        let mut lower = LowerCtx::new(resources.decls.clone());
        nodes.lower(&mut lower);
        let lowered = lower.finish();
        let analysis = validate::validate(&lowered.desc, &lowered.schemas);
        let mut errors = lowered.errors;
        let analysis = match analysis {
            Ok(analysis) => analysis,
            Err(mut validation) => {
                errors.append(&mut validation);
                validate::Analysis { tex_phys: vec![] }
            }
        };
        let max = renderer
            .physical_device_properties
            .limits
            .max_image_dimension2_d;
        errors.append(&mut validate::extent_limit_errors(
            &lowered.desc.textures,
            max,
        ));
        if !errors.is_empty() {
            anyhow::bail!(validate::validation_message(&errors));
        }

        let mut tex = vec![];
        let mut phys = vec![];
        let mut keep_alive = vec![];
        for (i, decl) in resources.decls.iter().enumerate() {
            let phys_count = analysis.tex_phys[i];
            let mut images = vec![];
            for _ in 0..phys_count {
                let SizeClass::Fixed(width, height) = decl.size else {
                    unreachable!()
                };
                let storage =
                    renderer.create_storage_texture(width, height, decl.format.to_vk())?;
                renderer.clear_storage_texture(&storage)?;
                let sampled = renderer.storage_texture_as_sampled(&storage)?;
                images.push(PhysImage {
                    storage: storage.bindless_handle(),
                    sampled: sampled.bindless_handle(),
                });
                keep_alive.push((storage, sampled));
            }
            tex.push(TexRunState::new(phys_count));
            phys.push(PhysTex { images });
        }

        Ok(Self {
            nodes,
            tex,
            phys,
            _keep_alive: keep_alive,
        })
    }

    pub fn execute(
        &mut self,
        frame: FrameRenderer<'_>,
        params: &N::Frame,
    ) -> Result<(), DrawError> {
        let mut cx = PlanCtx {
            tex: self.tex.clone(),
            phys: &self.phys,
            renderer: frame.renderer,
            dispatches: vec![],
            draws: vec![],
            staged: StagedWrites::default(),
            picking: None,
        };
        self.nodes
            .plan(params, &mut cx)
            .map_err(DrawError::DrawError)?;
        let PlanCtx {
            tex,
            dispatches,
            draws,
            staged,
            picking,
            ..
        } = cx;

        // committed before submission: a frame aborted by swapchain
        // recreation still advances versions, exactly as the hand-rolled
        // parity bools flipped before the terminal draw call
        self.tex = tex;

        let mut frame = frame;
        for (pipeline_index, group_count, push_constants) in dispatches {
            frame.queue_dispatch_raw(pipeline_index, group_count, push_constants);
        }
        frame.pending_draws.extend(draws);

        frame.draw_frame(picking, |gpu| staged.apply(gpu))
    }
}

#[cfg(test)]
mod tests {
    use super::{GraphFormat, GraphResources};

    #[test]
    fn texture_names_carry_the_call_site() {
        let mut resources = GraphResources::new();
        resources.texture(8, 8, GraphFormat::R32Float);

        let name = &resources.decls[0].name;
        assert!(name.starts_with("tex0 ("), "{name}");
        assert!(name.contains("render_graph.rs:"), "{name}");
    }
}
