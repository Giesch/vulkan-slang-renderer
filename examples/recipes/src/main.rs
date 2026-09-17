//! A compute shader solution to Advent of Code 2015 #15
//! https://adventofcode.com/2015/day/15

mod generated;

use mltrs::game::*;
use mltrs::renderer::{
    ComputeNode, DrawError, DrawVertexCountNode, FrameRenderer, PreparedRenderGraph, RenderGraph,
    Renderer, ResourcePlanner, StorageBufferHandle, StorageSlot, UniformBufferHandle, UploadNode,
    dispatch, draw_vertex_count, upload,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::recipe_render;
use crate::generated::shader_atlas::recipes_compute;
use crate::generated::shader_atlas::shared;

fn main() -> Result<(), anyhow::Error> {
    Recipes::run()
}

type RecipesGraph = PreparedRenderGraph<(
    UploadNode<shared::Solution>,
    ComputeNode<recipes_compute::IngredientParams>,
    DrawVertexCountNode<recipe_render::RenderParams>,
)>;

struct Recipes {
    graph: RecipesGraph,
    /// The graph captured these buffers' slots at build time; the handles
    /// stay here to keep the buffers alive.
    _compute_params_buffer: UniformBufferHandle<recipes_compute::IngredientParams>,
    _solution_buffer: StorageBufferHandle<shared::Solution>,
    _render_params_buffer: UniformBufferHandle<recipe_render::RenderParams>,
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

        // The same storage buffer is
        // 1. uploaded (reset to zero),
        // 2. written by the compute pass, and
        // 3. read back by the render pass
        // in that order, every frame.
        let solution = StorageSlot::from(&solution_buffer);
        let graph = RenderGraph::new(
            ResourcePlanner::new(),
            (
                upload(&solution_buffer),
                dispatch(&compute_pipeline, &compute_params_buffer, [10, 10, 10])
                    .with_param_bindings(recipes_compute::IngredientParamsBindings {
                        solution: solution.addr(),
                    }),
                draw_vertex_count(
                    &render_pipeline,
                    &render_params_buffer,
                    3,
                    recipe_render::RenderParamsBindings {
                        solution: solution.read_addr(),
                    },
                ),
            ),
        )?
        .prepare(renderer)?;

        Ok(Self {
            graph,
            _compute_params_buffer: compute_params_buffer,
            _solution_buffer: solution_buffer,
            _render_params_buffer: render_params_buffer,
        })
    }

    fn draw(&mut self, renderer: FrameRenderer) -> Result<(), DrawError> {
        let resolution = renderer.render_resolution();

        self.graph.execute(renderer, &frame_inputs(resolution))
    }
}

/// The per-frame graph inputs, in node order: an unconditional reset of the
/// solution storage before the compute pass, the fixed ingredient data, and
/// the render resolution.
fn frame_inputs(
    resolution: glam::Vec2,
) -> (
    Vec<shared::Solution>,
    recipes_compute::IngredientParamsData,
    recipe_render::RenderParamsData,
) {
    (
        // Every execution starts from a zeroed solution: the compute pass must find
        // the maximum score from scratch each frame, never accumulate onto the
        // previous frame's result.
        vec![shared::Solution { maximum_score: 0 }],
        recipes_compute::IngredientParamsData {
            weights: INGREDIENT_WEIGHTS,
            calories: CALORIES,
        },
        recipe_render::RenderParamsData { resolution },
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    /// The reset makes the compute pass re-derive the maximum score,
    /// instead of accumulating onto the previous frame's result.
    #[test]
    fn recipes_frame_input_resets_solution_each_call() {
        let first = frame_inputs(Vec2::new(640.0, 480.0));
        let second = frame_inputs(Vec2::new(800.0, 600.0));

        for (label, frame) in [("first", &first), ("second", &second)] {
            let (reset, ingredients, render) = frame;
            assert_eq!(reset.len(), 1, "{label} frame must upload one solution");
            assert_eq!(
                reset[0].maximum_score, 0,
                "{label} frame must reset the score to zero"
            );
            assert_eq!(ingredients.weights, INGREDIENT_WEIGHTS, "{label}");
            assert_eq!(ingredients.calories, CALORIES, "{label}");
            let _ = render;
        }

        // the frame-dependent resolution reaches the render params unchanged
        assert_eq!(first.2.resolution, Vec2::new(640.0, 480.0));
        assert_eq!(second.2.resolution, Vec2::new(800.0, 600.0));
    }
}
