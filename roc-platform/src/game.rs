use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::Context;

use mltrs::game::Game;
use mltrs::renderer::render_graph::plain::{
    PipelineKind, PlainBindings, PlainBuffer, PlainPipeline, PlainRenderGraph, PlainUniform,
    PlainValue, PreparedPlainRenderGraph,
};
use mltrs::renderer::{
    BufferBytes, Compute, ComputePipelineConfigBuilder, CpuUpdatedBufferHandle, DrawError,
    DrawIndexedIndirectCommand, FrameRenderer, GpuUpdatedBufferHandle, Graphics, GraphicsIndirect,
    ImmutableBufferHandle, IndexBufferHandle, NoPush, PipelineConfigBuilder, PipelineHandle,
    RasterState, RawUniformBufferHandle, Renderer, SingletonBufferHandle, StencilMode,
    UniformBufferHandle, UniformBytes,
};
use mltrs::shaders::atlas::NoAtlas;
use mltrs::shaders::runtime::{RuntimeComputeShader, RuntimeShader};

use crate::graph_lifecycle::GraphLifecycle;
use crate::host_graph::{
    HostBuffer, HostBufferContents, HostBufferRole, HostGraphDefinition, HostPipeline,
    HostPipelineStages, HostRasterState, HostStencilMode,
};
use crate::roc_platform_abi::{
    GameFrame, ValidatedRenderGraphHostFrameValue, ValidatedRenderGraphHostFrameValueTag,
    roc_draw_owned, roc_init_owned,
};
use crate::shaders_source_dir::resolve_shaders_source_dir;

/// What the app's `init!` returned. `Game`'s window and renderer settings are
/// associated fns, so they read it from a static set once before `run`.
struct HostInit {
    window_title: String,
    shader_hot_reload: bool,
    /// as the app wrote it; resolved once logging is up
    shaders_source_dir: String,
}

static HOST_INIT: OnceLock<HostInit> = OnceLock::new();

/// Calls the app's `init!` once and keeps its settings for `RocGame::run`.
pub fn init_from_roc() -> anyhow::Result<()> {
    // Safety: the generated wrapper owns the record and releases it on drop,
    // after its fields are copied.
    let init = unsafe { roc_init_owned() };
    let host_init = HostInit {
        window_title: init.window_title.as_str().to_owned(),
        shader_hot_reload: init.shader_hot_reload,
        shaders_source_dir: init.shaders_source_dir.as_str().to_owned(),
    };

    HOST_INIT
        .set(host_init)
        .map_err(|_| anyhow::anyhow!("the app's init! already ran"))
}

fn host_init() -> &'static HostInit {
    HOST_INIT
        .get()
        .expect("init_from_roc runs before RocGame::run")
}

pub struct RocGame {
    graphs: GraphLifecycle<PreparedHostGraph>,
    start_time: Instant,
}

impl Game for RocGame {
    type EditState = ();
    type Atlas = NoAtlas;

    fn window_title() -> &'static str {
        &host_init().window_title
    }

    fn shader_hot_reload() -> bool {
        host_init().shader_hot_reload
    }

    /// The app's dir, resolved against the working directory and then the
    /// executable's directory; empty, which leaves hot reload off, when
    /// neither holds it.
    fn shaders_source_dir() -> PathBuf {
        let dir = &host_init().shaders_source_dir;
        let bases: Vec<PathBuf> = [
            std::env::current_dir().ok(),
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(PathBuf::from)),
        ]
        .into_iter()
        .flatten()
        .collect();

        let resolved = resolve_shaders_source_dir(dir, &bases);
        let missing_configured_dir = resolved.is_none() && !dir.is_empty();
        if missing_configured_dir {
            log::warn!(
                "shaders_source_dir {dir:?} is not a directory relative to {bases:?}; \
                 shader hot reload stays off"
            );
        }

        resolved.unwrap_or_default()
    }

    fn setup(renderer: &mut Renderer, _: NoAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();
        let graphs = GraphLifecycle::setup(HostGraphDefinition::fetch_config, |definition| {
            PreparedHostGraph::prepare(renderer, definition)
        })?;

        Ok(Self { graphs, start_time })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let frame = GameFrame {
            aspect_ratio: renderer.aspect_ratio(),
            elapsed: self.start_time.elapsed().as_secs_f32(),
        };

        self.graphs.frame(
            || {
                // Safety: the generated wrapper owns the submission and
                // releases it after the frame is recorded.
                let submission = unsafe { roc_draw_owned(frame) };

                (submission.graph_id, submission)
            },
            |graph, submission| {
                let values = plain_values(submission.values.as_slice());

                graph.graph.execute(renderer, &values)
            },
        )
    }
}

