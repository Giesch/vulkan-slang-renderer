use std::time::Instant;

mod generated;

use mltrs::game::*;
use mltrs::renderer::{
    DrawError, DrawVertexCountNode, FrameRenderer, PreparedRenderGraph, RenderGraph, Renderer,
    ResourcePlanner, UniformBufferHandle, draw_vertex_count,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::dragon::*;

fn main() -> Result<(), anyhow::Error> {
    Dragon::run()
}

type DragonGraph = PreparedRenderGraph<DrawVertexCountNode<DragonParams>>;

struct Dragon {
    start_time: Instant,
    graph: DragonGraph,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _params_buffer: UniformBufferHandle<DragonParams>,
}

impl Game for Dragon {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Dragon Curve"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let start_time = Instant::now();

        let params_buffer = renderer.create_uniform_buffer::<DragonParams>()?;
        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let pipeline_config = shaders.dragon.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_vertex_count(&pipeline, &params_buffer, 3, ()),
        )?
        .prepare(renderer)?;

        Ok(Self {
            start_time,
            graph,
            _params_buffer: params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();
        let resolution = renderer.window_resolution();

        let dragon_params = DragonParams {
            resolution,
            time,
            _padding_0: Default::default(),
        };

        self.graph.execute(renderer, &dragon_params)
    }
}
