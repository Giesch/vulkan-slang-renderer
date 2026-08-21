use std::marker::PhantomData;

use ash::vk;

use crate::shaders::atlas::{ComputeShaderAtlasEntry, ShaderAtlasEntry};

use super::gpu_write::PushConstantBlock;
use super::vertex_description::{NoVertex, VertexDescription};
use super::{
    ComputeShaderPipelineLayout, RawUniformBufferHandle, ShaderPipelineLayout,
    StorageTextureHandle, TextureHandle,
};

/// A newtype-wrapped index into one of the renderer's pipeline/mesh storages.
/// Distinct types per storage make cross-storage index mixups a compile error.
/// The indexes are opaque to callers; only the renderer mints and resolves them.
pub trait PipelineIndex: Copy {
    #[doc(hidden)]
    fn from_raw(index: usize) -> Self;
    #[doc(hidden)]
    fn raw(self) -> usize;
}

/// Index into `PipelineStorage` (graphics pipelines, shared by DrawIndexed,
/// DrawVertexCount, and picking pipelines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphicsPipelineIndex(usize);

/// Index into `ComputePipelineStorage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComputePipelineIndex(usize);

/// Index into `Renderer::meshes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshIndex(usize);

impl PipelineIndex for GraphicsPipelineIndex {
    fn from_raw(index: usize) -> Self {
        Self(index)
    }
    fn raw(self) -> usize {
        self.0
    }
}

impl PipelineIndex for ComputePipelineIndex {
    fn from_raw(index: usize) -> Self {
        Self(index)
    }
    fn raw(self) -> usize {
        self.0
    }
}

impl MeshIndex {
    pub(super) fn from_raw(index: usize) -> Self {
        Self(index)
    }
    pub(super) fn raw(self) -> usize {
        self.0
    }
}

/// A marker trait for different draw call types
pub trait DrawCall {
    /// The typed index into the storage this draw-call kind lives in.
    type Index: PipelineIndex;
}

/// A marker that the pipeline uses basic cmd_draw draw calls,
/// passing a vertex count with no other vertex data
#[derive(Debug)]
pub struct DrawVertexCount;
impl DrawCall for DrawVertexCount {
    type Index = GraphicsPipelineIndex;
}

/// A marker that the pipeline uses cmd_draw_indexed draw calls,
/// using pre-allocated vertex and index buffers
#[derive(Debug)]
pub struct DrawIndexed;
impl DrawCall for DrawIndexed {
    type Index = GraphicsPipelineIndex;
}

/// A marker for compute pipelines
#[derive(Debug)]
pub struct Compute;
impl DrawCall for Compute {
    type Index = ComputePipelineIndex;
}

/// A pipeline whose shader declares no `[[vk::push_constant]]` block.
/// This is the default.
#[derive(Debug)]
pub struct NoPush;

/// A pipeline whose shader declares `P` as its push constant block.
#[derive(Debug)]
pub struct PushBlock<P: PushConstantBlock>(PhantomData<P>);

/// `P` is the *push slot* — [`NoPush`] or [`PushBlock<B>`] — not the block type
/// itself. It is erased at the storage boundary: `GraphicsPipelineIndex` stays
/// untyped and only the handle carries it.
pub struct PipelineHandle<T, P = NoPush> {
    index: usize,
    _phantom_data: PhantomData<(T, P)>,
}

// not derived so we don't require `P: Debug`
impl<T, P> std::fmt::Debug for PipelineHandle<T, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipelineHandle")
            .field("index", &self.index)
            .finish()
    }
}

impl<T: DrawCall, P> PipelineHandle<T, P> {
    pub(crate) fn index(&self) -> T::Index {
        T::Index::from_raw(self.index)
    }
}

/// Distinct from PipelineHandle<T> — compile-time prevents misuse with main draw calls
#[derive(Debug)]
pub struct PickingPipelineHandle {
    pub(super) index: GraphicsPipelineIndex,
}

pub(super) struct PipelineStorage(Vec<Option<RendererPipeline>>);

impl PipelineStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<T: DrawCall, P>(&mut self, pipeline: RendererPipeline) -> PipelineHandle<T, P> {
        let handle = PipelineHandle {
            index: self.0.len(),
            _phantom_data: PhantomData,
        };