/// One frame value per `ValueDecl`, borrowing Roc's packed bytes.
fn plain_values(values: &[ValidatedRenderGraphHostFrameValue]) -> Vec<PlainValue<'_>> {
    values
        .iter()
        .map(|value| match value.tag {
            ValidatedRenderGraphHostFrameValueTag::Absent => PlainValue::Absent,
            // Safety: each arm borrows the payload its tag selects.
            ValidatedRenderGraphHostFrameValueTag::Count => {
                PlainValue::Count(unsafe { *value.borrow_payload_count_unchecked() })
            }
            ValidatedRenderGraphHostFrameValueTag::Bytes => {
                PlainValue::Bytes(unsafe { value.borrow_payload_bytes_unchecked() }.as_slice())
            }
            ValidatedRenderGraphHostFrameValueTag::Array => {
                let array = unsafe { value.borrow_payload_array_unchecked() };
                PlainValue::Array {
                    count: array.count,
                    bytes: array.bytes.as_slice(),
                }
            }
        })
        .collect()
}

/// A prepared graph and every renderer resource it binds. The graph holds
/// only slots and indices, so the handles stay here for the game's lifetime.
struct PreparedHostGraph {
    graph: PreparedPlainRenderGraph<Renderer>,
    _buffers: Vec<HostBufferHandle>,
    _uniform_buffers: Vec<UniformBufferHandle<UniformBytes>>,
    _pipelines: Vec<HostPipelineHandle>,
}

enum HostBufferHandle {
    CpuUpdated(CpuUpdatedBufferHandle<BufferBytes>),
    GpuUpdated(GpuUpdatedBufferHandle<BufferBytes>),
    Immutable(ImmutableBufferHandle<BufferBytes>),
    Singleton(SingletonBufferHandle<BufferBytes>),
    Index(IndexBufferHandle),
    IndirectArgs(ImmutableBufferHandle<DrawIndexedIndirectCommand>),
}

impl HostBufferHandle {
    fn binding(&self) -> PlainBuffer<DrawIndexedIndirectCommand> {
        match self {
            Self::CpuUpdated(handle) => PlainBuffer::cpu_updated(handle),
            Self::GpuUpdated(handle) => PlainBuffer::gpu_updated(handle),
            Self::Immutable(handle) => PlainBuffer::immutable(handle),
            Self::Singleton(handle) => PlainBuffer::singleton(handle),
            Self::Index(handle) => PlainBuffer::index(handle),
            Self::IndirectArgs(handle) => PlainBuffer::indirect_args(handle),
        }
    }
}

/// Runtime pipelines take `NoPush`: the renderer's push marker is phantom,
/// and the plain graph reads each push block's layout from its description.
enum HostPipelineHandle {
    Graphics(PipelineHandle<Graphics, NoPush>),
    GraphicsIndirect(PipelineHandle<GraphicsIndirect, NoPush>),
    Compute(PipelineHandle<Compute, NoPush>),
}

impl HostPipelineHandle {
    fn binding(&self) -> PlainPipeline {
        match self {
            Self::Graphics(handle) => PlainPipeline::graphics(handle),
            Self::GraphicsIndirect(handle) => PlainPipeline::graphics_indirect(handle),
            Self::Compute(handle) => PlainPipeline::compute(handle),
        }
    }
}

