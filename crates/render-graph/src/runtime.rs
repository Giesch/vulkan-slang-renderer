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
mod erased;
pub use erased::{DynDrawCall, DynDrawNode};
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

use crate::backend::{BackendTypes, BindingLookup, FrameBackend, FrameLookup, PreparationBackend};
use crate::commands::{DrawCallConfig, PendingDrawCommand, PickingDrawConfig, PushConstantBytes};
use desc::{SizeClass, TexDecl, TexUsage};
use expand::TexRunState;
use lower::{LowerCtx, LowerDrawCall, PushInput, UniformInput};

use super::addr::{Addr, ImmutableAddr, ReadAddr};
use super::bindless::{BindlessHandle, RwTexture2D, Sampler2D};
pub use crate::{GPUWrite, PushConstantBlock};

/// A pipeline whose shader declares no `[[vk::push_constant]]` block.
#[derive(Debug)]
pub struct NoPush;

/// A pipeline whose shader declares `B` as its push constant block.
pub struct PushBlock<B: GraphShaderParams + PushConstantBlock>(PhantomData<B>);

/// A `Copy` construction key for one pipeline family.
macro_rules! graph_pipeline_key {
    ($($name:ident),+ $(,)?) => {$(
        pub struct $name<P = NoPush> {
            index: usize,
            _push: PhantomData<fn() -> P>,
        }

        // manual impls: derives would add spurious `P: ...` bounds
        impl<P> Clone for $name<P> {
            fn clone(&self) -> Self {
                *self
            }
        }
        impl<P> Copy for $name<P> {}
        impl<P> $name<P> {
            /// Backend integration: index must name a live pipeline of this family and push interface.
            pub fn new(index: usize) -> Self {
                Self {
                    index,
                    _push: PhantomData,
                }
            }

            fn index(self) -> usize {
                self.index
            }
        }
    )+};
}

graph_pipeline_key!(
    ComputePipelineKey,
    DrawVertexCountKey,
    DrawIndexedKey,
    DrawIndexedIndirectKey,
);

/// A `Copy` construction key for a picking pipeline.
/// Picking pipelines have no push interface.
#[derive(Clone, Copy)]
pub struct PickingPipelineKey {
    index: usize,
}

impl PickingPipelineKey {
    /// Backend integration: index must name a live picking pipeline.
    pub fn new(index: usize) -> Self {
        Self { index }
    }

    fn index(self) -> usize {
        self.index
    }
}

/// The graph's id for a fixed external texture the graph does not track
/// ie, a bindless heap slot
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ExternalTexId(u64);

impl ExternalTexId {
    pub(crate) fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    fn raw(self) -> u64 {
        self.0
    }
}

/// offset of element `index` in a buffer of `len` elements of `stride` bytes.
/// buffer device addresses bypass `robustBufferAccess` clamping,
/// so an out-of-range element address is undefined behavior
fn element_byte_offset(index: u32, len: u32, stride: usize) -> u64 {
    assert!(
        index < len,
        "element index {index} out of bounds for buffer of {len} element(s)"
    );

    index as u64 * stride as u64
}

/// true if [first, first + count) fits in a buffer of `total` elements,
fn range_in_bounds(first: u32, count: u32, total: u32) -> bool {
    first.checked_add(count).is_some_and(|end| end <= total)
}

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

/// The logical resource declarations a graph is built from.
/// In practice, only textures (at least for now).
#[derive(Default)]
pub struct ResourcePlanner {
    decls: Vec<TexDecl>,
}

impl ResourcePlanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a logical texture.
    pub fn texture(
        &mut self,
        name: &str,
        width: u32,
        height: u32,
        format: GraphFormat,
    ) -> GraphTex {
        let tex = GraphTex(self.decls.len() as u32);
        self.decls.push(TexDecl {
            name: name.to_string(),
            size: SizeClass::Fixed(width, height),
            format,
            usage: TexUsage::Storage,
        });

        tex
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
    External(ExternalTexId),
}

