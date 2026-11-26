use std::marker::PhantomData;

use ash::vk;

use crate::shaders::atlas::{ComputeShaderAtlasEntry, ShaderAtlasEntry};

use super::gpu_write::PushConstantBlock;
use super::{
    ComputeShaderPipelineLayout, RawUniformBufferHandle, ShaderPipelineLayout,
    StorageTextureHandle, TextureHandle,
};

/// A newtype-wrapped index into one of the renderer's pipeline storages.
/// Distinct types per storage make cross-storage index mixups a compile error.
/// The indexes are opaque to callers; only the renderer mints and resolves them.
pub trait PipelineIndex: Copy {
    #[doc(hidden)]
    fn from_raw(index: usize) -> Self;
    #[doc(hidden)]
    fn raw(self) -> usize;
}

/// Index into `PipelineStorage` (graphics pipelines, shared by Graphics,
/// GraphicsIndirect, and picking pipelines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphicsPipelineIndex(usize);

/// Index into `ComputePipelineStorage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComputePipelineIndex(usize);

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

/// A marker trait for different draw call types
pub trait DrawCall {
    /// The typed index into the storage this draw-call kind lives in.
    type Index: PipelineIndex;
}

/// A marker for a graphics pipeline drawn with direct draws: `cmd_draw` with a
/// vertex count, or `cmd_draw_indexed` with an index buffer. Its vertex entry
/// point does not read `SV_DrawIndex`.
#[derive(Debug)]
pub struct Graphics;

impl DrawCall for Graphics {
    type Index = GraphicsPipelineIndex;
}

/// A marker for a graphics pipeline drawn with `cmd_draw_indexed_indirect`,
/// reading draw arguments from a buffer. Its vertex entry point reads
/// `SV_DrawIndex` to tell the sub-draws apart.
#[derive(Debug)]
pub struct GraphicsIndirect;

impl DrawCall for GraphicsIndirect {
    type Index = GraphicsPipelineIndex;
}

mod sealed {
    pub trait Sealed {}
}

/// The graphics draw-call kinds, keyed by whether the shader's vertex entry
/// point reads `SV_DrawIndex`. Sealed: the shader decides the kind, and
/// generated `pipeline_config()` spells it.
pub trait GraphicsDrawCall: DrawCall<Index = GraphicsPipelineIndex> + sealed::Sealed {
    /// whether this kind's vertex entry point must declare `SV_DrawIndex`
    const DECLARES_DRAW_INDEX: bool;
}

impl sealed::Sealed for Graphics {}

impl GraphicsDrawCall for Graphics {
    const DECLARES_DRAW_INDEX: bool = false;
}

impl sealed::Sealed for GraphicsIndirect {}

impl GraphicsDrawCall for GraphicsIndirect {
    const DECLARES_DRAW_INDEX: bool = true;
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

    pub descriptor_pool: vk::DescriptorPool,
    pub descriptor_sets: Vec<vk::DescriptorSet>,

    pub shader: Box<dyn ShaderAtlasEntry>,

    /// kept so a hot reload rebuilds with the state the pipeline was created with
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

/// A complete graphics pipeline config. `K` is the draw-call kind —
/// [`Graphics`] or [`GraphicsIndirect`] — and is carried through to the
/// [`PipelineHandle`] that `Renderer::create_pipeline` returns. Vertex data is
/// not part of the config: the shader pulls it through an address binding,
/// and indexed draws name their [`super::IndexBufferHandle`] at draw time.
pub struct PipelineConfig<'t, K: DrawCall, P = NoPush> {
    pub(super) shader: Box<dyn ShaderAtlasEntry>,
    _draw_call: PhantomData<K>,
    _push: PhantomData<P>,
    pub(super) texture_handles: Vec<&'t TextureHandle>,
    pub(super) uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub(super) storage_texture_handles: Vec<&'t StorageTextureHandle>,
    pub(super) raster_state: RasterState,
}

