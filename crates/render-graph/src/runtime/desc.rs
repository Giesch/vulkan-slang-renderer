//! Plain-data description consumed by render-graph validation and compilation.
//!
//! Typed nodes lower into these tables; a plain-data host (the Roc platform)
//! builds them directly and executes them through [`crate::plain`]. Every id
//! indexes the matching `GraphDesc` table (or the `SchemaTable`).

pub use mltrs_render_graph::backend::GraphFormat;

macro_rules! id {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub u32);
    )+};
}
id!(
    TexId, BufferId, ImportId, ValueId, UniformId, PipelineId, SchemaId
);

/// A resource field's position in its schema's `resource_fields`; binding `i`
/// of a uniform or push source carries `FieldKey(i)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FieldKey(pub u16);

#[derive(Debug, Default)]
pub struct GraphDesc {
    pub textures: Vec<TexDecl>,
    pub buffers: Vec<BufferDecl>,
    pub imports: Vec<ImportDecl>,
    pub values: Vec<ValueDecl>,
    pub uniforms: Vec<UniformDecl>,
    pub pipelines: Vec<PipelineDecl>,
    pub uploads: Vec<UploadDesc>,
    pub passes: Vec<PassDesc>,
    /// A picking readback after the main raster pass. Typed graphs record
    /// picking outside the description, so only plain-data graphs set this.
    pub picking: Option<PickingDesc>,
}

/// A picking readback whose cursor position arrives as an 8-byte
/// `[f32; 2]` value each frame; an absent optional value skips the readback.
#[derive(Debug)]
pub struct PickingDesc {
    pub cursor: ValueId,
}

#[derive(Debug, Clone)]
pub struct TexDecl {
    pub name: String,
    pub format: GraphFormat,
    pub size: SizeClass,
    pub usage: TexUsage,
}

#[derive(Debug, Clone)]
pub enum SizeClass {
    Fixed(u32, u32),
    Window,
    WindowDiv(u32),
}

#[derive(Debug, Clone)]
pub enum TexUsage {
    Storage,
    Color,
    Depth,
}

#[derive(Debug)]
pub struct BufferDecl {
    pub name: String,
    pub kind: BufferKind,
    pub capacity: Option<u32>,
    pub elem_size: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BufferKind {
    GpuUpdatedFlight,
    Singleton,
    Immutable,
    Storage,
    /// a hardware index buffer; its `capacity` is the index count
    Index,
}

#[derive(Debug)]
pub struct ImportDecl {
    pub name: String,
}

#[derive(Debug)]
pub struct ValueDecl {
    pub name: String,
    pub kind: ValueKind,
    /// The frame may supply [`crate::plain::PlainValue::Absent`].
    pub optional: bool,
    pub scope: ValueScope,
}

/// The control-flow scope a value belongs to. It decides when an upload or
/// picking readback reading the value runs, and which values must be present
/// together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueScope {
    /// Outside any `When` or `Repeat`.
    Top,
    /// Inside the optional scope gated by `gate`: present exactly when the
    /// gate is. The gate value is itself `When { gate }`.
    When { gate: ValueId },
    /// Inside the repeat whose trip count is `count`; an upload reading it is
    /// skipped on a zero-trip frame.
    Repeat { count: ValueId },
}

#[derive(Debug, Clone, Copy)]
pub enum ValueKind {
    Count,
    Groups,
    Bytes { schema: SchemaId },
    Array { elem: SchemaId, max_len: u32 },
}

