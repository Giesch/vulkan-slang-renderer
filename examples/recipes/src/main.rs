//! A compute shader solution to Advent of Code 2015 #15
//! https://adventofcode.com/2015/day/15

mod generated;

use mltrs::game::*;
use mltrs::renderer::{
    Compute, DrawError, DrawVertexCount, FrameRenderer, PipelineHandle, Renderer,
    StorageBufferHandle, UniformBufferHandle,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::recipe_render;
use crate::generated::shader_atlas::recipes_compute;
use crate::generated::shader_atlas::shared;

fn main() -> Result<(), anyhow::Error> {
    Recipes::run()
}

struct Recipes {
    compute_pipeline: PipelineHandle<Compute>,
    compute_params_buffer: UniformBufferHandle<recipes_compute::IngredientParams>,
    solution_buffer: StorageBufferHandle<shared::Solution>,
    render_pipeline: PipelineHandle<DrawVertexCount>,
    render_params_buffer: UniformBufferHandle<recipe_render::RenderParams>,
}

impl Game for Recipes {
    type EditState = ();
    type Atlas = ShaderAtlas;

    fn window_title() -> &'static str {
        "Advent of Code 2015 #15"
    }

    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        let mut solution_buffer = renderer.create_storage_buffer::<shared::Solution>(1)?;
        renderer.write_storage_all_frames(
            &mut solution_buffer,
            &[shared::Solution { maximum_score: 0 }],
        );
        let compute_params_buffer =
            renderer.create_uniform_buffer::<recipes_compute::IngredientParams>()?;
        let compute_resources = recipes_compute::Resources {
            params_buffer: &compute_params_buffer,
        };
        let compute_config = shaders.recipes_compute.pipeline_config(compute_resources);
        let compute_pipeline = renderer.create_compute_pipeline(compute_config)?;

        let render_params_buffer =
            renderer.create_uniform_buffer::<recipe_render::RenderParams>()?;
        let render_resources = recipe_render::Resources {
            params_buffer: &render_params_buffer,
        };
        let render_config = shaders.recipe_render.pipeline_config(render_resources);
        let render_pipeline = renderer.create_pipeline(render_config)?;

        Ok(Self {
            compute_pipeline,
            compute_params_buffer,
            solution_buffer,
            render_pipeline,
            render_params_buffer,
        })
    }

    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let resolution = renderer.render_resolution();

        renderer.dispatch(&self.compute_pipeline, [10, 10, 10]);

        renderer.draw_vertex_count(&self.render_pipeline, 3, |gpu| {
            let compute_params = recipes_compute::IngredientParams {
                weights: INGREDIENT_WEIGHTS,
                calories: CALORIES,
                solution: gpu.addr(&self.solution_buffer),
                _padding_0: Default::default(),
            };
            gpu.write_uniform(&mut self.compute_params_buffer, compute_params);
            gpu.write_storage(
                &mut self.solution_buffer,
                &[shared::Solution { maximum_score: 0 }],
            );

            let render_params = recipe_render::RenderParams {
                solution: gpu.addr(&self.solution_buffer).into(),
                resolution,
            };
            gpu.write_uniform(&mut self.render_params_buffer, render_params);
        })?;

        Ok(())
    }
}

// Butterscotch: capacity -1, durability -2, flavor 6, texture 3, calories 8
// Cinnamon: capacity 2, durability 3, flavor -2, texture -1, calories 3
#[expect(unused)]
const EXAMPLE_WEIGHTS: [glam::IVec4; 4] = [
    glam::IVec4::new(-1, -2, 6, 3),
    glam::IVec4::new(2, 3, -2, -1),
    glam::IVec4::splat(0),
    glam::IVec4::splat(0),
];
#[expect(unused)]
const EXAMPLE_CALORIES: glam::IVec4 = glam::IVec4::new(8, 3, 0, 0);

// Frosting: capacity 4, durability -2, flavor 0, texture 0, calories 5
// Candy: capacity 0, durability 5, flavor -1, texture 0, calories 8
// Butterscotch: capacity -1, durability 0, flavor 5, texture 0, calories 6
// Sugar: capacity 0, durability 0, flavor -2, texture 2, calories 1
const INGREDIENT_WEIGHTS: [glam::IVec4; 4] = [
    glam::IVec4::new(4, -2, 0, 0),
    glam::IVec4::new(0, 5, -1, 0),
    glam::IVec4::new(-1, 0, 5, 0),
    glam::IVec4::new(0, 0, -2, 2),
];
const CALORIES: glam::IVec4 = glam::IVec4::new(5, 8, 6, 1);