impl PreparedHostGraph {
    fn prepare(renderer: &mut Renderer, definition: HostGraphDefinition) -> anyhow::Result<Self> {
        anyhow::ensure!(
            definition.import_names.is_empty(),
            "imported external textures are not yet wired in the Roc host (imports: {:?})",
            definition.import_names
        );

        let buffers = definition
            .buffers
            .iter()
            .map(|buffer| {
                create_buffer(renderer, buffer).with_context(|| format!("buffer {}", buffer.name))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let uniform_buffers = definition
            .uniform_buffers
            .iter()
            .map(|uniform| {
                renderer
                    .create_uniform_buffer_bytes(u64::from(uniform.size))
                    .with_context(|| format!("uniform buffer {}", uniform.name))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let pipelines = definition
            .pipelines
            .iter()
            .map(|pipeline| {
                create_pipeline(renderer, pipeline, &uniform_buffers)
                    .with_context(|| format!("pipeline {}", pipeline.name))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let uniforms = definition
            .uniform_slots
            .iter()
            .map(|&slot| {
                uniform_buffers
                    .get(slot as usize)
                    .map(PlainUniform::new)
                    .with_context(|| format!("a uniform writes unknown uniform buffer {slot}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let pipeline_bindings = definition
            .pipeline_indices
            .iter()
            .map(|&index| {
                pipelines
                    .get(index as usize)
                    .map(HostPipelineHandle::binding)
                    .with_context(|| format!("the description names unknown pipeline {index}"))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let bindings = PlainBindings {
            buffers: buffers.iter().map(HostBufferHandle::binding).collect(),
            uniforms,
            pipelines: pipeline_bindings,
            imports: vec![],
            picking: None,
        };
        let graph = PlainRenderGraph::new(definition.desc, definition.schemas, bindings)?
            .prepare(renderer)?;

        Ok(Self {
            graph,
            _buffers: buffers,
            _uniform_buffers: uniform_buffers,
            _pipelines: pipelines,
        })
    }
}

fn create_buffer(renderer: &mut Renderer, buffer: &HostBuffer) -> anyhow::Result<HostBufferHandle> {
    let element_size = buffer.element_size;
    let handle = match (buffer.role, &buffer.contents) {
        (HostBufferRole::Singleton, HostBufferContents::Bytes(bytes)) => {
            HostBufferHandle::Singleton(
                renderer.create_singleton_buffer_bytes(bytes, element_size)?,
            )
        }
        (HostBufferRole::Immutable, HostBufferContents::Bytes(bytes)) => {
            HostBufferHandle::Immutable(
                renderer.create_immutable_buffer_bytes(bytes, element_size)?,
            )
        }
        (HostBufferRole::IndirectArgs, HostBufferContents::Bytes(bytes)) => {
            let commands = indirect_commands(bytes)?;
            HostBufferHandle::IndirectArgs(renderer.create_indirect_buffer(&commands)?)
        }
        (HostBufferRole::CpuUpdated, HostBufferContents::Bytes(bytes)) => {
            let mut handle = renderer.create_cpu_updated_buffer_bytes(buffer.len, element_size)?;
            if !bytes.is_empty() {
                renderer.write_cpu_updated_bytes_all_frames(&mut handle, bytes)?;
            }

            HostBufferHandle::CpuUpdated(handle)
        }
        (HostBufferRole::GpuUpdated, HostBufferContents::Bytes(bytes)) => {
            let mut handle = renderer.create_gpu_updated_buffer_bytes(buffer.len, element_size)?;
            if !bytes.is_empty() {
                renderer.write_gpu_updated_bytes_all_frames(&mut handle, bytes)?;
            }

            HostBufferHandle::GpuUpdated(handle)
        }
        (
            HostBufferRole::Index,
            HostBufferContents::Indices {
                indices,
                vertex_count,
            },
        ) => HostBufferHandle::Index(renderer.create_index_buffer(indices, *vertex_count)?),
        (role, _) => anyhow::bail!("a {role:?} buffer carries the other kind of contents"),
    };

    Ok(handle)
}

/// Decodes packed `vkCmdDrawIndexedIndirect` records, in the field order of
/// `DrawIndexedIndirectCommand`.
fn indirect_commands(bytes: &[u8]) -> anyhow::Result<Vec<DrawIndexedIndirectCommand>> {
    const RECORD_SIZE: usize = size_of::<DrawIndexedIndirectCommand>();
    let (records, remainder) = bytes.as_chunks::<RECORD_SIZE>();
    anyhow::ensure!(
        remainder.is_empty(),
        "{} bytes is not a whole number of {RECORD_SIZE}-byte indirect records",
        bytes.len()
    );

    let commands = records
        .iter()
        .map(|record| {
            let word = |i: usize| -> [u8; 4] {
                record[i * 4..i * 4 + 4]
                    .try_into()
                    .expect("a record holds five 4-byte words")
            };

            DrawIndexedIndirectCommand {
                index_count: u32::from_le_bytes(word(0)),
                instance_count: u32::from_le_bytes(word(1)),
                first_index: u32::from_le_bytes(word(2)),
                vertex_offset: i32::from_le_bytes(word(3)),
                first_instance: u32::from_le_bytes(word(4)),
            }
        })
        .collect();

    Ok(commands)
}

fn create_pipeline(
    renderer: &mut Renderer,
    pipeline: &HostPipeline,
    uniform_buffers: &[UniformBufferHandle<UniformBytes>],
) -> anyhow::Result<HostPipelineHandle> {
    let uniform_buffer_handles = pipeline
        .uniform_buffers
        .iter()
        .map(|&index| {
            uniform_buffers
                .get(index as usize)
                .map(RawUniformBufferHandle::from_typed)
                .with_context(|| format!("binds unknown uniform buffer {index}"))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let handle = match (&pipeline.stages, pipeline.kind) {
        (
            HostPipelineStages::Graphics {
                vertex_spv,
                fragment_spv,
                raster,
            },
            kind @ (PipelineKind::Graphics | PipelineKind::GraphicsIndirect),
        ) => {
            let shader = RuntimeShader::new(&pipeline.reflection_json, vertex_spv, fragment_spv)?;
            let builder = PipelineConfigBuilder {
                shader: Box::new(shader),
                texture_handles: vec![],
                uniform_buffer_handles,
                storage_texture_handles: vec![],
            };
            let raster_state = raster_state(renderer, raster)?;

            if kind == PipelineKind::Graphics {
                let config = builder
                    .build_graphics::<NoPush>()
                    .with_raster_state(raster_state);
                HostPipelineHandle::Graphics(renderer.create_pipeline(config)?)
            } else {
                let config = builder
                    .build_graphics_indirect::<NoPush>()
                    .with_raster_state(raster_state);
                HostPipelineHandle::GraphicsIndirect(renderer.create_pipeline(config)?)
            }
        }
        (HostPipelineStages::Compute { compute_spv }, PipelineKind::Compute) => {
            let shader = RuntimeComputeShader::new(&pipeline.reflection_json, compute_spv)?;
            let config = ComputePipelineConfigBuilder {
                shader: Box::new(shader),
                texture_handles: vec![],
                uniform_buffer_handles,
                storage_texture_handles: vec![],
            }
            .build::<NoPush>();

            HostPipelineHandle::Compute(renderer.create_compute_pipeline(config)?)
        }
        (_, kind) => anyhow::bail!("a {kind:?} pipeline carries the other kind of stages"),
    };

    Ok(handle)
}

fn raster_state(renderer: &Renderer, raster: &HostRasterState) -> anyhow::Result<RasterState> {
    let stencil = match raster.stencil {
        HostStencilMode::Disabled => StencilMode::DISABLED,
        HostStencilMode::Write(reference) => stencil_support(renderer)?.write(reference),
        HostStencilMode::TestEqual(reference) => stencil_support(renderer)?.test_equal(reference),
    };

    Ok(RasterState {
        blend: raster.blend,
        cull: raster.cull,
        depth_test: raster.depth_test,
        depth_write: raster.depth_write,
        color_write: raster.color_write,
        stencil,
    })
}

fn stencil_support(renderer: &Renderer) -> anyhow::Result<mltrs::renderer::StencilSupport> {
    renderer
        .stencil_support()
        .context("a stencil raster state needs a stencil-capable depth buffer, which the Roc host does not request")
}
