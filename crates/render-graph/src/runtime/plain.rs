//! Render graphs built from plain data instead of typed nodes.
//!
//! A host that receives its graph as data (the Roc platform) cannot name a
//! generated `GraphShaderParams` type per shader. It describes the graph as a
//! [`GraphDesc`] plus a [`SchemaTable`] whose schemas carry a [`SchemaLayout`],
//! binds each declared buffer, uniform, pipeline, and import to a backend
//! resource with [`PlainBindings`], and supplies one [`PlainValue`] per
//! [`ValueDecl`] each frame.
//!
//! The description passes the same validation as a lowered typed graph. Each
//! uniform and push block executes its compiled assembly program: data fields
//! copy from the frame's bytes value, resource fields resolve to 8-byte
//! bindless handles or buffer addresses, and padding stays zero.

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

pub use super::desc::*;

use super::compile::{
    AsmId, AssemblyProgram, AssemblySrc, CompiledDraw, CompiledGraph, CompiledLeaf,
    CompiledLeafKind, CompiledPass, compile,
};
use super::expand::{ExecStep, FrameShape, RunState, expand};
use super::validate::{RESOURCE_FIELD_BYTES, validate, validation_message};
use super::{
    CapturedBufferSlot, ComputePipelineKey, CpuUpdatedSlot, GpuUpdatedSlot,
    GraphicsIndirectPipelineKey, GraphicsPipelineKey, ImmutableSlot, IndexBufferSlot, PhysTex,
    PickingPipelineKey, SingletonSlot, StagedTarget, StagedWrites, UniformSlot,
    prepare_physical_textures, range_in_bounds, validate_live_buffers,
};
use crate::backend::{
    BackendTypes, BufferAddressKind, FrameBackend, FrameLookup, IndexedIndirectArgs,
    PreparationBackend,
};
use crate::bindless::{BindlessHandle, Sampler2D};
use crate::commands::{
    CommandBatch, DrawCallConfig, IndirectRequest, PendingDrawCommand, PickingDrawConfig,
    PushConstantBytes,
};

/// The backend buffer a [`BufferDecl`] names, with the element count and
/// stride the backend allocated. `I` is the backend's exact indexed-indirect
/// record, so an argument buffer cannot carry another layout.
pub struct PlainBuffer<I> {
    role: PlainBufferRole,
    index: usize,
    len: u32,
    element_size: usize,
    _indirect: PhantomData<fn() -> I>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlainBufferRole {
    CpuUpdated,
    GpuUpdated,
    Immutable,
    Singleton,
    Index,
    IndirectArgs,
}

impl<I> PlainBuffer<I> {
    fn new(role: PlainBufferRole, index: usize, len: u32, element_size: usize) -> Self {
        Self {
            role,
            index,
            len,
            element_size,
            _indirect: PhantomData,
        }
    }

    /// Binds a [`BufferKind::Storage`] declaration; uploads target it.
    pub fn cpu_updated<T>(slot: impl Into<CpuUpdatedSlot<T>>) -> Self {
        let slot = slot.into();

        Self::new(
            PlainBufferRole::CpuUpdated,
            slot.index,
            slot.len,
            slot.element_size,
        )
    }

    /// Binds a [`BufferKind::GpuUpdatedFlight`] declaration.
    pub fn gpu_updated<T>(slot: impl Into<GpuUpdatedSlot<T>>) -> Self {
        let slot = slot.into();

        Self::new(
            PlainBufferRole::GpuUpdated,
            slot.index,
            slot.len,
            slot.element_size,
        )
    }

    /// Binds a [`BufferKind::Immutable`] declaration read through addresses.
    pub fn immutable<T>(slot: impl Into<ImmutableSlot<T>>) -> Self {
        let slot = slot.into();

        Self::new(
            PlainBufferRole::Immutable,
            slot.index,
            slot.len,
            slot.element_size,
        )
    }

    /// Binds a [`BufferKind::Singleton`] declaration.
    pub fn singleton<T>(slot: impl Into<SingletonSlot<T>>) -> Self {
        let slot = slot.into();

        Self::new(
            PlainBufferRole::Singleton,
            slot.index,
            slot.len,
            slot.element_size,
        )
    }