        self.0.push(Some(pipeline));

        handle
    }

    pub fn add_picking(&mut self, pipeline: RendererPipeline) -> PickingPipelineHandle {
        let handle = PickingPipelineHandle {
            index: GraphicsPipelineIndex::from_raw(self.0.len()),
        };

        self.0.push(Some(pipeline));

        handle
    }

    pub fn get<T, P>(&self, handle: &PipelineHandle<T, P>) -> &RendererPipeline {
        self.0[handle.index].as_ref().unwrap()
    }

    pub fn get_picking(&self, handle: &PickingPipelineHandle) -> &RendererPipeline {
        self.0[handle.index.raw()].as_ref().unwrap()
    }

    pub fn get_by_index(&self, index: GraphicsPipelineIndex) -> &RendererPipeline {
        self.0[index.raw()].as_ref().unwrap()
    }

    #[cfg(debug_assertions)] // used only during hot reload
    pub fn get_mut_by_index(&mut self, index: GraphicsPipelineIndex) -> &mut RendererPipeline {
        self.0[index.raw()].as_mut().unwrap()
    }

    #[expect(unused)]
    pub fn take<T, P>(&mut self, handle: PipelineHandle<T, P>) -> RendererPipeline {
        self.0[handle.index].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<RendererPipeline> {
        self.0.iter_mut().filter_map(|o| o.take()).collect()
    }
}

pub(super) struct RendererPipeline {
    pub layout: ShaderPipelineLayout,
    pub pipeline: vk::Pipeline,

    pub vertex_pipeline_config: VertexPipelineConfig,

    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,

    pub shader: Box<dyn ShaderAtlasEntry>,

    #[cfg_attr(not(debug_assertions), expect(unused))] // used only during hot reload
    pub raster_state: RasterState,
}

/// How fragments are combined with what is already in the color attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// SRC_ALPHA / ONE_MINUS_SRC_ALPHA with BlendOp::ADD, for color and alpha
    Alpha,
    /// DST_ALPHA / ONE_MINUS_DST_ALPHA with BlendOp::ADD, for color and alpha —
    /// GX's `GX_BL_DSTALPHA` / `GX_BL_INVDSTALPHA`. GX applies the blend
    /// expression to alpha as well as color, so both pairs match.
    ///
    /// Only meaningful when something earlier in the *same* render pass has
    /// written destination alpha. The color attachment is cleared to alpha 1.0,
    /// so against an untouched framebuffer this reduces to a plain source
    /// write; pair it with a `color_write`-masked pass that deposits the alpha
    /// first. See `llm_notes/link_rendering/phase_09_eyes.md`.
    DstAlpha,
    /// blending disabled; the fragment's alpha is ignored
    Opaque,
}

/// Which triangle facing is discarded. The front face is always
/// counter-clockwise; `Front` exists mainly as a test affordance, since it
/// renders a closed mesh inside-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CullMode {
    Back,
    Front,
    None,
}

/// The depth test's comparison, or no depth test at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepthCompare {
    Less,
    LessEqual,
    Always,
    /// No depth test. NOTE that Vulkan still honors depth writes when the test
    /// is disabled, so `Disabled` with `depth_write: true` writes the depth
    /// buffer unconditionally — pair it with `depth_write: false` unless that
    /// is really what you want.
    Disabled,
}

/// The fixed-function raster state a graphics pipeline is baked with.
/// [`RasterState::default()`] reproduces the renderer's original hardcoded
/// pipeline exactly, so leaving it alone is always a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RasterState {
    pub blend: BlendMode,
    pub cull: CullMode,
    pub depth_test: DepthCompare,
    pub depth_write: bool,
    /// per-channel color write mask, in RGBA order
    pub color_write: [bool; 4],
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            blend: BlendMode::Alpha,
            cull: CullMode::Back,
            depth_test: DepthCompare::Less,
            depth_write: true,
            color_write: [true; 4],
        }
    }
}

impl RasterState {
    /// No depth test and no depth writes. Vulkan honors depth writes even with
    /// the test disabled, so these two belong together — see
    /// [`DepthCompare::Disabled`].
    pub fn no_depth() -> Self {
        Self {
            depth_test: DepthCompare::Disabled,
            depth_write: false,
            ..Default::default()
        }
    }
}

