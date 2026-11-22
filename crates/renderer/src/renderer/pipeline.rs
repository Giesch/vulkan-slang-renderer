use std::marker::PhantomData;

use ash::vk;

use crate::shaders::atlas::{ComputeShaderAtlasEntry, ShaderAtlasEntry};

use super::gpu_write::{GPUWrite, PushConstantBlock};
pub use super::vertex_description::{
    IndexedVertexInterface, NoVertex, RuntimeVertex, TypedVertex, VertexDescription,
};
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

/// A marker that the pipeline uses cmd_draw_indexed_indirect draw calls,
/// reading draw arguments from a buffer
#[derive(Debug)]
pub struct DrawIndexedIndirect;
impl DrawCall for DrawIndexedIndirect {
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

/// A graphics pipeline created by `Renderer::create_pipeline`.
///
/// `I` is the pipeline's vertex interface — [`TypedVertex<V>`],
/// [`RuntimeVertex`], or [`NoVertex`] — fixed by the config constructor that
/// produced it. `D` is the draw-call kind and `P` the push slot, as on
/// [`PipelineHandle`]. Only the renderer mints these; the raw handle is
/// private, so a handle cannot be built or relabeled with another interface.
pub struct GraphicsPipelineHandle<I, D, P = NoPush> {
    raw: PipelineHandle<D, P>,
    _interface: PhantomData<fn() -> I>,
}

// not derived so we don't require `I: Debug` or `P: Debug`
impl<I, D, P> std::fmt::Debug for GraphicsPipelineHandle<I, D, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphicsPipelineHandle")
            .field("index", &self.raw.index)
            .finish()
    }
}

impl<I, D: DrawCall, P> GraphicsPipelineHandle<I, D, P> {
    pub(super) fn new(raw: PipelineHandle<D, P>) -> Self {
        Self {
            raw,
            _interface: PhantomData,
        }
    }