impl<K: GraphicsDrawCall, P> PipelineConfig<'_, K, P> {
    /// Checks the shader's vertex entry point against the draw-call kind.
    /// Runs before any GPU allocation, so a rejected config leaks nothing.
    ///
    /// A runtime shader's reflection is not checked by codegen, so a vertex
    /// input struct or a `SV_DrawIndex` mismatch with `K` is caught here.
    pub(super) fn validate_shader_interface(&self) -> anyhow::Result<()> {
        let shader_name = self.shader.source_file_name();
        let vertex_entry_point = &self.shader.reflection_json().vertex_entry_point;

        let struct_parameters = vertex_entry_point.struct_parameter_names();
        anyhow::ensure!(
            struct_parameters.is_empty(),
            "pipeline for {shader_name}: vertex entry point '{}' takes the varying struct \
             parameter(s) {struct_parameters:?}; vertex input structs are not supported. \
             Accept `uint vertexId : SV_VertexID` and pull vertices through an \
             `mltrs::ImmutableAddr<T>`, `mltrs::ReadAddr<T>`, or `mltrs::Addr<T>` binding",
            vertex_entry_point.entry_point_name,
        );

        let declares_draw_index = vertex_entry_point.declares_draw_index();
        anyhow::ensure!(
            declares_draw_index == K::DECLARES_DRAW_INDEX,
            "pipeline for {shader_name} was built as {}, but its vertex entry point '{}' {} \
             SV_DrawIndex; use {} instead",
            std::any::type_name::<K>(),
            vertex_entry_point.entry_point_name,
            if declares_draw_index {
                "reads"
            } else {
                "does not read"
            },
            if declares_draw_index {
                "PipelineConfigBuilder::build_graphics_indirect"
            } else {
                "PipelineConfigBuilder::build_graphics"
            },
        );

        Ok(())
    }
}

impl<K: DrawCall, P> PipelineConfig<'_, K, P> {
    /// Bake this pipeline with explicit fixed-function raster state instead of
    /// [`RasterState::default()`] (which reproduces the renderer's original
    /// hardcoded pipeline).
    pub fn with_raster_state(mut self, raster_state: RasterState) -> Self {
        self.raster_state = raster_state;

        self
    }
}

/// The fields every graphics pipeline config shares. The terminal method —
/// [`Self::build_graphics`] or [`Self::build_graphics_indirect`] — decides the
/// draw-call kind; it must agree with whether the shader reads `SV_DrawIndex`,
/// which `Renderer::create_pipeline` checks before any GPU allocation.
pub struct PipelineConfigBuilder<'t> {
    pub shader: Box<dyn ShaderAtlasEntry>,
    pub texture_handles: Vec<&'t TextureHandle>,
    pub uniform_buffer_handles: Vec<RawUniformBufferHandle>,
    pub storage_texture_handles: Vec<&'t StorageTextureHandle>,
}