pub(super) enum VertexPipelineConfig {
    VertexAndIndexBuffers(VertexAndIndexBuffers),
    /// index into Renderer::meshes; the buffers outlive this pipeline
    SharedMesh(MeshIndex),
    VertexCount, // this count is now passed in every time
}

/// A handle to a mesh created with Renderer::create_mesh, whose vertex and
/// index buffers can be shared by multiple pipelines via
/// PipelineConfig::with_shared_mesh. The vertex type parameter ties the mesh
/// to pipelines with a matching vertex layout at compile time.
#[derive(Debug)]
pub struct MeshHandle<V: VertexDescription> {
    pub(super) index: MeshIndex,
    pub(super) _phantom_data: PhantomData<V>,
}

pub(super) struct VertexAndIndexBuffers {
    pub(super) vertex_buffer: vk::Buffer,
    pub(super) vertex_buffer_memory: vk_mem::Allocation,

    pub(super) index_buffer: vk::Buffer,
    pub(super) index_buffer_memory: vk_mem::Allocation,

    pub(super) index_count: u32,
}

pub struct PipelineConfig<'t, V: VertexDescription, D: DrawCall, P = NoPush> {
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    pub(super) vertex_config: VertexConfig<V>,
    _draw_call: PhantomData<D>,
    _push: PhantomData<P>,
    pub(super) texture_handles: Vec<&'t TextureHandle>,
    pub(super) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(super) storage_texture_handles: Vec<&'t StorageTextureHandle>,
    pub(super) raster_state: RasterState,
}

/// which type of draw call to use, and the necessary data for it. Every variant
/// is a fully specified vertex source: an indexed shader's config cannot exist
/// without one (see [`IndexedPipelineConfig`]), so there is no "unset" state.
pub(super) enum VertexConfig<V> {
    // use a cmd_draw_indexed call, with prepared vertex and index buffers,
    // and an associated Vertex type
    VertexAndIndexBuffers(Vec<V>, Vec<u32>),
    // use cmd_draw_indexed calls against a shared mesh created with
    // Renderer::create_mesh (the index is into Renderer::meshes)
    SharedMesh(MeshIndex),
    // use a basic cmd_draw call passing a vertex count, with no vertex or index buffers,
    // and so no Vertex type
    VertexCount,
}

/// The config for an indexed shader that has not been given a vertex source
/// yet. Generated `pipeline_config()` returns this rather than a
/// [`PipelineConfig`]; [`Self::with_vertices`] and [`Self::with_shared_mesh`]
/// are the only ways to reach a `PipelineConfig`, and
/// `Renderer::create_pipeline` accepts nothing else. That makes "indexed
/// pipeline with no vertex data" unrepresentable instead of a runtime error.
pub struct IndexedPipelineConfig<'t, V: VertexDescription, P = NoPush> {
    shader: Box<dyn ShaderAtlasEntry>,
    texture_handles: Vec<&'t TextureHandle>,
    uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    storage_texture_handles: Vec<&'t StorageTextureHandle>,
    raster_state: RasterState,
    _vertex: PhantomData<V>,
    _push: PhantomData<P>,
}

impl<'t, V: VertexDescription, P> IndexedPipelineConfig<'t, V, P> {
    /// Draw from vertex and index buffers owned by this pipeline.
    pub fn with_vertices(
        self,
        vertices: Vec<V>,
        indices: Vec<u32>,
    ) -> PipelineConfig<'t, V, DrawIndexed, P> {
        self.into_config(VertexConfig::VertexAndIndexBuffers(vertices, indices))
    }

    /// Draw from a shared mesh instead of per-pipeline vertex/index buffers.
    pub fn with_shared_mesh(self, mesh: &MeshHandle<V>) -> PipelineConfig<'t, V, DrawIndexed, P> {
        self.into_config(VertexConfig::SharedMesh(mesh.index))
    }

    /// Bake this pipeline with explicit fixed-function raster state. Callable
    /// either side of the vertex source.
    pub fn with_raster_state(mut self, raster_state: RasterState) -> Self {
        self.raster_state = raster_state;
        self
    }

    fn into_config(self, vertex_config: VertexConfig<V>) -> PipelineConfig<'t, V, DrawIndexed, P> {
        PipelineConfig {
            shader: self.shader,
            vertex_config,
            _draw_call: PhantomData,
            _push: PhantomData,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_texture_handles: self.storage_texture_handles,
            raster_state: self.raster_state,
        }
    }
}