    /// the untyped-interface handle the renderer's storage and draw queue use
    pub(super) fn raw(&self) -> &PipelineHandle<D, P> {
        &self.raw
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

/// A token that means the depth buffer has a stencil component.
/// Required for constructing an enabled [`StencilMode`].
///
/// Returned by `Renderer::stencil_support` if the game opted in
/// with `Game::needs_stencil`.
#[derive(Debug, Clone, Copy)]
pub struct StencilSupport(pub(crate) ());

impl StencilSupport {
    /// Every fragment that passes the depth test writes `reference` to the stencil.
    pub fn write(self, reference: u8) -> StencilMode {
        StencilMode(StencilModeKind::Write { reference })
    }

    /// Draw only where the stencil buffer equals `reference`.
    /// Does not write to the stencil buffer.
    pub fn test_equal(self, reference: u8) -> StencilMode {
        StencilMode(StencilModeKind::TestEqual { reference })
    }
}

/// The stencil test and write behavior a graphics pipeline is baked with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StencilMode(pub(crate) StencilModeKind);

impl StencilMode {
    /// No stencil test, no stencil writes.
    pub const DISABLED: Self = Self(StencilModeKind::Disabled);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StencilModeKind {
    Disabled,
    Write { reference: u8 },
    TestEqual { reference: u8 },
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
    pub stencil: StencilMode,
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            blend: BlendMode::Alpha,
            cull: CullMode::Back,
            depth_test: DepthCompare::Less,
            depth_write: true,
            color_write: [true; 4],
            stencil: StencilMode::DISABLED,
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

/// A complete graphics pipeline config. `I` is the vertex interface
/// ([`TypedVertex<V>`], [`RuntimeVertex`], or [`NoVertex`]) and is carried
/// through to the [`GraphicsPipelineHandle`] that `Renderer::create_pipeline`
/// returns. The vertex data itself is stored type-erased.
pub struct PipelineConfig<'t, I, D: DrawCall, P = NoPush> {
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    pub(super) vertex_config: VertexConfig,
    /// the layout a typed interface claims, checked against the shader entry
    /// before allocation; `None` for runtime and vertex-count interfaces
    pub(super) typed_vertex_layout: Option<TypedVertexLayout>,
    _interface: PhantomData<fn() -> I>,
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
pub(super) enum VertexConfig {
    // use a cmd_draw_indexed call, with prepared vertex and index buffers,
    // uploaded from typed vertices owned by the upload object
    VertexAndIndexBuffers(Box<dyn TypedVertexUpload>, Vec<u32>),
    // use a cmd_draw_indexed call, with vertex bytes already packed to the
    // shader entry's vertex layout by the caller of
    // PipelineConfigBuilder::build_indexed_bytes
    PackedBytes(Vec<u8>, Vec<u32>),
    // use cmd_draw_indexed calls against a shared mesh created with
    // Renderer::create_mesh (the index is into Renderer::meshes)
    SharedMesh(MeshIndex),
    // use a basic cmd_draw call passing a vertex count, with no vertex or index buffers,
    // and so no Vertex type
    VertexCount,
}

/// Owned typed vertices awaiting upload, with their element type erased so
/// the surrounding config needs no upload bound. The implementation keeps
/// the original `Vec<V>` and copies it as `V`s, never as reinterpreted bytes.
pub(super) trait TypedVertexUpload {
    fn is_empty(&self) -> bool;

    fn upload(
        &self,
        allocator: &vk_mem::Allocator,
        device: &ash::Device,
        command_pool: vk::CommandPool,
        graphics_queue: vk::Queue,
    ) -> anyhow::Result<(vk::Buffer, vk_mem::Allocation)>;
}

struct OwnedVertices<V>(Vec<V>);

impl<V: GPUWrite> TypedVertexUpload for OwnedVertices<V> {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn upload(
        &self,
        allocator: &vk_mem::Allocator,
        device: &ash::Device,
        command_pool: vk::CommandPool,
        graphics_queue: vk::Queue,
    ) -> anyhow::Result<(vk::Buffer, vk_mem::Allocation)> {
        super::create_vertex_buffer(allocator, device, command_pool, graphics_queue, &self.0)
    }
}

/// The binding and attribute descriptions of a typed interface's vertex type,
/// captured when the typed config is completed.
pub(super) struct TypedVertexLayout {
    type_name: &'static str,
    bindings: Vec<vk::VertexInputBindingDescription>,
    attributes: Vec<vk::VertexInputAttributeDescription>,
}

impl TypedVertexLayout {
    fn of<V: VertexDescription>() -> Self {
        Self {
            type_name: std::any::type_name::<V>(),
            bindings: V::binding_descriptions(),
            attributes: V::attribute_descriptions(),
        }
    }
}

type BindingKey = (u32, u32, vk::VertexInputRate);
type AttributeKey = (u32, u32, vk::Format, u32);

fn binding_keys(bindings: &[vk::VertexInputBindingDescription]) -> Vec<BindingKey> {
    let mut keys: Vec<_> = bindings
        .iter()
        .map(|b| (b.binding, b.stride, b.input_rate))
        .collect();
    keys.sort_by_key(|&(binding, stride, rate)| (binding, stride, rate.as_raw()));

    keys
}

fn attribute_keys(attributes: &[vk::VertexInputAttributeDescription]) -> Vec<AttributeKey> {
    let mut keys: Vec<_> = attributes
        .iter()
        .map(|a| (a.location, a.binding, a.format, a.offset))
        .collect();
    keys.sort_by_key(|&(location, binding, format, offset)| {
        (location, binding, format.as_raw(), offset)
    });

    keys
}

impl<I, D: DrawCall, P> PipelineConfig<'_, I, D, P> {
    /// Checks the vertex source against the shader entry's vertex layout.
    /// Runs before any GPU allocation, so a rejected config leaks nothing.
    pub(super) fn validate_vertex_input(&self) -> anyhow::Result<()> {
        let shader_name = self.shader.source_file_name();
        let shader_bindings = self.shader.vertex_binding_descriptions();

        if let Some(layout) = &self.typed_vertex_layout {
            let shader_attributes = self.shader.vertex_attribute_descriptions();
            anyhow::ensure!(
                binding_keys(&layout.bindings) == binding_keys(&shader_bindings),
                "pipeline for {shader_name} was built for vertex type {} whose bindings \
                 (binding, stride, input rate) {:?} differ from the shader's {:?}",
                layout.type_name,
                binding_keys(&layout.bindings),
                binding_keys(&shader_bindings),
            );
            anyhow::ensure!(
                attribute_keys(&layout.attributes) == attribute_keys(&shader_attributes),
                "pipeline for {shader_name} was built for vertex type {} whose attributes \
                 (location, binding, format, offset) {:?} differ from the shader's {:?}",
                layout.type_name,
                attribute_keys(&layout.attributes),
                attribute_keys(&shader_attributes),
            );
        }

        match &self.vertex_config {
            VertexConfig::VertexAndIndexBuffers(vertices, indices) => {
                anyhow::ensure!(
                    !vertices.is_empty() && !indices.is_empty(),
                    "pipeline for {shader_name} was given empty vertex data by `.with_vertices()`",
                );
            }
            VertexConfig::PackedBytes(bytes, indices) => {
                let stride = shader_bindings
                    .first()
                    .map(|binding| binding.stride as usize)
                    .unwrap_or(0);
                let whole_vertices =
                    stride > 0 && !bytes.is_empty() && bytes.len().is_multiple_of(stride);
                anyhow::ensure!(
                    whole_vertices,
                    "pipeline for {shader_name} was given {} vertex bytes, not a nonzero \
                     multiple of the vertex stride {stride}",
                    bytes.len(),
                );
                anyhow::ensure!(
                    !indices.is_empty(),
                    "pipeline for {shader_name} was given empty index data by \
                     `.build_indexed_bytes()`",
                );
            }
            VertexConfig::SharedMesh(_) => {}
            VertexConfig::VertexCount => {
                anyhow::ensure!(
                    shader_bindings.is_empty(),
                    "pipeline for {shader_name} draws with a vertex count, but its shader \
                     declares vertex input bindings {:?}",
                    binding_keys(&shader_bindings),
                );
            }
        }

        Ok(())
    }
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
    ) -> PipelineConfig<'t, TypedVertex<V>, DrawIndexed, P>
    where
        V: 'static,
    {
        self.into_config(VertexConfig::VertexAndIndexBuffers(
            Box::new(OwnedVertices(vertices)),
            indices,
        ))
    }

    /// Draw from a shared mesh instead of per-pipeline vertex/index buffers.
    pub fn with_shared_mesh(
        self,
        mesh: &MeshHandle<V>,
    ) -> PipelineConfig<'t, TypedVertex<V>, DrawIndexed, P> {
        self.into_config(VertexConfig::SharedMesh(mesh.index))
    }

    /// Bake this pipeline with explicit fixed-function raster state. Callable
    /// either side of the vertex source.
    pub fn with_raster_state(mut self, raster_state: RasterState) -> Self {
        self.raster_state = raster_state;
        self
    }

    fn into_config(
        self,
        vertex_config: VertexConfig,
    ) -> PipelineConfig<'t, TypedVertex<V>, DrawIndexed, P> {
        PipelineConfig {
            shader: self.shader,
            vertex_config,
            typed_vertex_layout: Some(TypedVertexLayout::of::<V>()),
            _interface: PhantomData,
            _draw_call: PhantomData,
            _push: PhantomData,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_texture_handles: self.storage_texture_handles,
            raster_state: self.raster_state,
        }
    }
}

