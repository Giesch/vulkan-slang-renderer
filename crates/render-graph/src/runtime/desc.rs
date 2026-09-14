//! Plain-data description consumed by render-graph validation and compilation.

pub use mltrs_render_graph::backend::GraphFormat;

macro_rules! id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $name(pub(crate) u32);
    )+};
}
id!(
    TexId, BufferId, ImportId, ValueId, UniformId, PipelineId, SchemaId
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FieldKey(pub(crate) u16);

#[derive(Debug, Default)]
pub(crate) struct GraphDesc {
    pub(crate) textures: Vec<TexDecl>,
    pub(crate) buffers: Vec<BufferDecl>,
    pub(crate) imports: Vec<ImportDecl>,
    pub(crate) values: Vec<ValueDecl>,
    pub(crate) uniforms: Vec<UniformDecl>,
    pub(crate) pipelines: Vec<PipelineDecl>,
    pub(crate) uploads: Vec<UploadDesc>,
    pub(crate) passes: Vec<PassDesc>,
}

#[derive(Debug, Clone)]
pub(crate) struct TexDecl {
    pub(crate) name: String,
    pub(crate) format: GraphFormat,
    pub(crate) size: SizeClass,
    pub(crate) usage: TexUsage,
}

#[derive(Debug, Clone)]
pub(crate) enum SizeClass {
    Fixed(u32, u32),
    Window,
    WindowDiv(u32),
}

#[derive(Debug, Clone)]
pub(crate) enum TexUsage {
    Storage,
    Color,
    Depth,
}

#[derive(Debug)]
pub(crate) struct BufferDecl {
    pub(crate) name: String,
    pub(crate) kind: BufferKind,
    pub(crate) capacity: Option<u32>,
    pub(crate) elem_size: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BufferKind {
    GpuOnlyFlight,
    Singleton,
    Immutable,
    Storage,
}

#[derive(Debug)]
pub(crate) struct ImportDecl {
    pub(crate) name: String,
}

#[derive(Debug)]
pub(crate) struct ValueDecl {
    pub(crate) name: String,
    pub(crate) kind: ValueKind,
    pub(crate) optional: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ValueKind {
    Count,
    Groups,
    Bytes { schema: SchemaId },
    Array { elem: SchemaId, max_len: u32 },
}

#[derive(Debug)]
pub(crate) struct UniformDecl {
    pub(crate) name: String,
    pub(crate) schema: SchemaId,
    pub(crate) source: UniformSourceDesc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UniformSourceDesc {
    pub(crate) data: Option<ValueId>,
    pub(crate) bindings: Vec<(FieldKey, ResourceRef)>,
}

#[derive(Debug)]
pub(crate) struct PipelineDecl {
    pub(crate) name: String,
    pub(crate) kind: PipelineKind,
    pub(crate) params: SchemaId,
    pub(crate) push: Option<SchemaId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PipelineKind {
    Compute,
    Graphics,
}

#[derive(Debug)]
pub(crate) struct UploadDesc {
    pub(crate) name: String,
    pub(crate) buffer: BufferId,
    pub(crate) value: ValueId,
}

#[derive(Debug)]
pub(crate) enum PassDesc {
    Leaf(LeafPass),
    When {
        name: String,
        value: ValueId,
        body: Vec<LeafPass>,
    },
    Repeat {
        name: String,
        count: ValueId,
        body: Vec<LeafPass>,
    },
}

#[derive(Debug)]
pub(crate) enum LeafPass {
    Compute(DispatchDesc),
    Raster(RasterDesc),
}

impl GraphDesc {
    pub(crate) fn leaves(&self) -> impl Iterator<Item = &LeafPass> {
        self.passes.iter().flat_map(|pass| match pass {
            PassDesc::Leaf(leaf) => std::slice::from_ref(leaf),
            PassDesc::When { body, .. } | PassDesc::Repeat { body, .. } => body.as_slice(),
        })
    }
}

/// One dispatch or draw, retaining command boundaries inside raster passes.
#[derive(Clone, Copy)]
pub(crate) struct Command<'a> {
    pub(crate) name: &'a str,
    pub(crate) pipeline: PipelineId,
    pub(crate) uniform: UniformId,
    pub(crate) push: Option<&'a PushDesc>,
}

impl<'a> From<&'a DispatchDesc> for Command<'a> {
    fn from(dispatch: &'a DispatchDesc) -> Self {
        Self {
            name: &dispatch.name,
            pipeline: dispatch.pipeline,
            uniform: dispatch.uniform,
            push: dispatch.push.as_ref(),
        }
    }
}

impl<'a> From<&'a DrawDesc> for Command<'a> {
    fn from(draw: &'a DrawDesc) -> Self {
        Self {
            name: &draw.name,
            pipeline: draw.pipeline,
            uniform: draw.uniform,
            push: draw.push.as_ref(),
        }
    }
}

impl<'a> Command<'a> {
    pub(crate) fn bindings(
        self,
        desc: &'a GraphDesc,
    ) -> impl Iterator<Item = &'a (FieldKey, ResourceRef)> + Clone {
        desc.uniforms
            .get(self.uniform.0 as usize)
            .into_iter()
            .flat_map(|uniform| &uniform.source.bindings)
            .chain(self.push.into_iter().flat_map(|push| &push.bindings))
    }
}

impl LeafPass {
    pub(crate) fn commands(&self) -> impl Iterator<Item = Command<'_>> {
        let (compute, draws) = match self {
            Self::Compute(compute) => (Some(Command::from(compute)), &[][..]),
            Self::Raster(raster) => (None, raster.draws.as_slice()),
        };

        compute.into_iter().chain(draws.iter().map(Command::from))
    }
}

