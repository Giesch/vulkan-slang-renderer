use std::time::Instant;

mod generated;

use facet::Facet;
use glam::Vec2;

use mltrs::editor::Slider;
use mltrs::game::{Game, Input, MouseButton};
use mltrs::ktx::load_ktx2_texture;
use mltrs::manifest_path;
use mltrs::renderer::{
    DrawError, DrawVertexCountNode, FrameRenderer, PreparedRenderGraph, RenderGraph, Renderer,
    ResourcePlanner, TextureFilter, TextureHandle, UniformBufferHandle, draw_vertex_count,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::koch_curve::*;

fn main() -> Result<(), anyhow::Error> {
    KochCurve::run()
}

#[derive(Facet)]
pub struct EditState {
    pub koch_iterations: Slider,
    pub scale_factor: Slider,
    pub sphere_radius: Slider,
    pub sphere_blend: Slider,
    pub rotation_speed: Slider,
}

type KochCurveGraph = PreparedRenderGraph<DrawVertexCountNode<KochCurveParams>>;

pub struct KochCurve {
    start_time: Instant,
    edit_state: EditState,
    graph: KochCurveGraph,
    /// Bound into the graph at build time; kept here for ownership.
    _reflection_map: TextureHandle,
    /// The graph captured this buffer's slot at build time; the handle stays
    /// here to keep the buffer alive.
    _params_buffer: UniformBufferHandle<KochCurveParams>,
    mouse_down: bool,
    mouse_position: Vec2,
}

impl Game for KochCurve {
    type EditState = EditState;
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Koch Curve 3D"
    }

    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Koch Curve 3D", &mut self.edit_state))
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        const IMAGE_FILE_NAME: &str = "istockphoto-uffizi-blurred-612x612.ktx2";
        let file_path = manifest_path!["textures", IMAGE_FILE_NAME];
        let reflection_map = load_ktx2_texture(renderer, &file_path, TextureFilter::Linear)?;

        let params_buffer = renderer.create_uniform_buffer::<KochCurveParams>()?;

        let resources = Resources {
            params_buffer: &params_buffer,
        };

        let pipeline_config = shaders.koch_curve.pipeline_config(resources);
        let pipeline = renderer.create_pipeline(pipeline_config)?;

        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            draw_vertex_count(
                &pipeline,
                &params_buffer,
                3,
                KochCurveParamsBindings {
                    reflection_map: reflection_map.bindless_handle().into(),
                },
            ),
        )?
        .prepare(renderer)?;

        let edit_state = EditState {
            koch_iterations: Slider::new(4.0, 1.0, 8.0),
            scale_factor: Slider::new(3.0, 1.5, 5.0),
            sphere_radius: Slider::new(0.5, 0.1, 2.0),
            sphere_blend: Slider::new(0.5, 0.0, 1.0),
            rotation_speed: Slider::new(0.2, 0.0, 1.0),
        };

        Ok(Self {
            start_time: Instant::now(),
            edit_state,
            graph,
            _reflection_map: reflection_map,
            _params_buffer: params_buffer,
            mouse_down: false,
            mouse_position: Vec2::ZERO,
        })
    }

    fn input(&mut self, input: Input) {
        match input {
            Input::MouseDown { button, x, y } => {
                if button == MouseButton::Left {
                    self.mouse_down = true;
                    self.mouse_position = Vec2::new(x, y);
                }
            }

            Input::MouseUp { button, .. } => {
                if button == MouseButton::Left {
                    self.mouse_down = false;
                }
            }

            Input::MouseMotion { x, y } if self.mouse_down => {
                self.mouse_position = Vec2::new(x, y);
            }

            _ => {}
        }
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let time = (Instant::now() - self.start_time).as_secs_f32();

        let resolution = renderer.window_resolution();
        let mut mouse = self.mouse_position;
        mouse.y = resolution.y - mouse.y;

        self.graph.execute(
            renderer,
            &KochCurveParamsData {
                resolution,
                mouse,
                time,
                koch_iterations: self.edit_state.koch_iterations.value,
                scale_factor: self.edit_state.scale_factor.value,
                sphere_radius: self.edit_state.sphere_radius.value,
                sphere_blend: self.edit_state.sphere_blend.value,
                rotation_speed: self.edit_state.rotation_speed.value,
            },
        )
    }
}