impl<'t, I, P> PipelineConfig<'t, I, DrawIndexed, P> {
    /// Draw with cmd_draw_indexed_indirect instead of cmd_draw_indexed.
    /// An indirect pipeline's shader must read `SV_DrawIndex`.
    pub fn indirect(self) -> PipelineConfig<'t, I, DrawIndexedIndirect, P> {
        PipelineConfig {
            shader: self.shader,
            vertex_config: self.vertex_config,
            typed_vertex_layout: self.typed_vertex_layout,
            _interface: PhantomData,
            _draw_call: PhantomData,
            _push: PhantomData,
            texture_handles: self.texture_handles,
            uniform_buffer_handles: self.uniform_buffer_handles,
            storage_texture_handles: self.storage_texture_handles,
            raster_state: self.raster_state,
        }
    }
}

impl<'t, I, D: DrawCall, P> PipelineConfig<'t, I, D, P> {
    /// Bake this pipeline with explicit fixed-function raster state instead of
    /// [`RasterState::default()`] (which reproduces the renderer's original
    /// hardcoded pipeline).
    pub fn with_raster_state(mut self, raster_state: RasterState) -> Self {
        self.raster_state = raster_state;
        self
    }
}

/// The fields every graphics pipeline config shares. Which terminal method is
/// called — [`Self::build_indexed`], [`Self::build_indexed_bytes`], or
/// [`Self::build_vertex_count`] — decides both the vertex interface and the
/// draw-call kind, so the two cannot disagree.
pub struct PipelineConfigBuilder<'t> {
    pub shader: Box<dyn ShaderAtlasEntry>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'t StorageTextureHandle>,
}