#[derive(Debug)]
pub(crate) struct DispatchDesc {
    pub(crate) name: String,
    pub(crate) pipeline: PipelineId,
    pub(crate) uniform: UniformId,
    pub(crate) groups: GroupSource,
    pub(crate) push: Option<PushDesc>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum GroupSource {
    Fixed([u32; 3]),
    Value(ValueId),
}

#[derive(Debug)]
pub(crate) struct PushDesc {
    pub(crate) data: Option<ValueId>,
    pub(crate) schema: SchemaId,
    pub(crate) bindings: Vec<(FieldKey, ResourceRef)>,
}

#[derive(Debug)]
pub(crate) struct RasterDesc {
    pub(crate) name: String,
    pub(crate) targets: RasterTargets,
    pub(crate) draws: Vec<DrawDesc>,
}

#[derive(Debug)]
pub(crate) enum RasterTargets {
    Main,
    Offscreen {
        color: Vec<TexId>,
        depth: Option<TexId>,
    },
}

#[derive(Debug)]
pub(crate) struct DrawDesc {
    pub(crate) name: String,
    pub(crate) pipeline: PipelineId,
    pub(crate) uniform: UniformId,
    pub(crate) call: DrawCall,
    pub(crate) push: Option<PushDesc>,
}

#[derive(Debug, Clone)]
pub(crate) enum DrawCall {
    VertexCount(u32),
    WholeIndexed,
    IndexRange { first_index: u32, index_count: u32 },
    IndexedIndirect { args: BufferRef, draw_count: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResourceRef {
    Tex(TexId, TexAccess),
    Buf(BufferRef, BufAccess),
    External(ImportId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TexAccess {
    Read,
    ReadPrevious,
    Write,
    Mutate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BufAccess {
    Read,
    Mutate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BufferRef {
    pub(crate) buffer: BufferId,
    pub(crate) slot: SlotSel,
    pub(crate) offset: u32,
    pub(crate) range: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SlotSel {
    Current,
    Previous,
}

#[derive(Debug, Default)]
pub(crate) struct SchemaTable {
    pub(crate) schemas: Vec<SchemaDesc>,
}

impl SchemaTable {
    pub(crate) fn push(&mut self, schema: SchemaDesc) -> SchemaId {
        let id = SchemaId(self.schemas.len() as u32);
        self.schemas.push(schema);
        id
    }

    pub(crate) fn get(&self, id: SchemaId) -> Option<&SchemaDesc> {
        self.schemas.get(id.0 as usize)
    }
}

#[derive(Debug)]
pub(crate) struct SchemaDesc {
    pub(crate) name: String,
    pub(crate) size: u32,
    pub(crate) resource_fields: Vec<ResourceFieldKind>,
    pub(crate) layout: Option<SchemaLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResourceFieldKind {
    SampledTex,
    StorageTex,
    BufAddr,
}

#[derive(Debug)]
pub(crate) struct SchemaLayout {
    pub(crate) fields: Vec<SchemaField>,
}

#[derive(Debug)]
pub(crate) struct SchemaField {
    pub(crate) key: FieldKey,
    pub(crate) offset: u32,
    pub(crate) len: u32,
    pub(crate) kind: SchemaFieldKind,
}

#[derive(Debug)]
pub(crate) enum SchemaFieldKind {
    Data { src_offset: u32 },
    SampledTex,
    StorageTex,
    BufAddr,
}
