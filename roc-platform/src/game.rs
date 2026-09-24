use std::sync::OnceLock;

use anyhow::Context;

use mltrs::game::Game;
use mltrs::renderer::render_graph::{
    DynDrawNode, PreparedRenderGraph, RenderGraph, ResourcePlanner,
};
use mltrs::renderer::vertex_description::VertexBytes;
use mltrs::renderer::{
    DrawError, DrawIndexed, DrawVertexCount, FrameRenderer, NoPush, PipelineConfigBuilder,
    PipelineHandle, RawUniformBufferHandle, Renderer, UniformBufferHandle, UniformBytes,
};
use mltrs::shaders::atlas::NoAtlas;
use mltrs::shaders::runtime::RuntimeShader;

use crate::roc_platform_abi::{
    GameFrame, ValidatedRenderGraphHostDrawCallTag, ValidatedRenderGraphHostGraph,
    ValidatedRenderGraphHostMeshTag, roc_config_owned, roc_draw_owned,
};

/// `Game::window_title` is an associated fn returning `&'static str`, so the
/// title Roc supplies cannot be threaded through an argument. A `OnceLock` in a
/// static outlives the process, so borrowing from it yields a real `&'static str`.
static WINDOW_TITLE: OnceLock<String> = OnceLock::new();

const FALLBACK_WINDOW_TITLE: &str = "mltrs";

/// Returns `Err` with the unused title if a title was already set.
pub fn set_window_title(title: String) -> Result<(), String> {
    WINDOW_TITLE.set(title)
}

/// One configured graph, copied once from Roc during setup.
struct GraphDefinition {
    pipelines: Vec<PipelineDecl>,
    uniforms: Vec<UniformDecl>,
    draws: Vec<DrawDecl>,
}

struct FrameSubmission {
    graph_id: u32,
    values: Vec<Vec<u8>>,
}

impl FrameSubmission {
    fn fetch(aspect_ratio: f32) -> Self {
        // Safety: the generated wrapper owns the returned Roc allocations.
        let owned = unsafe { roc_draw_owned(GameFrame { aspect_ratio }) };

        Self {
            graph_id: owned.graph_id,
            values: owned
                .values
                .as_slice()
                .iter()
                .map(|bytes| bytes.as_slice().to_vec())
                .collect(),
        }
    }
}

struct PipelineDecl {
    name: String,
    vertex_spv: Vec<u8>,
    fragment_spv: Vec<u8>,
    reflection_json: String,
    mesh: MeshDecl,
    /// Indices into `GraphDefinition::uniforms`, in descriptor-set-layout order.
    uniforms: Vec<u32>,
}

enum MeshDecl {
    Indexed {
        vertex_bytes: Vec<u8>,
        indices: Vec<u32>,
    },
    VertexCount,
}

struct UniformDecl {
    name: String,
    size: u32,
}

struct DrawDecl {
    pipeline: u32,
    uniform: u32,
    call: DrawCall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawCall {
    WholeIndexed,
    VertexCount(u32),
}

impl GraphDefinition {
    fn fetch_config() -> Vec<Self> {
        // Safety: the generated wrapper owns the config until all data is copied.
        let owned = unsafe { roc_config_owned() };
        owned
            .graphs
            .as_slice()
            .iter()
            .map(Self::copy_from_roc)
            .collect()
    }

    fn copy_from_roc(draw: &ValidatedRenderGraphHostGraph) -> Self {
        let pipelines = draw
            .pipelines
            .as_slice()
            .iter()
            .map(|pipeline| PipelineDecl {
                name: pipeline.name.as_str().to_owned(),
                vertex_spv: pipeline.vertex_spv.as_slice().to_vec(),
                fragment_spv: pipeline.fragment_spv.as_slice().to_vec(),
                reflection_json: pipeline.reflection_json.as_str().to_owned(),
                mesh: match pipeline.mesh.tag {
                    ValidatedRenderGraphHostMeshTag::Indexed => {
                        // Safety: the tag selects the payload variant.
                        let mesh = unsafe { pipeline.mesh.borrow_payload_indexed_unchecked() };
                        MeshDecl::Indexed {
                            vertex_bytes: mesh.vertex_bytes.as_slice().to_vec(),
                            indices: mesh.indices.as_slice().to_vec(),
                        }
                    }
                    ValidatedRenderGraphHostMeshTag::VertexCount => MeshDecl::VertexCount,
                },
                uniforms: pipeline.uniforms.as_slice().to_vec(),
            })
            .collect();

        let uniforms = draw
            .uniforms
            .as_slice()
            .iter()
            .map(|uniform| UniformDecl {
                name: uniform.name.as_str().to_owned(),
                size: uniform.size,
            })
            .collect();

        let draws = draw
            .draws
            .as_slice()
            .iter()
            .map(|draw| DrawDecl {
                pipeline: draw.pipeline,
                uniform: draw.uniform,
                call: match draw.call.tag {
                    ValidatedRenderGraphHostDrawCallTag::WholeIndexed => DrawCall::WholeIndexed,
                    ValidatedRenderGraphHostDrawCallTag::VertexCount => {
                        // Safety: the tag selects the payload variant.
                        DrawCall::VertexCount(unsafe {
                            *draw.call.borrow_payload_vertex_count_unchecked()
                        })
                    }
                },
            })
            .collect();

        Self {
            pipelines,
            uniforms,
            draws,
        }
    }
}

/// A live pipeline, by family, as an index into the matching handle list.
#[derive(Clone, Copy)]
enum LivePipeline {
    Indexed(usize),
    VertexCount(usize),
}

pub struct RocGame {
    graphs: crate::graph_lifecycle::GraphLifecycle<PreparedGraph>,
}

struct PreparedGraph {
    graph: PreparedRenderGraph<Vec<DynDrawNode>>,
    /// The graph captured these handles' slots and indices at build time;
    /// they stay here to keep the resources alive.
    _uniforms: Vec<UniformBufferHandle<UniformBytes>>,
    _indexed: Vec<PipelineHandle<DrawIndexed>>,
    _vertex_count: Vec<PipelineHandle<DrawVertexCount>>,
}

impl Game for RocGame {
    type EditState = ();
    type Atlas = NoAtlas;