impl<'t> PipelineConfigBuilder<'t> {
    /// Terminal call for a shader drawn with direct vertex-count or indexed
    /// draws; its vertex entry point must not read `SV_DrawIndex`.
    pub fn build_graphics<P>(self) -> PipelineConfig<'t, Graphics, P> {
        self.into_config()
    }

    /// Terminal call for a shader drawn with indexed-indirect draws; its
    /// vertex entry point must read `SV_DrawIndex`.
    pub fn build_graphics_indirect<P>(self) -> PipelineConfig<'t, GraphicsIndirect, P> {
        self.into_config()
    }

    fn into_config<K: DrawCall, P>(self) -> PipelineConfig<'t, K, P> {
        PipelineConfig {
            shader: self.shader,
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
    //! `validate_shader_interface` runs at the top of `Renderer::init_pipeline`
    //! and `create_picking_pipeline`, before anything is allocated, so these
    //! configs exercise the pre-allocation checks with no GPU.

    use mltrs_slang_reflection::json::{
        EntryPointParameter, ReflectionJson, ScalarEntryPointParameter, ScalarType,
        SemanticScalarEntryPointParameter,
    };

    use super::*;
    use crate::shaders::runtime::RuntimeShader;

    /// reflection whose vertex stage reads only `SV_VertexID`, pulling its
    /// vertices through an `ImmutableAddr<Vertex>`
    const VERTEX_PULLING_REFLECTION: &str =
        include_str!("../../../slang-reflection/src/fixtures/basic_triangle.json");

    fn spv_module() -> Vec<u8> {
        // magic number, version, generator, bound, schema
        [0x0723_0203u32, 0x0001_0000, 0, 1, 0]
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .collect()
    }

    fn builder(reflection: &ReflectionJson) -> PipelineConfigBuilder<'static> {
        let module = spv_module();
        let reflection = serde_json::to_string(reflection).unwrap();

        PipelineConfigBuilder {
            shader: Box::new(RuntimeShader::new(&reflection, &module, &module).unwrap()),
            texture_handles: vec![],
            uniform_buffer_handles: vec![],
            storage_texture_handles: vec![],
        }
    }

    fn vertex_pulling() -> ReflectionJson {
        serde_json::from_str(VERTEX_PULLING_REFLECTION).unwrap()
    }

    /// the vertex-pulling reflection, with `SV_DrawIndex` read as well
    fn draw_index_reading() -> ReflectionJson {
        let mut reflection = vertex_pulling();
        reflection
            .vertex_entry_point
            .parameters
            .push(EntryPointParameter::Scalar(
                ScalarEntryPointParameter::Semantic(SemanticScalarEntryPointParameter {
                    parameter_name: "drawIndex".into(),
                    semantic_name: "SV_DRAWINDEX".into(),
                    scalar_type: ScalarType::Uint32,
                }),
            ));

        reflection
    }

    /// the vertex-pulling reflection, with the fragment stage's varying struct
    /// also taken by the vertex stage, as a fixed-function vertex input would be
    fn vertex_struct_input() -> ReflectionJson {
        let mut reflection = vertex_pulling();
        let struct_input = reflection.fragment_entry_point.parameters[0].clone();
        assert!(matches!(struct_input, EntryPointParameter::Struct(_)));
        reflection.vertex_entry_point.parameters.push(struct_input);

        reflection
    }

    fn error_of<K: GraphicsDrawCall, P>(config: &PipelineConfig<'_, K, P>) -> String {
        match config.validate_shader_interface() {
            Ok(()) => panic!("the config was accepted"),
            Err(err) => err.to_string(),
        }
    }

    #[test]
    fn a_graphics_config_for_a_vertex_id_shader_is_accepted() {
        builder(&vertex_pulling())
            .build_graphics::<NoPush>()
            .validate_shader_interface()
            .unwrap();
    }

    #[test]
    fn a_graphics_indirect_config_for_a_draw_index_shader_is_accepted() {
        builder(&draw_index_reading())
            .build_graphics_indirect::<NoPush>()
            .validate_shader_interface()
            .unwrap();
    }

    #[test]
    fn a_graphics_config_for_a_draw_index_shader_is_rejected() {
        let config = builder(&draw_index_reading()).build_graphics::<NoPush>();
        let err = error_of(&config);
        assert!(err.contains("reads SV_DrawIndex"), "{err}");
        assert!(err.contains("build_graphics_indirect"), "{err}");
    }

    #[test]
    fn a_graphics_indirect_config_for_a_vertex_id_shader_is_rejected() {
        let config = builder(&vertex_pulling()).build_graphics_indirect::<NoPush>();
        let err = error_of(&config);
        assert!(err.contains("does not read SV_DrawIndex"), "{err}");
        assert!(err.contains("build_graphics"), "{err}");
    }

    #[test]
    fn a_vertex_struct_input_is_rejected_for_either_kind() {
        let graphics = builder(&vertex_struct_input()).build_graphics::<NoPush>();
        let err = error_of(&graphics);
        assert!(err.contains("fragVertex"), "{err}");
        assert!(err.contains("SV_VertexID"), "{err}");
        assert!(err.contains("mltrs::ImmutableAddr<T>"), "{err}");

        let indirect = builder(&vertex_struct_input()).build_graphics_indirect::<NoPush>();
        assert!(error_of(&indirect).contains("vertex input structs"));
    }
}