#[derive(Debug)]
pub struct UniformDecl {
    pub name: String,
    pub schema: SchemaId,
    pub source: UniformSourceDesc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniformSourceDesc {
    pub data: Option<ValueId>,
    pub bindings: Vec<(FieldKey, ResourceRef)>,
}

#[derive(Debug)]
pub struct PipelineDecl {
    pub name: String,
    pub kind: PipelineKind,
    pub params: SchemaId,
    pub push: Option<SchemaId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PipelineKind {
    Compute,
    /// Direct vertex-count and indexed draws.
    Graphics,
    /// Indexed-indirect draws; the shader declares `SV_DrawIndex`.
    GraphicsIndirect,
}

#[derive(Debug)]
pub struct UploadDesc {
    pub name: String,
    pub buffer: BufferId,
    pub value: ValueId,
}

#[derive(Debug)]
pub enum PassDesc {
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
pub enum LeafPass {
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
pub struct DispatchDesc {
    pub name: String,
    pub pipeline: PipelineId,
    pub uniform: UniformId,
    pub groups: GroupSource,
    pub push: Option<PushDesc>,
}

#[derive(Debug, Clone, Copy)]
pub enum GroupSource {
    Fixed([u32; 3]),
    Value(ValueId),
}

#[derive(Debug)]
pub struct PushDesc {
    pub data: Option<ValueId>,
    pub schema: SchemaId,
    pub bindings: Vec<(FieldKey, ResourceRef)>,
}

#[derive(Debug)]
pub struct RasterDesc {
    pub name: String,
    pub targets: RasterTargets,
    pub draws: Vec<DrawDesc>,
}

#[derive(Debug)]
pub enum RasterTargets {
    Main,
    Offscreen {
        color: Vec<TexId>,
        depth: Option<TexId>,
    },
}

#[derive(Debug)]
pub struct DrawDesc {
    pub name: String,
    pub pipeline: PipelineId,
    pub uniform: UniformId,
    pub call: DrawCall,
    pub push: Option<PushDesc>,
}

#[derive(Debug, Clone)]
pub enum DrawCall {
    VertexCount(u32),
    WholeIndexed {
        indices: BufferId,
    },
    IndexRange {
        indices: BufferId,
        first_index: u32,
        index_count: u32,
    },
    IndexedIndirect {
        indices: BufferId,
        args: BufferRef,
        draw_count: u32,
    },
}

impl DrawCall {
    /// The index buffer an indexed draw binds; `None` for a vertex-count draw.
    pub(crate) fn index_buffer(&self) -> Option<BufferId> {
        match self {
            Self::VertexCount(_) => None,
            Self::WholeIndexed { indices }
            | Self::IndexRange { indices, .. }
            | Self::IndexedIndirect { indices, .. } => Some(*indices),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceRef {
    Tex(TexId, TexAccess),
    Buf(BufferRef, BufAccess),
    External(ImportId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TexAccess {
    Read,
    ReadPrevious,
    Write,
    Mutate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufAccess {
    Read,
    Mutate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferRef {
    pub buffer: BufferId,
    pub slot: SlotSel,
    pub offset: u32,
    pub range: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotSel {
    Current,
    Previous,
}

#[derive(Debug, Default)]
pub struct SchemaTable {
    pub schemas: Vec<SchemaDesc>,
}

impl SchemaTable {
    pub fn push(&mut self, schema: SchemaDesc) -> SchemaId {
        let id = SchemaId(self.schemas.len() as u32);
        self.schemas.push(schema);
        id
    }

    pub fn get(&self, id: SchemaId) -> Option<&SchemaDesc> {
        self.schemas.get(id.0 as usize)
    }
}

#[derive(Debug)]
pub struct SchemaDesc {
    pub name: String,
    pub size: u32,
    pub resource_fields: Vec<ResourceFieldKind>,
    pub layout: Option<SchemaLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceFieldKind {
    SampledTex,
    StorageTex,
    BufAddr,
}

#[derive(Debug)]
pub struct SchemaLayout {
    pub fields: Vec<SchemaField>,
}

#[derive(Debug)]
pub struct SchemaField {
    pub key: FieldKey,
    pub offset: u32,
    pub len: u32,
    pub kind: SchemaFieldKind,
}

#[derive(Debug)]
pub enum SchemaFieldKind {
    Data { src_offset: u32 },
    SampledTex,
    StorageTex,
    BufAddr,
}