    /// Binds a [`BufferKind::Index`] declaration; its capacity must be the
    /// slot's index count.
    pub fn index(slot: impl Into<IndexBufferSlot>) -> Self {
        let slot = slot.into();

        Self::new(
            PlainBufferRole::Index,
            slot.index(),
            slot.index_count(),
            size_of::<u32>(),
        )
    }

    /// Binds a [`BufferKind::Immutable`] declaration that indexed-indirect
    /// draws read their argument records from.
    pub fn indirect_args(slot: impl Into<ImmutableSlot<I>>) -> Self {
        let slot = slot.into();

        Self::new(
            PlainBufferRole::IndirectArgs,
            slot.index,
            slot.len,
            size_of::<I>(),
        )
    }

    fn kind(&self) -> BufferKind {
        match self.role {
            PlainBufferRole::CpuUpdated => BufferKind::Storage,
            PlainBufferRole::GpuUpdated => BufferKind::GpuUpdatedFlight,
            PlainBufferRole::Immutable | PlainBufferRole::IndirectArgs => BufferKind::Immutable,
            PlainBufferRole::Singleton => BufferKind::Singleton,
            PlainBufferRole::Index => BufferKind::Index,
        }
    }

    /// The allocated byte length, or `None` if `len * element_size` overflows.
    fn byte_len(&self) -> Option<u64> {
        let element_size = u64::try_from(self.element_size).ok()?;

        u64::from(self.len).checked_mul(element_size)
    }
}

/// The backend uniform buffer a [`UniformDecl`] writes. The backend rejects a
/// write whose size differs from the buffer's.
#[derive(Debug, Clone, Copy)]
pub struct PlainUniform {
    index: usize,
}

impl PlainUniform {
    pub fn new<T>(slot: impl Into<UniformSlot<T>>) -> Self {
        Self {
            index: slot.into().index,
        }
    }
}

/// The backend pipeline a [`PipelineDecl`] names, tagged with the family its
/// key came from so validation can compare it with the declared kind.
#[derive(Debug, Clone, Copy)]
pub struct PlainPipeline {
    kind: PipelineKind,
    index: usize,
}

impl PlainPipeline {
    pub fn compute<P>(key: impl Into<ComputePipelineKey<P>>) -> Self {
        Self {
            kind: PipelineKind::Compute,
            index: key.into().index(),
        }
    }

    pub fn graphics<P>(key: impl Into<GraphicsPipelineKey<P>>) -> Self {
        Self {
            kind: PipelineKind::Graphics,
            index: key.into().index(),
        }
    }