/// The normal input for an external sampled texture: a renderer-side
/// adapter that captures the handle's bindless slot as a graph-owned id.
impl From<BindlessHandle<Sampler2D>> for SampledTexBinding {
    fn from(handle: BindlessHandle<Sampler2D>) -> Self {
        Self {
            inner: SampledRef::External(ExternalTexId::from_raw(handle.to_raw())),
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
    // Kept for defensive lowering tests; no public constructor accepts this.
    #[cfg_attr(not(test), expect(dead_code))]
    External(ExternalTexId),
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

// an immutable or singleton buffer provides strictly more guarantees
// than the ReadAddr<T> requires
impl<T> From<ImmutableBufferBinding<T>> for ReadBufferBinding<T> {
    fn from(binding: ImmutableBufferBinding<T>) -> Self {
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
    element_size: usize,
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

impl<T> UniformSlot<T> {
    /// Backend integration: index must identify a live uniform buffer of T.
    pub fn from_backend(index: usize) -> Self {
        Self {
            index,
            _elem: PhantomData,
        }
    }
}
impl<T> StorageSlot<T> {
    /// Backend integration: index and capacity must describe a live storage buffer of T.
    pub fn from_backend(index: usize, len: u32) -> Self {
        Self {
            index,
            len,
            _elem: PhantomData,
        }
    }
}
impl<T> GpuOnlySlot<T> {
    /// Backend integration: index must identify a live GPU-only buffer of T.
    pub fn from_backend(index: usize) -> Self {
        Self {
            index,
            _elem: PhantomData,
        }
    }
}
impl<T> ImmutableSlot<T> {
    /// Backend integration: index and capacity must describe a live immutable buffer of T.
    pub fn from_backend(index: usize, len: u32) -> Self {
        Self {
            index,
            len,
            element_size: std::mem::size_of::<T>(),
            _elem: PhantomData,
        }
    }
}
impl<T> SingletonSlot<T> {
    /// Backend integration: index and capacity must describe a live singleton buffer of T.
    pub fn from_backend(index: usize, len: u32) -> Self {
        Self {
            index,
            len,
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
        let byte_offset = element_byte_offset(index, self.len, self.element_size);
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

fn commit_writes(
    tex: &mut [TexRunState],
    visit_bindings: impl FnOnce(&mut dyn FnMut(GraphBinding)),
) {
    visit_bindings(&mut |binding| {
        if let GraphBinding::StorageTex(StorageTexBinding {
            inner: StorageRef::Graph(texture, StorageTexAccess::Write),
        }) = binding
        {
            tex[texture.0 as usize].commit_write();
        }
    });
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
    lookup: &'a dyn BindingLookup,
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
            SampledRef::External(id) => BindlessHandle::from_raw(id.raw()),
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
            StorageRef::External(id) => BindlessHandle::from_raw(id.raw()),
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
        use mltrs_render_graph::backend::BufferAddressKind;
        let kind = match raw.kind {
            BufferBindingKind::Storage
            | BufferBindingKind::GpuOnlyCurrent
            | BufferBindingKind::Immutable => BufferAddressKind::Current,
            BufferBindingKind::GpuOnlyPrevious => BufferAddressKind::Previous,
            BufferBindingKind::Singleton => BufferAddressKind::Singleton,
        };

        self.lookup.buffer_address(kind, raw.index) + raw.byte_offset
    }
}

/// Implemented by generated params and push-block structs: splits the GPU
/// struct into per-frame data and build-time resource bindings, and
/// reassembles it once the bindings resolve.
pub trait GraphShaderParams: Sized {
    type Data;
    type Bindings: GraphBindingSet;
    type Input: GraphBindingSet;

    fn input(data: &Self::Data, bindings: &Self::Bindings) -> Self::Input;

    fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self;

    fn assemble(
        data: &Self::Data,
        bindings: &Self::Bindings,
        resolver: &BindingResolver<'_>,
    ) -> Self {
        Self::assemble_input(&Self::input(data, bindings), resolver)
    }
}

/// A repeat node's per-frame iteration count. A newtype, not a bare `u32`,
/// so a count cannot swap with an adjacent scalar in the params tuple.
#[derive(Debug, Clone, Copy)]
pub struct LoopCount(pub u32);

/// One node of the graph.
/// Implemented by the node types below and by tuples of nodes.
/// Games compose values, they do not implement this.
pub trait GraphNode {
    /// The per-frame value this node consumes from the params tuple.
    type Frame;

    /// Lowers this node into the graph's build-time representation.
    fn lower(&self, cx: &mut LowerCtx);

    /// Plans this node's commands and data writes for the current frame.
    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()>;
}

/// A compute dispatch writing one uniform params buffer per frame.
pub struct ComputeNode<S: GraphShaderParams, P = (), Bindings = <S as GraphShaderParams>::Bindings>
{
    pipeline_index: usize,
    uniform: UniformSlot<S>,
    group_count: [u32; 3],
    bindings: Bindings,
    push: P,
}

/// Selects the initial binding state for a compute command.
/// Unit bindings are already complete; generated resource bindings start pending.
pub trait GraphParamBindingSet: GraphBindingSet {
    type Pending;

    fn pending() -> Self::Pending;
}

impl GraphParamBindingSet for () {
    type Pending = ();

    fn pending() {}
}

/// A compute command that still requires its parameter block bindings.
pub struct PendingParamBindings<B>(PhantomData<B>);

impl<B> PendingParamBindings<B> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<B> Default for PendingParamBindings<B> {
    fn default() -> Self {
        Self::new()
    }
}

pub fn dispatch<S, P>(
    pipeline: impl Into<ComputePipelineKey<P>>,
    params_buffer: impl Into<UniformSlot<S>>,
    group_count: [u32; 3],
) -> ComputeNode<S, P::Pending, <S::Bindings as GraphParamBindingSet>::Pending>
where
    S: GraphShaderParams,
    S::Bindings: GraphParamBindingSet,
    P: GraphPipelinePush,
{
    ComputeNode {
        pipeline_index: pipeline.into().index(),
        uniform: params_buffer.into(),
        group_count,
        bindings: S::Bindings::pending(),
        push: P::pending(),
    }
}

impl<S: GraphShaderParams, P> ComputeNode<S, P, PendingParamBindings<S::Bindings>> {
    pub fn with_param_bindings(self, bindings: S::Bindings) -> ComputeNode<S, P> {
        ComputeNode {
            pipeline_index: self.pipeline_index,
            uniform: self.uniform,
            group_count: self.group_count,
            bindings,
            push: self.push,
        }
    }
}

impl<S: GraphShaderParams + GPUWrite, P: GraphPush> GraphNode for ComputeNode<S, P> {
    type Frame = S::Data;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.dispatch(
            self.pipeline_index,
            self.group_count,
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
        let resolver = cx.resolver();
        let value = S::assemble(frame_data, &self.bindings, &resolver);
        let push = self.push.payload(&resolver);
        cx.stage_uniform(self.uniform, &value);
        cx.dispatches
            .push((self.pipeline_index, self.group_count, push.bytes));
        commit_writes(&mut cx.tex, |visit| {
            self.bindings.visit(visit);
            self.push.visit_bindings(visit);
        });

        Ok(())
    }
}

/// A compute dispatch whose push block re-resolves per dispatch — inside a
/// `repeat`, that is what lets its texture references rotate per iteration.
/// The push block's data half is fixed at build time.
pub type ComputeNodeWithPush<S, B> = ComputeNode<S, PushValues<B>>;

/// An opaque command push payload; the wrapper keeps the byte type out of
/// the public trait signature.
pub struct GraphPushPayload {
    bytes: Option<PushConstantBytes>,
}

/// A command's optional push block: `()` for none, [`PushValues`] for a
/// payload resolved at plan time.
pub trait GraphPush {
    fn payload(&self, resolver: &BindingResolver<'_>) -> GraphPushPayload;
    fn lower_input(&self) -> Option<PushInput>;

    /// Visit resource bindings so compute writes can advance texture versions.
    fn visit_bindings(&self, _visit: &mut dyn FnMut(GraphBinding)) {}
}

impl GraphPush for () {
    fn payload(&self, _resolver: &BindingResolver<'_>) -> GraphPushPayload {
        GraphPushPayload { bytes: None }
    }

    fn lower_input(&self) -> Option<PushInput> {
        None
    }
}

/// A push block's complete unresolved input, fixed at graph construction.
pub struct PushValues<B: GraphShaderParams + PushConstantBlock> {
    input: B::Input,
}

/// A command that still needs its pipeline's complete push input.
///
/// Both command forms become graph nodes after the complete input is attached:
/// ```
/// use mltrs_render_graph::*;
/// use mltrs_render_graph::{GPUWrite, PushConstantBlock};
/// fn complete<S, B>(compute: ComputeNode<S, PendingPush<B>>,
///                   draw: DrawNode<S, PendingPush<B>>, input: B::Input)
/// where S: GraphShaderParams + GPUWrite,
///       B: GraphShaderParams + PushConstantBlock, B::Input: Clone {
///     fn node<N: GraphNode>(_: N) {}
///     node(compute.with_push_constant(input.clone()));
///     node(draw.with_push_constant(input));
/// }
/// ```
/// Missing push input cannot enter the graph:
/// ```compile_fail,E0277
/// use mltrs_render_graph::*;
/// use mltrs_render_graph::{GPUWrite, PushConstantBlock};
/// fn incomplete<S: GraphShaderParams + GPUWrite, B: GraphShaderParams + PushConstantBlock>(
///     command: ComputeNode<S, PendingPush<B>>,
/// ) {
///     fn node<N: GraphNode>(_: N) {}
///     node(command);
/// }
/// ```
/// Draws enforce the same restriction:
/// ```compile_fail,E0277
/// use mltrs_render_graph::*;
/// use mltrs_render_graph::{GPUWrite, PushConstantBlock};
/// fn incomplete<S: GraphShaderParams + GPUWrite, B: GraphShaderParams + PushConstantBlock>(
///     command: DrawNode<S, PendingPush<B>>,
/// ) {
///     fn node<N: GraphNode>(_: N) {}
///     node(command);
/// }
/// ```
/// A different push interface is not accepted:
/// ```compile_fail,E0308
/// use mltrs_render_graph::*;
/// use mltrs_render_graph::PushConstantBlock;
/// fn wrong<S, B, Other>(command: DrawNode<S, PendingPush<B>>, input: Other::Input)
/// where S: GraphShaderParams, B: GraphShaderParams + PushConstantBlock,
///       Other: GraphShaderParams {
///     command.with_push_constant(input);
/// }
/// ```
pub struct PendingPush<B>(PhantomData<B>);

mod push_state {
    pub trait Sealed {}

    impl Sealed for super::NoPush {}

    impl<B: super::GraphShaderParams + super::PushConstantBlock> Sealed for super::PushBlock<B> {}
}

/// Selects the initial command state from the pipeline's push interface.
pub trait GraphPipelinePush: push_state::Sealed {
    type Pending;
    fn pending() -> Self::Pending;
}

impl GraphPipelinePush for NoPush {
    type Pending = ();

    fn pending() {}
}

impl<B: GraphShaderParams + PushConstantBlock> GraphPipelinePush for PushBlock<B> {
    type Pending = PendingPush<B>;

    fn pending() -> Self::Pending {
        PendingPush(PhantomData)
    }
}

impl<S: GraphShaderParams, B: GraphShaderParams + PushConstantBlock, Bindings>
    ComputeNode<S, PendingPush<B>, Bindings>
{
    pub fn with_push_constant(self, input: B::Input) -> ComputeNode<S, PushValues<B>, Bindings> {
        ComputeNode {
            pipeline_index: self.pipeline_index,
            uniform: self.uniform,
            group_count: self.group_count,
            bindings: self.bindings,
            push: PushValues { input },
        }
    }
}

impl<S: GraphShaderParams, B: GraphShaderParams + PushConstantBlock> DrawNode<S, PendingPush<B>> {
    pub fn with_push_constant(self, input: B::Input) -> DrawNode<S, PushValues<B>> {
        DrawNode {
            pipeline_index: self.pipeline_index,
            uniform: self.uniform,
            call: self.call,
            bindings: self.bindings,
            push: PushValues { input },
        }
    }
}

impl<B: GraphShaderParams + PushConstantBlock> GraphPush for PushValues<B> {
    fn visit_bindings(&self, visit: &mut dyn FnMut(GraphBinding)) {
        self.input.visit(visit);
    }

    fn payload(&self, resolver: &BindingResolver<'_>) -> GraphPushPayload {
        let value = B::assemble_input(&self.input, resolver);
        GraphPushPayload {
            bytes: Some(PushConstantBytes::from_value(&value)),
        }
    }

    fn lower_input(&self) -> Option<PushInput> {
        Some(PushInput {
            size: std::mem::size_of::<B>() as u32,
            bindings: collect_bindings(&self.input),
        })
    }
}

/// A draw writing one uniform params buffer per frame. Declaration order is
/// draw order; draw nodes follow every compute node.
pub struct DrawNode<S: GraphShaderParams, P = ()> {
    pipeline_index: usize,
    uniform: UniformSlot<S>,
    call: LowerDrawCall,
    bindings: S::Bindings,
    push: P,
}

pub type DrawVertexCountNode<S> = DrawNode<S, ()>;

pub fn draw_vertex_count<S: GraphShaderParams, P: GraphPipelinePush>(
    pipeline: impl Into<DrawVertexCountKey<P>>,
    params_buffer: impl Into<UniformSlot<S>>,
    vertex_count: u32,
    bindings: S::Bindings,
) -> DrawNode<S, P::Pending> {
    DrawNode {
        pipeline_index: pipeline.into().index(),
        uniform: params_buffer.into(),
        call: LowerDrawCall::VertexCount(vertex_count),
        bindings,
        push: P::pending(),
    }
}

pub fn draw_indexed<S: GraphShaderParams, P: GraphPipelinePush>(
    pipeline: impl Into<DrawIndexedKey<P>>,
    params_buffer: impl Into<UniformSlot<S>>,
    bindings: S::Bindings,
) -> DrawNode<S, P::Pending> {
    DrawNode {
        pipeline_index: pipeline.into().index(),
        uniform: params_buffer.into(),
        call: LowerDrawCall::WholeIndexed,
        bindings,
        push: P::pending(),
    }
}

pub fn draw_index_range<S: GraphShaderParams, P: GraphPipelinePush>(
    pipeline: impl Into<DrawIndexedKey<P>>,
    params_buffer: impl Into<UniformSlot<S>>,
    first_index: u32,
    index_count: u32,
    bindings: S::Bindings,
) -> DrawNode<S, P::Pending> {
    DrawNode {
        pipeline_index: pipeline.into().index(),
        uniform: params_buffer.into(),
        call: LowerDrawCall::IndexRange {
            first_index,
            index_count,
        },
        bindings,
        push: P::pending(),
    }
}

fn indirect_call<I: crate::backend::IndexedIndirectArgs>(
    args: ImmutableSlot<I>,
    first_command: u32,
    draw_count: u32,
) -> LowerDrawCall {
    // `assert!`, not `debug_assert!`, on the argument range: the command
    // processor fetches these records outside the descriptor model, so
    // `robustBufferAccess` does not clamp a fetch past the allocation.
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

    LowerDrawCall::IndexedIndirect {
        args_index: args.index,
        byte_offset: element_byte_offset(first_command, args.len, size_of::<I>()),
        draw_count,
        request: crate::commands::IndirectRequest::new::<I>(
            args.index,
            element_byte_offset(first_command, args.len, size_of::<I>()),
            draw_count,
        ),
    }
}

pub struct IndirectDrawNode<S: GraphShaderParams, P, I> {
    draw: DrawNode<S, P>,
    args: ImmutableSlot<I>,
}

impl<S: GraphShaderParams, B: GraphShaderParams + PushConstantBlock, I>
    IndirectDrawNode<S, PendingPush<B>, I>
{
    pub fn with_push_constant(self, input: B::Input) -> IndirectDrawNode<S, PushValues<B>, I> {
        IndirectDrawNode {
            draw: self.draw.with_push_constant(input),
            args: self.args,
        }
    }
}

impl<S: GraphShaderParams + GPUWrite, P: GraphPush, I: crate::backend::IndexedIndirectArgs>
    GraphNode for IndirectDrawNode<S, P, I>
{
    type Frame = S::Data;
    fn lower(&self, cx: &mut LowerCtx) {
        self.draw.lower(cx);
    }
    fn plan(&self, data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        self.draw.plan(data, cx)
    }
}

pub fn draw_indexed_indirect<
    S: GraphShaderParams,
    P: GraphPipelinePush,
    I: crate::backend::IndexedIndirectArgs,
>(
    pipeline: impl Into<DrawIndexedIndirectKey<P>>,
    params_buffer: impl Into<UniformSlot<S>>,
    args: impl Into<ImmutableSlot<I>>,
    first_command: u32,
    draw_count: u32,
    bindings: S::Bindings,
) -> IndirectDrawNode<S, P::Pending, I> {
    let args = args.into();
    let call = indirect_call(args, first_command, draw_count);

    IndirectDrawNode {
        args,
        draw: DrawNode {
            pipeline_index: pipeline.into().index(),
            uniform: params_buffer.into(),
            call,
            bindings,
            push: P::pending(),
        },
    }
}

impl<S: GraphShaderParams + GPUWrite, P: GraphPush> GraphNode for DrawNode<S, P> {
    type Frame = S::Data;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.draw(
            self.pipeline_index,
            self.call,
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
        let pipeline_index = self.pipeline_index;
        let value = S::assemble(frame_data, &self.bindings, &cx.resolver());
        cx.stage_uniform(self.uniform, &value);
        let push_constants = self.push.payload(&cx.resolver()).bytes;
        let draw_call = match self.call {
            LowerDrawCall::VertexCount(vertex_count) => DrawCallConfig::VertexCount(vertex_count),
            LowerDrawCall::WholeIndexed => {
                DrawCallConfig::IndexCount(cx.whole_index_count(pipeline_index))
            }
            LowerDrawCall::IndexRange {
                first_index,
                index_count,
            } => {
                // debug-only: a release-build out-of-range draw renders
                // garbage silently under robustBufferAccess
                debug_assert!(
                    range_in_bounds(
                        first_index,
                        index_count,
                        cx.whole_index_count(pipeline_index)
                    ),
                    "index range [{first_index}, {first_index} + {index_count}) out of bounds \
                     (index count {})",
                    cx.whole_index_count(pipeline_index),
                );
                DrawCallConfig::IndexRange {
                    first_index,
                    index_count,
                }
            }
            LowerDrawCall::IndexedIndirect { request, .. } => {
                DrawCallConfig::IndexedIndirect(request)
            }
        };
        cx.draws.push(PendingDrawCommand {
            pipeline_index,
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
    pipeline_index: usize,
    _cursor: PhantomData<fn() -> C>,
}

pub fn picking<C: PickingCursor>(pipeline: impl Into<PickingPipelineKey>) -> PickingNode<C> {
    PickingNode {
        pipeline_index: pipeline.into().index(),
        _cursor: PhantomData,
    }
}

impl<C: PickingCursor> GraphNode for PickingNode<C> {
    type Frame = C;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.picking(self.pipeline_index)
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        cx.picking = Some(PickingDrawConfig {
            pipeline_index: self.pipeline_index,
            position: frame_data.position(),
        });

        Ok(())
    }
}

/// A per-frame CPU upload into a storage buffer.
pub struct UploadNode<T> {
    slot: StorageSlot<T>,
}

pub fn upload<T: GPUWrite>(slot: impl Into<StorageSlot<T>>) -> UploadNode<T> {
    UploadNode { slot: slot.into() }
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
        cx.staged.stage_storage(self.slot, frame_data)?;

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

mod compatible {
    pub trait Sealed<B> {}
}
/// Sealed proof that all indirect records match this backend's exact representation.
pub trait CompatibleWith<B: BackendTypes>: GraphNode + compatible::Sealed<B> {}
impl<N: GraphNode + compatible::Sealed<B>, B: BackendTypes> CompatibleWith<B> for N {}
impl<S: GraphShaderParams + GPUWrite, P: GraphPush, B: BackendTypes> compatible::Sealed<B>
    for ComputeNode<S, P>
{
}
impl<S: GraphShaderParams + GPUWrite, P: GraphPush, B: BackendTypes> compatible::Sealed<B>
    for DrawNode<S, P>
{
}
impl<
    S: GraphShaderParams + GPUWrite,
    P: GraphPush,
    I: crate::backend::IndexedIndirectArgs,
    B: BackendTypes<IndirectCommand = I>,
> compatible::Sealed<B> for IndirectDrawNode<S, P, I>
{
}
impl<C: PickingCursor, B: BackendTypes> compatible::Sealed<B> for PickingNode<C> {}
impl<T: GPUWrite, B: BackendTypes> compatible::Sealed<B> for UploadNode<T> {}
impl<N: CompatibleWith<B>, B: BackendTypes> compatible::Sealed<B> for RepeatNode<N> {}
impl<N: CompatibleWith<B>, B: BackendTypes> compatible::Sealed<B> for OptionalNode<N> {}
impl<N: CompatibleWith<B>, B: BackendTypes, const K: usize> compatible::Sealed<B> for [N; K] {}
impl<N: CompatibleWith<B>, B: BackendTypes> compatible::Sealed<B> for Vec<N> {}

/// A runtime-length list of one node type, for graphs built from data. Its
/// frame is a `Vec` of the same length; a mismatch is an execution error.
impl<N: GraphNode> GraphNode for Vec<N> {
    type Frame = Vec<N::Frame>;

    fn lower(&self, cx: &mut LowerCtx) {
        for node in self {
            node.lower(cx);
        }
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        anyhow::ensure!(
            frame_data.len() == self.len(),
            "render graph: {} nodes received {} frame values",
            self.len(),
            frame_data.len(),
        );
        for (node, frame) in self.iter().zip(frame_data) {
            node.plan(frame, cx)?;
        }

        Ok(())
    }
}

impl<N: GraphNode, const K: usize> GraphNode for [N; K] {
    type Frame = [N::Frame; K];

    fn lower(&self, cx: &mut LowerCtx) {
        for node in self {
            node.lower(cx);
        }
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        for (node, frame) in self.iter().zip(frame_data) {
            node.plan(frame, cx)?;
        }

        Ok(())
    }
}

macro_rules! impl_graph_node_for_tuple {
    ($(($n:ident, $f:tt)),+) => {
        impl<Backend: BackendTypes, $($n: CompatibleWith<Backend>),+> compatible::Sealed<Backend> for ($($n,)+) {}
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

#[cfg(test)]
mod backend_tests;

#[derive(Clone, Copy)]
struct PhysImage {
    storage: BindlessHandle<RwTexture2D>,
    sampled: BindlessHandle<Sampler2D>,
}

struct PhysTex {
    images: Vec<PhysImage>,
}

#[derive(Default)]
pub(crate) struct StagedWrites {
    pub(crate) bytes: Vec<std::mem::MaybeUninit<u8>>,
    pub(crate) targets: Vec<StagedTarget>,
}

pub(crate) enum StagedTarget {
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
    fn stage<T>(&mut self, values: &[T]) -> std::ops::Range<usize> {
        let len = std::mem::size_of_val(values);
        let src = values.as_ptr().cast::<std::mem::MaybeUninit<u8>>();
        let start = self.bytes.len();
        self.bytes.reserve(len);
        // The source may have uninitialized padding. Copy and retain it as
        // MaybeUninit bytes; never observe padding as an initialized u8.
        unsafe {
            std::ptr::copy_nonoverlapping(src, self.bytes.as_mut_ptr().add(start), len);
            self.bytes.set_len(start + len);
        }

        start..start + len
    }

    fn stage_storage<T>(&mut self, slot: StorageSlot<T>, data: &[T]) -> anyhow::Result<()> {
        anyhow::ensure!(
            data.len() <= slot.len as usize,
            "render graph: upload to storage buffer {} has {} elements; capacity is {}",
            slot.index,
            data.len(),
            slot.len,
        );
        let byte_range = self.stage(data);
        self.targets.push(StagedTarget::Storage {
            buffer_index: slot.index,
            byte_range,
        });

        Ok(())
    }
}

/// Execute-time planning state: the working texture cursors and the command,
/// draw, and CPU-write plans accumulated by the node walk.
pub struct PlanCtx<'a> {
    tex: Vec<TexRunState>,
    phys: &'a [PhysTex],
    renderer: &'a dyn FrameLookup,
    dispatches: Vec<(usize, [u32; 3], Option<PushConstantBytes>)>,
    draws: Vec<PendingDrawCommand>,
    staged: StagedWrites,
    picking: Option<PickingDrawConfig>,
}

impl PlanCtx<'_> {
    fn resolver(&self) -> BindingResolver<'_> {
        BindingResolver {
            tex: &self.tex,
            phys: self.phys,
            lookup: self.renderer,
        }
    }

    /// The index count of a pipeline's whole vertex/index source.
    fn whole_index_count(&self, pipeline_index: usize) -> u32 {
        self.renderer.whole_index_count(pipeline_index)
    }

    fn stage_uniform<S>(&mut self, slot: UniformSlot<S>, value: &S) {
        self.stage_uniform_bytes(slot.index, std::slice::from_ref(value));
    }

    fn stage_uniform_bytes<T>(&mut self, buffer_index: usize, value: &[T]) {
        let byte_range = self.staged.stage(value);
        self.staged.targets.push(StagedTarget::Uniform {
            buffer_index,
            byte_range,
        });
    }
}

struct CapturedBufferSlot {
    kind: desc::BufferKind,
    index: usize,
    name: String,
}

/// A validated graph made from a node tuple `N`, before any renderer
/// resources are created.
///
/// [`RenderGraph::prepare`] converts this logical graph into an
/// executbale [`PreparedRenderGraph`] using a renderer.
pub struct RenderGraph<N: GraphNode> {
    nodes: N,
    uniform_slots: Vec<usize>,
    buffer_slots: Vec<CapturedBufferSlot>,
    /// the logical texture declarations
    texture_decls: Vec<TexDecl>,
    /// analyzed physical-image count per logical texture
    tex_phys: Vec<u32>,
    /// initial version cursors
    tex: Vec<TexRunState>,
}

impl<N: GraphNode> RenderGraph<N> {
    /// Validate and create the graph.
    pub fn new(resources: ResourcePlanner, nodes: N) -> anyhow::Result<Self> {
        let mut lower = LowerCtx::new(resources.decls);
        nodes.lower(&mut lower);
        let lowered = lower.finish();
        let analysis = validate::validate(&lowered.desc, &lowered.schemas);
        let mut errors = lowered.errors;
        let tex_phys = match analysis {
            Ok(analysis) => analysis.tex_phys,
            Err(mut validation) => {
                errors.append(&mut validation);
                vec![]
            }
        };
        if !errors.is_empty() {
            anyhow::bail!(validate::validation_message(&errors));
        }

        let buffer_slots = lowered
            .desc
            .buffers
            .into_iter()
            .zip(lowered.buffer_indices)
            .map(|(decl, index)| CapturedBufferSlot {
                kind: decl.kind,
                index,
                name: decl.name,
            })
            .collect();

        Ok(Self {
            nodes,
            uniform_slots: lowered.uniform_slots,
            buffer_slots,
            texture_decls: lowered.desc.textures,
            tex: tex_phys.iter().copied().map(TexRunState::new).collect(),
            tex_phys,
        })
    }

    /// Prepare the validated logical graph against a live renderer.
    /// Device limits are checked, then physical textures created,
    /// cleared (a blocking graphics-queue submission), and given a
    /// sampled alias.
    ///
    /// A partially-created graph can have stray created resources.
    ///
    /// Error staging: logical validation problems are reported by
    /// [`RenderGraph::new`]; this step reports device-limit and
    /// allocation/clear/alias failures; runtime execution problems
    /// (dropped buffers, oversized uploads) are reported by
    /// [`PreparedRenderGraph::execute`].
    pub fn prepare<B: PreparationBackend>(
        self,
        renderer: &mut B,
    ) -> anyhow::Result<PreparedRenderGraph<N, B>>
    where
        N: CompatibleWith<B>,
    {
        let max = renderer.max_image_dimension_2d();
        let extent_errors = validate::extent_limit_errors(&self.texture_decls, max);
        if !extent_errors.is_empty() {
            anyhow::bail!(validate::validation_message(&extent_errors));
        }

        let mut phys = vec![];
        let mut keep_alive = vec![];
        for (i, decl) in self.texture_decls.iter().enumerate() {
            let phys_count = self.tex_phys[i];
            let mut images = vec![];
            for _ in 0..phys_count {
                let SizeClass::Fixed(width, height) = decl.size else {
                    unreachable!()
                };
                let (image, resource) = renderer.prepare_image(width, height, decl.format)?;
                images.push(PhysImage {
                    storage: image.storage,
                    sampled: image.sampled,
                });
                keep_alive.push(resource);
            }
            phys.push(PhysTex { images });
        }

        Ok(PreparedRenderGraph {
            nodes: self.nodes,
            uniform_slots: self.uniform_slots,
            buffer_slots: self.buffer_slots,
            tex: self.tex,
            phys,
            _keep_alive: keep_alive,
        })
    }
}

/// An executable render graph, returned by [`RenderGraph::prepare`]
/// Includes a validated logical graph plus the renderer resources backing it.
pub struct PreparedRenderGraph<N: GraphNode, B: BackendTypes> {
    nodes: N,
    uniform_slots: Vec<usize>,
    buffer_slots: Vec<CapturedBufferSlot>,
    tex: Vec<TexRunState>,
    phys: Vec<PhysTex>,
    /// the physical images and sampled aliases backing the logical textures
    _keep_alive: Vec<B::Resource>,
}

impl<N: GraphNode, B: BackendTypes> PreparedRenderGraph<N, B> {
    // NOTE This is necessary because the graph uses copied slot keys,
    // instead of references to owned ones.
    // TODO An alternative would be to use reference counted handles
    // that keep resources alive while a graph referencing them is alive.
    fn validate_buffers(&self, lookup: &dyn FrameLookup) -> anyhow::Result<()> {
        for index in &self.uniform_slots {
            anyhow::ensure!(
                lookup.uniform_live(*index),
                "render graph: uniform buffer slot {index} was dropped"
            );
        }

        for slot in &self.buffer_slots {
            let live = match slot.kind {
                desc::BufferKind::Singleton => lookup.singleton_live(slot.index),
                _ => lookup.storage_live(slot.index),
            };
            anyhow::ensure!(
                live,
                "render graph: {} ({:?}, slot {}) was dropped",
                slot.name,
                slot.kind,
                slot.index,
            );
        }

        Ok(())
    }

    pub fn execute<F: FrameBackend<Backend = B>>(
        &mut self,
        frame: F,
        params: &N::Frame,
    ) -> Result<(), F::Error>
    where
        N: CompatibleWith<B>,
    {
        self.validate_buffers(frame.lookup())?;
        let mut cx = PlanCtx {
            tex: self.tex.clone(),
            phys: &self.phys,
            renderer: frame.lookup(),
            dispatches: vec![],
            draws: vec![],
            staged: StagedWrites::default(),
            picking: None,
        };
        self.nodes.plan(params, &mut cx)?;
        let PlanCtx {
            tex,
            dispatches,
            draws,
            staged,
            picking,
            ..
        } = cx;

        frame.submit(
            crate::commands::CommandBatch {
                dispatches,
                draws,
                staged,
                picking,
            },
            // only cycle textures if the frame submission succeeded
            || self.tex = tex,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[repr(C)]
    pub(super) struct DrawIndexedIndirectCommand([u32; 5]);
    impl GPUWrite for DrawIndexedIndirectCommand {}
    impl crate::backend::IndexedIndirectArgs for DrawIndexedIndirectCommand {
        fn index_count(&self) -> u32 {
            self.0[0]
        }
        fn instance_count(&self) -> u32 {
            self.0[1]
        }
        fn first_index(&self) -> u32 {
            self.0[2]
        }
        fn vertex_offset(&self) -> i32 {
            self.0[3] as i32
        }
        fn first_instance(&self) -> u32 {
            self.0[4]
        }
    }
    struct EmptyBackend;
    impl BackendTypes for EmptyBackend {
        type IndirectCommand = DrawIndexedIndirectCommand;
        type Resource = ();
    }
    impl BindingLookup for EmptyBackend {
        fn buffer_address(&self, _: crate::backend::BufferAddressKind, _: usize) -> u64 {
            0
        }
    }
    impl FrameLookup for EmptyBackend {
        fn uniform_live(&self, _: usize) -> bool {
            false
        }
        fn storage_live(&self, _: usize) -> bool {
            false
        }
        fn singleton_live(&self, _: usize) -> bool {
            false
        }
        fn whole_index_count(&self, _: usize) -> u32 {
            0
        }
    }
    use std::marker::PhantomData;

    #[test]
    fn cursor_commits_only_follow_graph_writes() {
        use super::*;

        let mut tex = vec![TexRunState::new(2), TexRunState::new(1)];
        let ignored = [
            GraphBinding::SampledTex(GraphTex(0).read()),
            GraphBinding::SampledTex(GraphTex(0).read_previous()),
            GraphBinding::StorageTex(GraphTex(0).mutate()),
            GraphBinding::SampledTex(BindlessHandle::<Sampler2D>::from_raw(5).into()),
            GraphBinding::StorageTex(StorageTexBinding {
                inner: StorageRef::External(ExternalTexId::from_raw(6)),
            }),
            GraphBinding::Buffer(BufferBinding::<u32>::new(BufferBindingKind::Storage, 9).erased()),
        ];
        commit_writes(&mut tex, |visit| {
            for binding in ignored {
                visit(binding);
            }
        });
        assert_eq!(tex[0].cursor, 0);

        for expected in [1, 0, 1] {
            commit_writes(&mut tex, |visit| {
                visit(GraphBinding::StorageTex(GraphTex(0).write()));
                visit(GraphBinding::StorageTex(GraphTex(1).write()));
            });
            assert_eq!(tex[0].cursor, expected);
            assert_eq!(tex[1].cursor, 0);
        }
    }

    #[test]
    fn uniform_and_push_resolve_before_commit_and_push_data_stays_fixed() {
        use super::*;

        #[derive(Clone, Copy)]
        struct Bindings {
            read: SampledTexBinding,
            write: StorageTexBinding,
        }

        impl GraphBindingSet for Bindings {
            fn visit(&self, visit: &mut dyn FnMut(GraphBinding)) {
                visit(GraphBinding::SampledTex(self.read));
                visit(GraphBinding::StorageTex(self.write));
            }
        }

        impl GraphBindingSet for (u64, Bindings) {
            fn visit(&self, visit: &mut dyn FnMut(GraphBinding)) {
                self.1.visit(visit);
            }
        }

        // All fields are initialized, with no interior or trailing padding.
        #[repr(C)]
        struct Block([u64; 3]);

        impl GPUWrite for Block {}
        impl PushConstantBlock for Block {}

        impl GraphShaderParams for Block {
            type Data = u64;
            type Bindings = Bindings;
            type Input = (u64, Bindings);

            fn input(data: &u64, bindings: &Bindings) -> Self::Input {
                (*data, *bindings)
            }

            fn assemble_input(input: &Self::Input, resolver: &BindingResolver<'_>) -> Self {
                let (data, bindings) = input;

                Self([
                    resolver.sampled_tex(bindings.read).to_raw(),
                    resolver.storage_tex(bindings.write).to_raw(),
                    *data,
                ])
            }
        }

        fn frame_is_u64<N: GraphNode<Frame = u64>>() {}
        frame_is_u64::<ComputeNode<Block>>();
        frame_is_u64::<ComputeNodeWithPush<Block, Block>>();

        let mut tex = vec![TexRunState::new(2), TexRunState::new(2)];
        let phys: Vec<_> = [10, 20]
            .into_iter()
            .map(|base| PhysTex {
                images: (base..base + 2)
                    .map(|slot| PhysImage {
                        sampled: BindlessHandle::from_raw(slot as u64),
                        storage: BindlessHandle::from_raw(slot as u64),
                    })
                    .collect(),
            })
            .collect();
        struct NoBuffers;
        impl mltrs_render_graph::backend::BindingLookup for NoBuffers {
            fn buffer_address(
                &self,
                _: mltrs_render_graph::backend::BufferAddressKind,
                _: usize,
            ) -> u64 {
                panic!("texture-only test must not resolve buffer addresses")
            }
        }
        let uniform = Bindings {
            read: GraphTex(1).read(),
            write: GraphTex(0).write(),
        };
        let push = PushValues::<Block> {
            input: Block::input(
                &73,
                &Bindings {
                    read: GraphTex(0).read(),
                    write: GraphTex(1).write(),
                },
            ),
        };

        for (iteration, expected) in [[10u64, 21, 73], [11, 20, 73], [11, 21, 73]]
            .into_iter()
            .enumerate()
        {
            let resolver = BindingResolver {
                tex: &tex,
                phys: &phys,
                lookup: &NoBuffers,
            };
            // First dispatch writes through both blocks. Subsequent dispatches
            // only write through the push block, as in a repeat with stable uniforms.
            if iteration == 0 {
                assert_eq!(Block::assemble(&42, &uniform, &resolver).0, [20, 11, 42]);
            }
            let payload = push.payload(&resolver).bytes.unwrap();
            let expected_bytes: Vec<_> = expected.into_iter().flat_map(u64::to_ne_bytes).collect();
            assert_eq!(payload.as_slice(), expected_bytes);
            commit_writes(&mut tex, |visit| {
                if iteration == 0 {
                    uniform.visit(visit);
                }
                push.visit_bindings(visit);
            });
            assert_eq!(tex[0].cursor, 1);
            assert_eq!(tex[1].cursor, (iteration as u32 + 1) % 2);
        }
    }

    #[test]
    fn missing_captured_buffer_slots_return_named_errors() {
        let mut graph = super::PreparedRenderGraph::<_, EmptyBackend> {
            nodes: (super::UploadNode::<u32> {
                slot: super::StorageSlot {
                    index: 42,
                    len: 1,
                    _elem: std::marker::PhantomData,
                },
            },),
            uniform_slots: vec![42],
            buffer_slots: vec![],
            tex: vec![],
            phys: vec![],
            _keep_alive: vec![],
        };
        let error = graph
            .validate_buffers(&EmptyBackend)
            .unwrap_err()
            .to_string();
        assert!(error.contains("uniform buffer slot 42"), "{error}");
        graph.uniform_slots.clear();
        for kind in [
            super::desc::BufferKind::Storage,
            super::desc::BufferKind::Immutable,
            super::desc::BufferKind::GpuOnlyFlight,
            super::desc::BufferKind::Singleton,
        ] {
            graph.buffer_slots = vec![super::CapturedBufferSlot {
                kind,
                index: 42,
                name: "captured buffer".into(),
            }];
            let error = graph
                .validate_buffers(&EmptyBackend)
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("captured buffer") && error.contains("slot 42"),
                "{error}"
            );
        }
    }

    #[test]
    fn oversized_upload_returns_error_without_staging_writes() {
        let slot = super::StorageSlot::<u32> {
            index: 42,
            len: 2,
            _elem: std::marker::PhantomData,
        };
        let mut staged = super::StagedWrites::default();
        let error = staged
            .stage_storage(slot, &[1, 2, 3])
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("buffer 42") && error.contains("capacity is 2"),
            "{error}"
        );
        assert!(staged.bytes.is_empty() && staged.targets.is_empty());
        staged.stage_storage(slot, &[]).unwrap();
        staged.stage_storage(slot, &[1, 2]).unwrap();
        assert_eq!(staged.bytes.len(), 8);
    }

    #[test]
    fn staging_preserves_fields_of_a_type_with_interior_padding() {
        #[repr(C)]
        struct Padded {
            small: u8,
            large: u32,
        }
        let source = Padded {
            small: 7,
            large: 123456,
        };
        let mut staged = super::StagedWrites::default();
        let range = staged.stage(std::slice::from_ref(&source));
        // Force growth before copying back, so padding survives arena relocation too.
        staged.stage(&[0u32; 1024]);
        let mut destination = std::mem::MaybeUninit::<Padded>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(
                staged.bytes[range].as_ptr(),
                destination.as_mut_ptr().cast::<std::mem::MaybeUninit<u8>>(),
                std::mem::size_of::<Padded>(),
            );
            let destination = destination.assume_init();
            assert_eq!(destination.small, 7);
            assert_eq!(destination.large, 123456);
        }
    }

    #[test]
    fn texture_names_come_from_the_caller() {
        let mut resources = ResourcePlanner::new();
        resources.texture("height", 8, 8, GraphFormat::R32Float);

        assert_eq!(resources.decls[0].name, "height");
    }

    // ---- GPU-free logical construction --------------------------------

    /// A shader-params fixture for logical tests: two resource bindings and
    /// no data half.
    #[derive(Clone, Copy)]
    struct LogicalBindings {
        first: SampledTexBinding,
        second: StorageTexBinding,
    }

    impl GraphParamBindingSet for LogicalBindings {
        type Pending = PendingParamBindings<Self>;

        fn pending() -> Self::Pending {
            PendingParamBindings::new()
        }
    }

    impl GraphBindingSet for LogicalBindings {
        fn visit(&self, visit: &mut dyn FnMut(GraphBinding)) {
            visit(GraphBinding::SampledTex(self.first));
            visit(GraphBinding::StorageTex(self.second));
        }
    }

    #[repr(C)]
    struct LogicalParams {
        _a: u64,
        _b: u64,
    }

    impl GPUWrite for LogicalParams {}

    impl GraphShaderParams for LogicalParams {
        type Data = ();
        type Bindings = LogicalBindings;
        type Input = LogicalBindings;

        fn input(_data: &(), bindings: &LogicalBindings) -> Self::Input {
            *bindings
        }

        fn assemble_input(_input: &Self::Input, _resolver: &BindingResolver<'_>) -> Self {
            Self { _a: 0, _b: 0 }
        }
    }

    fn logical_dispatch(
        first: SampledTexBinding,
        second: StorageTexBinding,
    ) -> ComputeNode<LogicalParams> {
        let pipeline_key = ComputePipelineKey::<NoPush>::new(0);

        dispatch(
            pipeline_key,
            UniformSlot {
                index: 0,
                _elem: PhantomData,
            },
            [1, 1, 1],
        )
        .with_param_bindings(LogicalBindings { first, second })
    }

    #[test]
    fn logical_valid_without_renderer() {
        let mut resources = ResourcePlanner::new();
        let tex = resources.texture("tex", 8, 8, GraphFormat::R32Float);

        let graph =
            super::RenderGraph::new(resources, (logical_dispatch(tex.read(), tex.write()),))
                .expect("a read+write ping-pong is logically valid");

        // the analysis stays on the logical graph: 2 physical images
        assert_eq!(graph.tex_phys, vec![2]);
        assert_eq!(graph.texture_decls.len(), 1);
    }

    #[test]
    fn logical_invalid_without_renderer() {
        let mut resources = ResourcePlanner::new();
        let ping = resources.texture("ping", 8, 8, GraphFormat::R32Float);

        // one command both reads and mutates the same texture in place
        let error =
            super::RenderGraph::new(resources, (logical_dispatch(ping.read(), ping.mutate()),))
                .err()
                .expect("a mutate combined with a read must fail logical validation")
                .to_string();
        assert!(error.contains("render graph validation failed"), "{error}");
    }

    #[test]
    fn logical_aggregated_callsite_errors() {
        let mut resources = ResourcePlanner::new();
        let tex = resources.texture("tex", 8, 8, GraphFormat::R32Float);
        let other = resources.texture("other", 8, 8, GraphFormat::R32Float);

        // two independent problems: a read_previous with no writer, and a
        // mutate combined with a read of the same texture
        let error = super::RenderGraph::new(
            resources,
            (
                logical_dispatch(tex.read_previous(), other.write()),
                logical_dispatch(tex.read(), tex.mutate()),
            ),
        )
        .err()
        .expect("two independent problems must fail logical validation")
        .to_string();

        let problems = error
            .lines()
            .filter(|line| line.trim_start().starts_with('-'))
            .count();
        assert!(problems >= 2, "expected aggregated problems, got: {error}");
        assert!(error.contains("tex"), "texture name missing: {error}");
    }

    // ---- construction-time release-mode assertions --------------------

    fn indirect_args(len: u32) -> ImmutableSlot<DrawIndexedIndirectCommand> {
        ImmutableSlot {
            index: 0,
            len,
            element_size: std::mem::size_of::<DrawIndexedIndirectCommand>(),
            _elem: PhantomData,
        }
    }

    #[test]
    fn indirect_offsets_use_uploaded_record_size() {
        let args = indirect_args(4);
        let call = indirect_call(args, 2, 1);
        let LowerDrawCall::IndexedIndirect {
            byte_offset,
            request,
            ..
        } = call
        else {
            panic!("expected indirect draw");
        };
        assert_eq!(byte_offset, 40);
        assert_eq!(args.addr_at(2).erased().byte_offset, 40);
        assert_eq!(
            request.element_size(),
            size_of::<DrawIndexedIndirectCommand>()
        );
        assert_eq!(
            request.alignment(),
            align_of::<DrawIndexedIndirectCommand>()
        );
        assert_eq!(request.stride(), 20);
    }

    #[test]
    #[should_panic(expected = "an indirect draw needs at least one command")]
    fn indirect_zero_count_panics() {
        indirect_call(indirect_args(4), 0, 0);
    }

    #[test]
    #[should_panic(expected = "out of bounds")]
    fn indirect_range_panics() {
        indirect_call(indirect_args(4), 2, 3);
    }

    #[test]
    #[should_panic(expected = "out of bounds for buffer of 2 element")]
    fn immutable_addr_at_out_of_bounds_panics() {
        let immutable = ImmutableSlot::<u32> {
            index: 0,
            len: 2,
            element_size: std::mem::size_of::<u32>(),
            _elem: PhantomData,
        };
        immutable.addr_at(2);
    }

    #[test]
    #[should_panic(expected = "out of bounds for buffer of 2 element")]
    fn singleton_addr_at_out_of_bounds_panics() {
        let singleton = SingletonSlot::<u32> {
            index: 0,
            len: 2,
            _elem: PhantomData,
        };
        singleton.addr_at(2);
    }

    #[test]
    fn addr_at_last_valid_index_succeeds() {
        let immutable = ImmutableSlot::<u32> {
            index: 0,
            len: 2,
            element_size: std::mem::size_of::<u32>(),
            _elem: PhantomData,
        };
        let binding = immutable.addr_at(1);
        assert_eq!(
            binding.erased().byte_offset,
            std::mem::size_of::<u32>() as u64
        );

        let singleton = SingletonSlot::<u32> {
            index: 0,
            len: 2,
            _elem: PhantomData,
        };
        assert_eq!(
            singleton.addr_at(1).erased().byte_offset,
            std::mem::size_of::<u32>() as u64
        );
    }

    // ---- immutable/singleton buffers as read-only shader fields ----------

    #[test]
    fn immutable_binding_converts_to_read_binding_keeping_raw() {
        // A shader field typed `ReadAddr<T>` can be fed from an immutable or
        // singleton buffer: the conversion must preserve the raw kind, index,
        // and byte offset so lowering and resolution stay on the immutable or
        // singleton path rather than reinterpreting the buffer as storage.
        fn assert_raw_equal(read: RawBufferBinding, expected: RawBufferBinding) {
            assert_eq!(read.kind, expected.kind);
            assert_eq!(read.index, expected.index);
            assert_eq!(read.byte_offset, expected.byte_offset);
        }

        let immutable = ImmutableSlot::<u32> {
            index: 3,
            len: 4,
            element_size: std::mem::size_of::<u32>(),
            _elem: PhantomData,
        };
        let read: ReadBufferBinding<u32> = immutable.addr_at(1).into();
        assert_raw_equal(read.erased(), immutable.addr_at(1).erased());

        let singleton = SingletonSlot::<u32> {
            index: 5,
            len: 4,
            _elem: PhantomData,
        };
        let read: ReadBufferBinding<u32> = singleton.addr().into();
        assert_raw_equal(read.erased(), singleton.addr().erased());
    }

    // ---- slot Copy/Clone without element bounds -----------------------

    // The Clone calls are deliberate: this test exercises the manual
    // Clone/Copy impls that omit element bounds.
    #[test]
    #[allow(clippy::clone_on_copy)]
    fn slot_copy_non_copy_element() {
        struct NotCopy;

        fn copied<T>(slot: UniformSlot<T>) -> (usize, usize) {
            let first = slot.index;
            let clone = slot;
            (first, clone.index)
        }

        let uniform = UniformSlot::<NotCopy> {
            index: 1,
            _elem: PhantomData,
        };
        assert_eq!(copied(uniform), (1, 1));

        let storage = StorageSlot::<NotCopy> {
            index: 2,
            len: 3,
            _elem: PhantomData,
        };
        let storage_copy = storage;
        assert_eq!((storage.index, storage_copy.len), (2, 3));

        let gpu_only = GpuOnlySlot::<NotCopy> {
            index: 4,
            _elem: PhantomData,
        };
        let gpu_only_copy = gpu_only.clone();
        assert_eq!(gpu_only.index, gpu_only_copy.index);

        let immutable = ImmutableSlot::<NotCopy> {
            index: 5,
            len: 6,
            element_size: std::mem::size_of::<NotCopy>(),
            _elem: PhantomData,
        };
        assert_eq!(immutable.clone().len, 6);

        let singleton = SingletonSlot::<NotCopy> {
            index: 7,
            len: 8,
            _elem: PhantomData,
        };
        let (a, b) = (singleton, singleton);
        assert_eq!((a.index, b.len), (7, 8));
    }
}