    fn window_title() -> &'static str {
        WINDOW_TITLE
            .get()
            .map(String::as_str)
            .unwrap_or(FALLBACK_WINDOW_TITLE)
    }

    fn setup(renderer: &mut Renderer, _: NoAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let graphs = crate::graph_lifecycle::GraphLifecycle::setup(
            GraphDefinition::fetch_config,
            |definition| PreparedGraph::prepare(renderer, definition),
        )?;
        Ok(Self { graphs })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let aspect_ratio = renderer.aspect_ratio();
        self.graphs.frame(
            || {
                let submission = FrameSubmission::fetch(aspect_ratio);
                (submission.graph_id, submission.values)
            },
            |graph, values| graph.graph.execute(renderer, &values),
        )
    }
}

impl PreparedGraph {
    fn prepare(renderer: &mut Renderer, bundle: GraphDefinition) -> anyhow::Result<Self> {
        let uniforms = bundle
            .uniforms
            .iter()
            .map(|uniform| {
                renderer
                    .create_uniform_buffer_bytes(u64::from(uniform.size))
                    .with_context(|| format!("uniform {}", uniform.name))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let mut indexed = vec![];
        let mut vertex_count = vec![];
        let mut live = vec![];
        for pipeline in &bundle.pipelines {
            let shader = RuntimeShader::new(
                &pipeline.reflection_json,
                &pipeline.vertex_spv,
                &pipeline.fragment_spv,
            )
            .with_context(|| format!("pipeline {}", pipeline.name))?;

            let uniform_buffer_handles = pipeline
                .uniforms
                .iter()
                .map(|&index| {
                    uniforms
                        .get(index as usize)
                        .map(RawUniformBufferHandle::from_typed)
                        .with_context(|| {
                            format!("pipeline {} binds unknown uniform {index}", pipeline.name)
                        })
                })
                .collect::<anyhow::Result<Vec<_>>>()?;

            let builder = PipelineConfigBuilder {
                shader: Box::new(shader),
                texture_handles: vec![],
                uniform_buffer_handles,
                storage_texture_handles: vec![],
            };

            match &pipeline.mesh {
                MeshDecl::Indexed {
                    vertex_bytes,
                    indices,
                } => {
                    let config = builder
                        .build_indexed::<VertexBytes, NoPush>()
                        .with_vertex_bytes(vertex_bytes.clone(), indices.clone());
                    live.push(LivePipeline::Indexed(indexed.len()));
                    indexed.push(renderer.create_pipeline(config)?);
                }

                MeshDecl::VertexCount => {
                    let config = builder.build_vertex_count::<NoPush>();
                    live.push(LivePipeline::VertexCount(vertex_count.len()));
                    vertex_count.push(renderer.create_pipeline(config)?);
                }
            }
        }

        let nodes = bundle
            .draws
            .iter()
            .enumerate()
            .map(|(index, draw)| {
                let uniform = uniforms
                    .get(draw.uniform as usize)
                    .with_context(|| format!("draw {index} reads unknown uniform"))?;
                let size = bundle.uniforms[draw.uniform as usize].size;
                let pipeline = live
                    .get(draw.pipeline as usize)
                    .with_context(|| format!("draw {index} uses unknown pipeline"))?;
                match (pipeline, draw.call) {
                    (LivePipeline::Indexed(i), DrawCall::WholeIndexed) => {
                        Ok(DynDrawNode::indexed(&indexed[*i], uniform, size))
                    }
                    (LivePipeline::VertexCount(i), DrawCall::VertexCount(count)) => Ok(
                        DynDrawNode::vertex_count(&vertex_count[*i], uniform, size, count),
                    ),
                    _ => anyhow::bail!("draw {index}'s call does not match its pipeline family"),
                }
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let graph = RenderGraph::new(ResourcePlanner::new(), nodes)?.prepare(renderer)?;

        Ok(Self {
            graph,
            _uniforms: uniforms,
            _indexed: indexed,
            _vertex_count: vertex_count,
        })
    }
}