    pub fn graphics_indirect<P>(key: impl Into<GraphicsIndirectPipelineKey<P>>) -> Self {
        Self {
            kind: PipelineKind::GraphicsIndirect,
            index: key.into().index(),
        }
    }
}

/// Backend resources for every table of a [`GraphDesc`], in table order:
/// `buffers[i]` binds `BufferId(i)`, and likewise for uniforms, pipelines, and
/// imports. `picking` binds [`GraphDesc::picking`].
pub struct PlainBindings<I> {
    pub buffers: Vec<PlainBuffer<I>>,
    pub uniforms: Vec<PlainUniform>,
    pub pipelines: Vec<PlainPipeline>,
    pub imports: Vec<BindlessHandle<Sampler2D>>,
    pub picking: Option<PickingPipelineKey>,
}

impl<I> Default for PlainBindings<I> {
    fn default() -> Self {
        Self {
            buffers: vec![],
            uniforms: vec![],
            pipelines: vec![],
            imports: vec![],
            picking: None,
        }
    }
}

/// One frame's value for one [`ValueDecl`], in declaration order.
#[derive(Debug, Clone, Copy)]
pub enum PlainValue<'a> {
    /// An optional value this frame skips: its `When` body, upload, or
    /// picking readback does not run.
    Absent,
    /// A [`ValueKind::Count`] value, such as a repeat's trip count.
    Count(u32),
    /// A [`ValueKind::Bytes`] value of exactly its schema's size.
    Bytes(&'a [u8]),
    /// A [`ValueKind::Array`] value of `count` packed elements.
    Array { count: u32, bytes: &'a [u8] },
}

/// A validated plain-data graph, before any backend resources are created.
pub struct PlainRenderGraph<I> {
    desc: GraphDesc,
    schemas: SchemaTable,
    compiled: CompiledGraph,
    bindings: PlainBindings<I>,
}

impl<I: IndexedIndirectArgs> PlainRenderGraph<I> {
    /// Validates the description, compiles it, and checks every binding
    /// against its declaration.
    pub fn new(
        desc: GraphDesc,
        schemas: SchemaTable,
        bindings: PlainBindings<I>,
    ) -> anyhow::Result<Self> {
        let analysis = validate(&desc, &schemas)
            .map_err(|errors| anyhow::anyhow!(validation_message(&errors)))?;
        let compiled = compile(&desc, &analysis, &schemas);
        let errors = binding_errors(&desc, &schemas, &bindings);
        if !errors.is_empty() {
            let mut message = String::from("plain render graph bindings are invalid:");
            for error in errors {
                message.push_str("\n  - ");
                message.push_str(&error);
            }
            anyhow::bail!(message);
        }

        Ok(Self {
            desc,
            schemas,
            compiled,
            bindings,
        })
    }

    /// Checks device limits, then creates the physical textures. Failures
    /// follow [`crate::RenderGraph::prepare`]'s staging.
    pub fn prepare<B: PreparationBackend<IndirectCommand = I>>(
        self,
        backend: &mut B,
    ) -> anyhow::Result<PreparedPlainRenderGraph<B>> {
        let (phys, keep_alive) =
            prepare_physical_textures(backend, &self.desc.textures, &self.compiled.tex_phys)?;
        let uniform_slots = self
            .bindings
            .uniforms
            .iter()
            .map(|uniform| uniform.index)
            .collect();
        let buffer_slots = self
            .desc
            .buffers
            .iter()
            .zip(&self.bindings.buffers)
            .map(|(decl, buffer)| CapturedBufferSlot {
                kind: decl.kind,
                index: buffer.index,
                name: decl.name.clone(),
            })
            .collect();

        Ok(PreparedPlainRenderGraph {
            run: RunState::new(&self.compiled.tex_phys),
            graph: self,
            uniform_slots,
            buffer_slots,
            phys,
            _keep_alive: keep_alive,
        })
    }
}

/// Checks that each binding is the kind, size, and capacity its declaration
/// states, and that every block has a field layout to assemble from.
fn binding_errors<I>(
    desc: &GraphDesc,
    schemas: &SchemaTable,
    bindings: &PlainBindings<I>,
) -> Vec<String> {
    let mut errors = vec![];
    let mut table_len = |table: &str, declared: usize, bound: usize| {
        if declared != bound {
            errors.push(format!("{declared} {table} declared but {bound} bound"));
        }
    };
    table_len("buffers", desc.buffers.len(), bindings.buffers.len());
    table_len("uniforms", desc.uniforms.len(), bindings.uniforms.len());
    table_len("pipelines", desc.pipelines.len(), bindings.pipelines.len());
    table_len("imports", desc.imports.len(), bindings.imports.len());
    let picking_bound = desc.picking.is_some() == bindings.picking.is_some();
    if !picking_bound {
        errors.push("picking is declared without a picking pipeline, or the reverse".into());
    }
    if !errors.is_empty() {
        return errors;
    }

    for (decl, buffer) in desc.buffers.iter().zip(&bindings.buffers) {
        check_buffer_binding(decl, buffer, &mut errors);
    }

    let mut uniform_slots = HashSet::new();
    for (decl, uniform) in desc.uniforms.iter().zip(&bindings.uniforms) {
        // `GraphDesc` shares one uniform buffer by sharing one `UniformId`;
        // two declarations staging into one buffer would overwrite each other
        if !uniform_slots.insert(uniform.index) {
            errors.push(format!(
                "uniform {} is bound to backend uniform buffer {}, which another uniform \
                 declaration already writes",
                decl.name, uniform.index
            ));
        }
    }

    for (decl, pipeline) in desc.pipelines.iter().zip(&bindings.pipelines) {
        if decl.kind != pipeline.kind {
            errors.push(format!(
                "pipeline {} is declared {:?} but bound to a {:?} pipeline",
                decl.name, decl.kind, pipeline.kind
            ));
        }
    }

    for value in &desc.values {
        match value.kind {
            ValueKind::Groups => errors.push(format!(
                "value {}: per-frame group counts are unsupported",
                value.name
            )),
            ValueKind::Array { elem, .. } if schemas.get(elem).is_some_and(|s| s.size == 0) => {
                errors.push(format!("value {} has zero-sized elements", value.name))
            }
            _ => {}
        }
    }

    // each block schema once, whether a uniform, a pipeline, or a command
    // names it, so a shared schema reports once
    let mut block_schemas: Vec<SchemaId> = desc
        .uniforms
        .iter()
        .map(|uniform| uniform.schema)
        .chain(desc.pipelines.iter().filter_map(|pipeline| pipeline.push))
        .chain(
            desc.leaves()
                .flat_map(LeafPass::commands)
                .filter_map(|command| command.push.map(|push| push.schema)),
        )
        .collect();
    block_schemas.sort_by_key(|schema| schema.0);
    block_schemas.dedup();
    for schema in block_schemas.iter().filter_map(|id| schemas.get(*id)) {
        if schema.layout.is_none() {
            errors.push(format!("schema {} has no field layout", schema.name));
        }
    }

    // uniform sources once per declaration, consumed or not; push sources
    // belong to exactly one command
    for uniform in &desc.uniforms {
        check_buffer_offsets(
            &uniform.name,
            &uniform.source.bindings,
            desc,
            bindings,
            &mut errors,
        );
    }
    for leaf in desc.leaves() {
        for command in leaf.commands() {
            check_command_sizes(command, desc, schemas, &mut errors);
            if let Some(push) = command.push {
                check_buffer_offsets(command.name, &push.bindings, desc, bindings, &mut errors);
            }
        }
        if let LeafPass::Raster(raster) = leaf {
            for draw in &raster.draws {
                check_indirect_args(draw, desc, bindings, &mut errors);
            }
        }
    }

    errors
}

/// A buffer binding is the declared kind, and the declared capacity and
/// element size match what the backend allocated.
fn check_buffer_binding<I>(decl: &BufferDecl, buffer: &PlainBuffer<I>, errors: &mut Vec<String>) {
    if decl.kind != buffer.kind() {
        errors.push(format!(
            "buffer {} is declared {:?} but bound to a {:?} buffer",
            decl.name,
            decl.kind,
            buffer.kind()
        ));
    }
    match decl.capacity {
        Some(capacity) if capacity != buffer.len => errors.push(format!(
            "buffer {} declares capacity {capacity} but its backend buffer holds {}",
            decl.name, buffer.len
        )),
        None if decl.kind == BufferKind::Index => errors.push(format!(
            "index buffer {} must declare its index count as its capacity",
            decl.name
        )),
        _ => {}
    }
    if let Some(elem_size) = decl.elem_size
        && elem_size as usize != buffer.element_size
    {
        errors.push(format!(
            "buffer {} declares {elem_size}-byte elements but its backend stride is {}",
            decl.name, buffer.element_size
        ));
    }
}

/// A command writes a uniform and push block of the sizes its pipeline
/// expects.
fn check_command_sizes(
    command: Command<'_>,
    desc: &GraphDesc,
    schemas: &SchemaTable,
    errors: &mut Vec<String>,
) {
    let size = |schema: Option<SchemaId>| {
        schema.and_then(|schema| schemas.get(schema).map(|schema| schema.size))
    };
    let pipeline = &desc.pipelines[command.pipeline.0 as usize];
    let uniform = &desc.uniforms[command.uniform.0 as usize];
    let params_size = size(Some(pipeline.params));
    let uniform_size = size(Some(uniform.schema));
    if params_size != uniform_size {
        errors.push(format!(
            "{} writes uniform {} of {uniform_size:?} bytes to pipeline {} with \
             {params_size:?}-byte params",
            command.name, uniform.name, pipeline.name
        ));
    }
    let pipeline_push = size(pipeline.push);
    let command_push = size(command.push.map(|push| push.schema));
    if pipeline_push != command_push {
        errors.push(format!(
            "{} supplies a {command_push:?}-byte push block to pipeline {} with a \
             {pipeline_push:?}-byte push block",
            command.name, pipeline.name
        ));
    }
}

/// A buffer device address bypasses `robustBufferAccess`, so every buffer
/// binding's offset must lie inside the allocation.
fn check_buffer_offsets<I>(
    block: &str,
    resources: &[(FieldKey, ResourceRef)],
    desc: &GraphDesc,
    bindings: &PlainBindings<I>,
    errors: &mut Vec<String>,
) {
    for (key, resource) in resources {
        let ResourceRef::Buf(buffer, _) = resource else {
            continue;
        };

        let id = buffer.buffer.0 as usize;
        let name = &desc.buffers[id].name;
        let Some(byte_len) = bindings.buffers[id].byte_len() else {
            errors.push(format!(
                "{block} binding {}: buffer {name}'s byte length overflows u64",
                key.0
            ));
            continue;
        };

        let in_bounds = u64::from(buffer.offset) < byte_len;
        if !in_bounds {
            errors.push(format!(
                "{block} binding {} offset {} is past the end of buffer {name} ({byte_len} \
                 bytes)",
                key.0, buffer.offset
            ));
        }
    }
}

/// An indexed-indirect draw reads whole records of the backend's exact type
/// from an argument buffer bound with [`PlainBuffer::indirect_args`].
fn check_indirect_args<I>(
    draw: &DrawDesc,
    desc: &GraphDesc,
    bindings: &PlainBindings<I>,
    errors: &mut Vec<String>,
) {
    let DrawCall::IndexedIndirect {
        args, draw_count, ..
    } = &draw.call
    else {
        return;
    };

    let args_buffer = &bindings.buffers[args.buffer.0 as usize];
    if args_buffer.role != PlainBufferRole::IndirectArgs {
        errors.push(format!(
            "draw {}'s argument buffer {} is not bound with PlainBuffer::indirect_args",
            draw.name, desc.buffers[args.buffer.0 as usize].name
        ));
        return;
    }

    let len = args_buffer.len;
    let stride = size_of::<I>() as u32;
    let aligned = args.offset.is_multiple_of(stride);
    let first_command = args.offset / stride;
    let in_bounds = *draw_count > 0 && range_in_bounds(first_command, *draw_count, len);
    let whole_records = args.slot == SlotSel::Current && aligned && in_bounds;
    if !whole_records {
        errors.push(format!(
            "draw {}'s indirect commands at byte offset {} (count {draw_count}) are not whole \
             {stride}-byte records inside an argument buffer of {len}",
            draw.name, args.offset
        ));
    }
}

/// An executable plain-data graph, returned by [`PlainRenderGraph::prepare`].
pub struct PreparedPlainRenderGraph<B: BackendTypes> {
    graph: PlainRenderGraph<B::IndirectCommand>,
    uniform_slots: Vec<usize>,
    buffer_slots: Vec<CapturedBufferSlot>,
    pub(super) run: RunState,
    phys: Vec<PhysTex>,
    /// the physical images and sampled aliases backing the logical textures
    _keep_alive: Vec<B::Resource>,
}

impl<B: BackendTypes> PreparedPlainRenderGraph<B> {
    /// Plans and submits one frame. `values` holds one value per
    /// [`ValueDecl`]; every value is checked before anything is staged.
    /// Texture cursors advance only after a successful submission.
    pub fn execute<F: FrameBackend<Backend = B>>(
        &mut self,
        frame: F,
        values: &[PlainValue<'_>],
    ) -> Result<(), F::Error> {
        let lookup = frame.lookup();
        validate_live_buffers(&self.uniform_slots, &self.buffer_slots, lookup)?;
        let shape = self.frame_shape(values)?;
        let expanded = expand(&self.graph.compiled, &self.run, &shape);
        let planner = FramePlanner {
            graph: &self.graph,
            phys: &self.phys,
            lookup,
            values,
            shape: &shape,
        };
        let batch = planner.plan(&expanded.steps)?;
        let next = expanded.next;

        // only cycle textures if the frame submission succeeded
        frame.submit(batch, || self.run = next)
    }

    /// Checks each value against its declaration and derives the frame's
    /// repeat counts and optional gates.
    fn frame_shape(&self, values: &[PlainValue<'_>]) -> anyhow::Result<FrameShape> {
        let decls = &self.graph.desc.values;
        anyhow::ensure!(
            values.len() == decls.len(),
            "render graph: {} values declared but {} received",
            decls.len(),
            values.len(),
        );

        let mut shape = FrameShape::neutral(self.graph.compiled.value_count);
        let schema_size = |schema: SchemaId| self.graph.schemas.get(schema).map_or(0, |s| s.size);
        for (i, (decl, value)) in decls.iter().zip(values).enumerate() {
            match (decl.kind, *value) {
                (_, PlainValue::Absent) => {
                    anyhow::ensure!(
                        decl.optional,
                        "render graph: required value {} is absent",
                        decl.name
                    );
                    shape.present[i] = false;
                }
                (ValueKind::Count, PlainValue::Count(count)) => shape.counts[i] = count,
                (ValueKind::Bytes { schema }, PlainValue::Bytes(bytes)) => {
                    let size = schema_size(schema);
                    anyhow::ensure!(
                        bytes.len() == size as usize,
                        "render graph: value {} has {} bytes; its schema is {size} bytes",
                        decl.name,
                        bytes.len(),
                    );
                }
                (ValueKind::Array { elem, max_len }, PlainValue::Array { count, bytes }) => {
                    let elem_size = schema_size(elem);
                    anyhow::ensure!(
                        count <= max_len,
                        "render graph: value {} has {count} elements; at most {max_len} fit",
                        decl.name,
                    );
                    anyhow::ensure!(
                        bytes.len() as u64 == u64::from(count) * u64::from(elem_size),
                        "render graph: value {} has {} bytes for {count} elements of \
                         {elem_size} bytes",
                        decl.name,
                        bytes.len(),
                    );
                }
                (kind, value) => anyhow::bail!(
                    "render graph: value {} is declared {kind:?} but received {value:?}",
                    decl.name,
                ),
            }
        }
        for (i, decl) in decls.iter().enumerate() {
            let ValueScope::When { gate } = decl.scope else {
                continue;
            };

            let gate_present = shape.present[gate.0 as usize];
            anyhow::ensure!(
                shape.present[i] == gate_present,
                "render graph: value {} is {} but its optional scope's gate {} is {}",
                decl.name,
                presence(shape.present[i]),
                decls[gate.0 as usize].name,
                presence(gate_present),
            );
        }

        Ok(shape)
    }
}

/// Builds one frame's command batch from expanded steps.
struct FramePlanner<'a, I> {
    graph: &'a PlainRenderGraph<I>,
    phys: &'a [PhysTex],
    lookup: &'a dyn FrameLookup,
    values: &'a [PlainValue<'a>],
    shape: &'a FrameShape,
}

impl<I: IndexedIndirectArgs> FramePlanner<'_, I> {
    fn plan(&self, steps: &[ExecStep]) -> anyhow::Result<CommandBatch> {
        let mut batch = CommandBatch {
            dispatches: vec![],
            draws: vec![],
            staged: StagedWrites::default(),
            picking: None,
        };
        self.stage_uploads(&mut batch.staged)?;
        let mut uniform_blocks = HashMap::new();
        for step in steps {
            match &self.leaf(step).kind {
                CompiledLeafKind::Dispatch {
                    pipeline,
                    groups,
                    uniform,
                    asm,
                    push_asm,
                } => {
                    self.stage_uniform(
                        *uniform,
                        *asm,
                        step,
                        &mut batch.staged,
                        &mut uniform_blocks,
                    )?;
                    let push = self.push_constants(*push_asm, step)?;
                    let GroupSource::Fixed(groups) = groups else {
                        unreachable!("validation rejects per-frame group counts")
                    };
                    batch
                        .dispatches
                        .push((self.pipeline_index(*pipeline), *groups, push));
                }
                CompiledLeafKind::Raster { draws } => {
                    for draw in draws {
                        let command =
                            self.draw(draw, step, &mut batch.staged, &mut uniform_blocks)?;
                        batch.draws.push(command);
                    }
                }
            }
        }
        batch.picking = self.picking();

        Ok(batch)
    }

    fn leaf(&self, step: &ExecStep) -> &CompiledLeaf {
        let pass = &self.graph.compiled.passes[step.leaf.pass as usize];

        match pass {
            CompiledPass::Leaf(leaf) => leaf,
            CompiledPass::Repeat { body, .. } | CompiledPass::When { body, .. } => {
                &body[step.leaf.body_index as usize]
            }
        }
    }

    fn pipeline_index(&self, pipeline: PipelineId) -> usize {
        self.graph.bindings.pipelines[pipeline.0 as usize].index
    }

    fn present_bytes(&self, value: ValueId) -> anyhow::Result<&[u8]> {
        match self.values[value.0 as usize] {
            PlainValue::Bytes(bytes) => Ok(bytes),
            _ => anyhow::bail!(
                "render graph: value {} is absent but a running command reads it",
                self.graph.desc.values[value.0 as usize].name
            ),
        }
    }

    /// Whether an upload or picking readback reading `value` runs this frame:
    /// the value is present, and a repeat it sits in makes at least one trip.
    /// A repeated upload writes the same bytes each trip, so it stages once.
    fn value_runs(&self, value: ValueId) -> bool {
        let present = self.shape.present[value.0 as usize];

        match self.graph.desc.values[value.0 as usize].scope {
            ValueScope::Repeat { count } => present && self.shape.counts[count.0 as usize] > 0,
            ValueScope::Top | ValueScope::When { .. } => present,
        }
    }

    fn stage_uploads(&self, staged: &mut StagedWrites) -> anyhow::Result<()> {
        for upload in &self.graph.desc.uploads {
            if !self.value_runs(upload.value) {
                continue;
            }

            let PlainValue::Array { count, bytes } = self.values[upload.value.0 as usize] else {
                unreachable!("frame values are checked against their array kind")
            };
            let buffer = &self.graph.bindings.buffers[upload.buffer.0 as usize];
            anyhow::ensure!(
                count <= buffer.len,
                "render graph: upload {} has {count} elements; capacity is {}",
                upload.name,
                buffer.len,
            );
            let byte_range = staged.stage(bytes);
            staged.targets.push(StagedTarget::Storage {
                buffer_index: buffer.index,
                byte_range,
            });
        }

        Ok(())
    }

    /// Stages a uniform block once per frame. Every consumer's commands run
    /// after all CPU writes, so a later consumer resolving the block to other
    /// bytes would overwrite the earlier one's before it executes; validation
    /// rules that out, and this check keeps it an error rather than a
    /// silently wrong frame.
    fn stage_uniform(
        &self,
        uniform: UniformId,
        asm: AsmId,
        step: &ExecStep,
        staged: &mut StagedWrites,
        uniform_blocks: &mut HashMap<UniformId, Vec<u8>>,
    ) -> anyhow::Result<()> {
        let block = self.assemble(asm, step)?;
        if let Some(earlier) = uniform_blocks.get(&uniform) {
            anyhow::ensure!(
                *earlier == block,
                "render graph: uniform {} resolved to different bytes across commands in the \
                 same frame; a texture written between consumers must be bound through a push \
                 block",
                self.graph.desc.uniforms[uniform.0 as usize].name,
            );

            return Ok(());
        }

        let byte_range = staged.stage(&block);
        staged.targets.push(StagedTarget::Uniform {
            buffer_index: self.graph.bindings.uniforms[uniform.0 as usize].index,
            byte_range,
        });
        uniform_blocks.insert(uniform, block);

        Ok(())
    }

    fn push_constants(
        &self,
        push_asm: Option<AsmId>,
        step: &ExecStep,
    ) -> anyhow::Result<Option<PushConstantBytes>> {
        let Some(asm) = push_asm else {
            return Ok(None);
        };

        let block = self.assemble(asm, step)?;

        Ok(Some(PushConstantBytes::from_bytes(&block)))
    }

    fn draw(
        &self,
        draw: &CompiledDraw,
        step: &ExecStep,
        staged: &mut StagedWrites,
        uniform_blocks: &mut HashMap<UniformId, Vec<u8>>,
    ) -> anyhow::Result<PendingDrawCommand> {
        self.stage_uniform(draw.uniform, draw.asm, step, staged, uniform_blocks)?;
        let push_constants = self.push_constants(draw.push_asm, step)?;
        let buffers = &self.graph.bindings.buffers;
        let index_buffer_index = |id: BufferId| buffers[id.0 as usize].index;
        let draw_call = match &draw.call {
            DrawCall::VertexCount(vertex_count) => DrawCallConfig::VertexCount(*vertex_count),
            DrawCall::WholeIndexed { indices } => {
                let index_buffer = index_buffer_index(*indices);
                DrawCallConfig::Indexed {
                    index_buffer,
                    first_index: 0,
                    index_count: self.lookup.index_count(index_buffer),
                }
            }
            DrawCall::IndexRange {
                indices,
                first_index,
                index_count,
            } => {
                let index_buffer = index_buffer_index(*indices);
                let whole = self.lookup.index_count(index_buffer);
                anyhow::ensure!(
                    range_in_bounds(*first_index, *index_count, whole),
                    "render graph: index range [{first_index}, {first_index} + {index_count}) \
                     is out of bounds for index buffer {index_buffer} ({whole} indices)",
                );
                DrawCallConfig::Indexed {
                    index_buffer,
                    first_index: *first_index,
                    index_count: *index_count,
                }
            }
            DrawCall::IndexedIndirect {
                indices,
                args,
                draw_count,
            } => {
                // binding checks require an `indirect_args` slot of records `I`
                let args_index = buffers[args.buffer.0 as usize].index;
                DrawCallConfig::IndexedIndirect {
                    index_buffer: index_buffer_index(*indices),
                    request: IndirectRequest::new::<I>(
                        args_index,
                        u64::from(args.offset),
                        *draw_count,
                    ),
                }
            }
        };

        Ok(PendingDrawCommand {
            pipeline_index: self.pipeline_index(draw.pipeline),
            draw_call,
            push_constants,
        })
    }

    /// Runs one assembly program into a zeroed block of the schema's size, so
    /// padding between fields is initialized.
    fn assemble(&self, asm: AsmId, step: &ExecStep) -> anyhow::Result<Vec<u8>> {
        let compiled = &self.graph.compiled;
        let AssemblyProgram::Steps(program) = &compiled.assemblies[asm.0 as usize] else {
            unreachable!("binding checks require a field layout for every block")
        };
        let schema = compiled.assembly_schemas[asm.0 as usize];
        let size = self
            .graph
            .schemas
            .get(schema)
            .expect("validated schema ids")
            .size;

        let mut block = vec![0; size as usize];
        for assembly_step in program {
            let dst = assembly_step.dst_offset as usize;
            match &assembly_step.src {
                AssemblySrc::FrameBytes {
                    value,
                    src_offset,
                    len,
                } => {
                    let src = *src_offset as usize;
                    let len = *len as usize;
                    let bytes = self.present_bytes(*value)?;
                    block[dst..dst + len].copy_from_slice(&bytes[src..src + len]);
                }
                AssemblySrc::ResolveTex { tex, access } => {
                    let resolved = step
                        .tex
                        .iter()
                        .find(|resolved| resolved.tex == *tex && resolved.access == *access)
                        .expect("expansion resolves every texture its leaf binds");
                    let image = &self.phys[tex.0 as usize].images[resolved.phys as usize];
                    let raw = match access {
                        TexAccess::Read | TexAccess::ReadPrevious => image.sampled.to_raw(),
                        TexAccess::Write | TexAccess::Mutate => image.storage.to_raw(),
                    };
                    write_resource(&mut block, dst, raw);
                }
                AssemblySrc::ResolveBuf(buffer) => {
                    let address = self.buffer_address(buffer);
                    write_resource(&mut block, dst, address);
                }
                AssemblySrc::External(import) => {
                    let raw = self.graph.bindings.imports[import.0 as usize].to_raw();
                    write_resource(&mut block, dst, raw);
                }
            }
        }

        Ok(block)
    }

    fn buffer_address(&self, buffer: &BufferRef) -> u64 {
        let decl = &self.graph.desc.buffers[buffer.buffer.0 as usize];
        let kind = match (decl.kind, buffer.slot) {
            (BufferKind::Singleton, _) => BufferAddressKind::Singleton,
            (_, SlotSel::Previous) => BufferAddressKind::Previous,
            (_, SlotSel::Current) => BufferAddressKind::Current,
        };
        let index = self.graph.bindings.buffers[buffer.buffer.0 as usize].index;

        self.lookup.buffer_address(kind, index) + u64::from(buffer.offset)
    }

    fn picking(&self) -> Option<PickingDrawConfig> {
        let picking = self.graph.desc.picking.as_ref()?;
        if !self.value_runs(picking.cursor) {
            return None;
        }

        let PlainValue::Bytes(cursor) = self.values[picking.cursor.0 as usize] else {
            unreachable!("validation requires an 8-byte bytes cursor")
        };

        let coordinate = |start: usize| {
            f32::from_ne_bytes(cursor[start..start + 4].try_into().expect("4-byte slice"))
        };

        Some(PickingDrawConfig {
            pipeline_index: self.graph.bindings.picking?.index(),
            position: [coordinate(0), coordinate(4)],
        })
    }
}

fn presence(present: bool) -> &'static str {
    if present { "present" } else { "absent" }
}

/// Writes an 8-byte handle or address into a resource field.
fn write_resource(block: &mut [u8], dst: usize, raw: u64) {
    block[dst..dst + RESOURCE_FIELD_BYTES as usize].copy_from_slice(&raw.to_ne_bytes());
}