impl<'t> PipelineConfigBuilder<'t> {
    /// Terminal call for a shader whose vertex input is the vertex type `V`.
    /// `V`'s layout is checked against the shader entry's when the pipeline
    /// is created, before any GPU allocation.
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
        self.into_config(VertexConfig::VertexCount)
    }

    /// Terminal call for an indexed shader fed with vertex bytes already
    /// packed to the shader entry's vertex layout, with no Rust vertex type.
    /// The byte count must be a nonzero multiple of that layout's stride.
    pub fn build_indexed_bytes<P>(
        self,
        vertex_bytes: Vec<u8>,
        indices: Vec<u32>,
    ) -> PipelineConfig<'t, RuntimeVertex, DrawIndexed, P> {
        self.into_config(VertexConfig::PackedBytes(vertex_bytes, indices))
    }

    fn into_config<I, D: DrawCall, P>(
        self,
        vertex_config: VertexConfig,
    ) -> PipelineConfig<'t, I, D, P> {
        PipelineConfig {
            shader: self.shader,
            vertex_config,
            typed_vertex_layout: None,
            _interface: PhantomData,
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

#[cfg(test)]
mod tests {
    //! `validate_vertex_input` runs at the top of `Renderer::init_pipeline`
    //! and `create_picking_pipeline`, before anything is allocated, so these
    //! configs exercise the pre-allocation checks with no GPU.

    use super::*;
    use crate::renderer::render_graph::GPUWrite as GraphGPUWrite;
    use crate::shaders::runtime::RuntimeShader;

    /// reflection with one 32-byte vertex: float3 at 0, float3 at 12
    const VERTEX_INPUT_REFLECTION: &str =
        include_str!("../../../slang-reflection/src/fixtures/basic_triangle.json");
    /// reflection whose vertex stage reads only `SV_VertexID`
    const NO_VERTEX_INPUT_REFLECTION: &str =
        include_str!("../../../slang-reflection/src/fixtures/koch_curve.json");

    fn spv_module() -> Vec<u8> {
        // magic number, version, generator, bound, schema
        [0x0723_0203u32, 0x0001_0000, 0, 1, 0]
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .collect()
    }

    fn builder(reflection: &str) -> PipelineConfigBuilder<'static> {
        let module = spv_module();

        PipelineConfigBuilder {
            shader: Box::new(RuntimeShader::new(reflection, &module, &module).unwrap()),
            texture_handles: vec![],
            uniform_buffer_handles: vec![],
            storage_texture_handles: vec![],
        }
    }

    fn binding(stride: u32) -> Vec<vk::VertexInputBindingDescription> {
        vec![
            vk::VertexInputBindingDescription::default()
                .binding(0)
                .stride(stride)
                .input_rate(vk::VertexInputRate::VERTEX),
        ]
    }

    fn attribute(location: u32, offset: u32) -> vk::VertexInputAttributeDescription {
        vk::VertexInputAttributeDescription::default()
            .binding(0)
            .location(location)
            .offset(offset)
            .format(vk::Format::R32G32B32_SFLOAT)
    }

    /// matches the reflected layout exactly
    #[derive(Clone, Copy)]
    struct MatchingVertex(#[expect(dead_code)] [f32; 8]);

    impl GraphGPUWrite for MatchingVertex {}

    impl VertexDescription for MatchingVertex {
        fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
            binding(32)
        }

        fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
            vec![attribute(0, 0), attribute(1, 12)]
        }
    }

    /// matches the reflected layout, with its attributes listed in reverse order
    #[derive(Clone, Copy)]
    struct ReorderedVertex(#[expect(dead_code)] [f32; 8]);

    impl GraphGPUWrite for ReorderedVertex {}

    impl VertexDescription for ReorderedVertex {
        fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
            binding(32)
        }

        fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
            vec![attribute(1, 12), attribute(0, 0)]
        }
    }

    /// same stride as the reflected layout, but the second attribute moved
    #[derive(Clone, Copy)]
    struct WrongOffsetVertex(#[expect(dead_code)] [f32; 8]);

    impl GraphGPUWrite for WrongOffsetVertex {}

    impl VertexDescription for WrongOffsetVertex {
        fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
            binding(32)
        }

        fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
            vec![attribute(0, 0), attribute(1, 16)]
        }
    }

    /// same stride and offsets as the reflected layout, but the second
    /// attribute is a float4 instead of a float3
    #[derive(Clone, Copy)]
    struct WrongFormatVertex(#[expect(dead_code)] [f32; 8]);

    impl GraphGPUWrite for WrongFormatVertex {}

    impl VertexDescription for WrongFormatVertex {
        fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
            binding(32)
        }

        fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
            vec![
                attribute(0, 0),
                attribute(1, 12).format(vk::Format::R32G32B32A32_SFLOAT),
            ]
        }
    }

    /// a different stride from the reflected layout
    #[derive(Clone, Copy)]
    struct WrongStrideVertex(#[expect(dead_code)] [f32; 6]);

    impl GraphGPUWrite for WrongStrideVertex {}

    impl VertexDescription for WrongStrideVertex {
        fn binding_descriptions() -> Vec<vk::VertexInputBindingDescription> {
            binding(24)
        }

        fn attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
            vec![attribute(0, 0), attribute(1, 12)]
        }
    }

    fn mesh<V>() -> MeshHandle<V>
    where
        V: VertexDescription,
    {
        MeshHandle {
            index: MeshIndex::from_raw(0),
            _phantom_data: PhantomData,
        }
    }

    fn error_of<I, D: DrawCall, P>(config: &PipelineConfig<'_, I, D, P>) -> String {
        match config.validate_vertex_input() {
            Ok(()) => panic!("the config was accepted"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn a_typed_config_matching_the_shader_layout_is_accepted() {
        let owned = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<MatchingVertex, NoPush>()
            .with_vertices(vec![MatchingVertex([0.0; 8])], vec![0]);
        owned.validate_vertex_input().unwrap();

        let shared = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<MatchingVertex, NoPush>()
            .with_shared_mesh(&mesh())
            .indirect();
        shared.validate_vertex_input().unwrap();
    }

    #[test]
    fn a_typed_config_with_a_different_attribute_offset_is_rejected() {
        let config = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<WrongOffsetVertex, NoPush>()
            .with_shared_mesh(&mesh());
        let err = error_of(&config);
        assert!(err.contains("WrongOffsetVertex"), "{err}");
        assert!(err.contains("attributes"), "{err}");
    }

    #[test]
    fn indirect_keeps_the_layout_check_of_the_typed_config() {
        let config = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<WrongOffsetVertex, NoPush>()
            .with_shared_mesh(&mesh())
            .indirect();
        let err = error_of(&config);
        assert!(err.contains("WrongOffsetVertex"), "{err}");
        assert!(err.contains("attributes"), "{err}");
    }

    #[test]
    fn a_typed_config_with_a_different_attribute_format_is_rejected() {
        let config = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<WrongFormatVertex, NoPush>()
            .with_vertices(vec![WrongFormatVertex([0.0; 8])], vec![0]);
        let err = error_of(&config);
        assert!(err.contains("WrongFormatVertex"), "{err}");
        assert!(err.contains("attributes"), "{err}");
    }

    #[test]
    fn a_typed_config_listing_matching_attributes_in_another_order_is_accepted() {
        builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<ReorderedVertex, NoPush>()
            .with_vertices(vec![ReorderedVertex([0.0; 8])], vec![0])
            .validate_vertex_input()
            .unwrap();
    }

    #[test]
    fn a_typed_config_with_a_different_stride_is_rejected() {
        let config = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<WrongStrideVertex, NoPush>()
            .with_vertices(vec![WrongStrideVertex([0.0; 6])], vec![0]);
        let err = error_of(&config);
        assert!(err.contains("WrongStrideVertex"), "{err}");
        assert!(err.contains("bindings"), "{err}");
    }

    #[test]
    fn a_typed_config_for_a_shader_without_vertex_input_is_rejected() {
        let config = builder(NO_VERTEX_INPUT_REFLECTION)
            .build_indexed::<MatchingVertex, NoPush>()
            .with_shared_mesh(&mesh());
        let err = error_of(&config);
        assert!(err.contains("MatchingVertex"), "{err}");
    }

    #[test]
    fn empty_typed_vertices_or_indices_are_rejected() {
        let no_vertices = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<MatchingVertex, NoPush>()
            .with_vertices(vec![], vec![0]);
        assert!(error_of(&no_vertices).contains("empty vertex data"));

        let no_indices = builder(VERTEX_INPUT_REFLECTION)
            .build_indexed::<MatchingVertex, NoPush>()
            .with_vertices(vec![MatchingVertex([0.0; 8])], vec![]);
        assert!(error_of(&no_indices).contains("empty vertex data"));
    }

    #[test]
    fn a_vertex_count_config_for_a_shader_with_vertex_input_is_rejected() {
        let config = builder(VERTEX_INPUT_REFLECTION).build_vertex_count::<NoPush>();
        let err = error_of(&config);
        assert!(err.contains("vertex count"), "{err}");

        builder(NO_VERTEX_INPUT_REFLECTION)
            .build_vertex_count::<NoPush>()
            .validate_vertex_input()
            .unwrap();
    }

    #[test]
    fn packed_bytes_must_be_a_nonzero_multiple_of_the_shader_stride() {
        let whole =
            builder(VERTEX_INPUT_REFLECTION).build_indexed_bytes::<NoPush>(vec![0; 64], vec![0]);
        whole.validate_vertex_input().unwrap();

        for len in [0, 31, 33] {
            let config = builder(VERTEX_INPUT_REFLECTION)
                .build_indexed_bytes::<NoPush>(vec![0; len], vec![0]);
            let err = error_of(&config);
            assert!(err.contains("vertex stride 32"), "{len}: {err}");
        }

        let no_stride =
            builder(NO_VERTEX_INPUT_REFLECTION).build_indexed_bytes::<NoPush>(vec![0; 32], vec![0]);
        assert!(error_of(&no_stride).contains("vertex stride 0"));

        let no_indices =
            builder(VERTEX_INPUT_REFLECTION).build_indexed_bytes::<NoPush>(vec![0; 32], vec![]);
        assert!(error_of(&no_indices).contains("empty index data"));
    }
}