impl<'t, V: VertexDescription, D: DrawCall, P> PipelineConfig<'t, V, D, P> {
    /// Bake this pipeline with explicit fixed-function raster state instead of
    /// [`RasterState::default()`] (which reproduces the renderer's original
    /// hardcoded pipeline).
    pub fn with_raster_state(mut self, raster_state: RasterState) -> Self {
        self.raster_state = raster_state;
        self
    }
}

/// The fields every graphics pipeline config shares. Which terminal method is
/// called — [`Self::build_indexed`] or [`Self::build_vertex_count`] — decides
/// both the vertex type and the draw-call kind, so the two cannot disagree.
pub struct PipelineConfigBuilder<'t> {
    pub shader: Box<dyn ShaderAtlasEntry>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'t StorageTextureHandle>,
}

impl<'t> PipelineConfigBuilder<'t> {
    pub fn build_indexed<V: VertexDescription, P>(self) -> IndexedPipelineConfig<'t, V, P> {
        IndexedPipelineConfig {
            shader: self.shader,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_texture_handles: self.storage_texture_handles,
            raster_state: RasterState::default(),
            _vertex: PhantomData,
            _push: PhantomData,
        }
    }

    /// Terminal call for a shader with no vertex input. The
    /// vertex-type/draw-call pairing is a signature guarantee here rather than
    /// a codegen convention.
    pub fn build_vertex_count<P>(self) -> PipelineConfig<'t, NoVertex, DrawVertexCount, P> {
        PipelineConfig {
            shader: self.shader,
            vertex_config: VertexConfig::VertexCount,
            _draw_call: PhantomData,
            _push: PhantomData,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_texture_handles: self.storage_texture_handles,
            raster_state: RasterState::default(),
        }
    }
}

// --- Compute pipeline types ---

pub(super) struct ComputeRendererPipeline {
    pub layout: ComputeShaderPipelineLayout,
    pub pipeline: vk::Pipeline,
    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,
    pub shader: Box<dyn ComputeShaderAtlasEntry>,
}

pub(super) struct ComputePipelineStorage(Vec<Option<ComputeRendererPipeline>>);

impl ComputePipelineStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub fn add<P>(&mut self, pipeline: ComputeRendererPipeline) -> PipelineHandle<Compute, P> {
        let handle = PipelineHandle {
            index: self.0.len(),
            _phantom_data: PhantomData,
        };

        self.0.push(Some(pipeline));

        handle
    }

    #[cfg(debug_assertions)]
    #[expect(unused)]
    pub fn get_mut<P>(
        &mut self,
        handle: &PipelineHandle<Compute, P>,
    ) -> &mut ComputeRendererPipeline {
        self.0[handle.index].as_mut().unwrap()
    }

    pub fn get_by_index(&self, index: ComputePipelineIndex) -> &ComputeRendererPipeline {
        self.0[index.raw()].as_ref().unwrap()
    }

    #[cfg(debug_assertions)]
    pub fn get_mut_by_index(
        &mut self,
        index: ComputePipelineIndex,
    ) -> &mut ComputeRendererPipeline {
        self.0[index.raw()].as_mut().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<ComputeRendererPipeline> {
        self.0.iter_mut().filter_map(|o| o.take()).collect()
    }
}

pub struct ComputePipelineConfig<'t, P = NoPush> {
    pub(super) shader: Box<dyn ComputeShaderAtlasEntry>,
    pub(super) texture_handles: Vec<&'t TextureHandle>,
    pub(super) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(super) storage_texture_handles: Vec<&'t StorageTextureHandle>,
    _push: PhantomData<P>,
}

// fields are pub because generated compute atlas entries construct this directly
pub struct ComputePipelineConfigBuilder<'t> {
    pub shader: Box<dyn ComputeShaderAtlasEntry>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'t StorageTextureHandle>,
}

impl<'t> ComputePipelineConfigBuilder<'t> {
    pub fn build<P>(self) -> ComputePipelineConfig<'t, P> {
        ComputePipelineConfig {
            shader: self.shader,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_texture_handles: self.storage_texture_handles,
            _push: PhantomData,
        }
    }
}
