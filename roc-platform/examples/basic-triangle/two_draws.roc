# Not an example: two draws of the triangle, built from a tuple of render
# graphs. `roc test` runs its expectations; `roc check` must accept it.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner
import pf.ShaderReflection exposing [Float4x4]

import Generated/BasicTriangle

game : Game
game = Game.new({ init!, graphs, draw })

init! : {} => Game.Init
init! = |_| Game.Init.titled("two draws")

draw = |_frame| two_draws(filled(0.0), filled(1.0))

two_draws : BasicTriangle.TriangleParamsData, BasicTriangle.TriangleParamsData -> GraphSet.Submission(_)
two_draws = |first_params, second_params| graphs.draw((first_params, second_params))

vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }

(planner1, vertices) = ResourcePlanner.singleton_buffer(ResourcePlanner.empty, "vertices", BasicTriangle.Vertex.packer, [vertex, vertex, vertex])

(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)

## Declares a uniform buffer and a pipeline that reads it, and draws the
## triangle with them, so each draw takes its own frame value.
draw_triangle : ResourcePlanner, Str -> (ResourcePlanner, RenderGraph(BasicTriangle.TriangleParamsData))
draw_triangle = |planner, name| {
	(with_params, params) = ResourcePlanner.uniform_buffer(planner, name, BasicTriangle.params)
	(with_pipeline, pipeline) = ResourcePlanner.graphics_pipeline(
		with_params,
		{ name, shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default },
	)

	(with_pipeline, RenderGraph.draw_indexed(pipeline, indices, params, { vertices: vertices.addr() }))
}

(planner3, first) = draw_triangle(planner2, "first")

(planner4, second) = draw_triangle(planner3, "second")

Ok(graphs) = RenderGraph.new(planner4, RenderGraph.from_tuple_2((first, second))).map_ok(GraphSet.single)

filled : F32 -> BasicTriangle.TriangleParamsData
filled = |value| {
	matrix : Float4x4
	matrix = {
		row_0: { x: value, y: value, z: value, w: value },
		row_1: { x: value, y: value, z: value, w: value },
		row_2: { x: value, y: value, z: value, w: value },
		row_3: { x: value, y: value, z: value, w: value },
	}

	{ mvp: { model: matrix, view: matrix, proj: matrix } }
}

## Each tuple position supplies its own draw's value, in declaration order.
expect
	two_draws(filled(0.0), filled(1.0)).to_host()
		== { graph_id: 0, values: [Bytes(filled(0.0).to_bytes()), Bytes(filled(1.0).to_bytes())] }

expect two_draws(filled(0.0), filled(1.0)).to_host() != two_draws(filled(1.0), filled(0.0)).to_host()

expect
	match Game.host_config(game).graphs {
		[host] => host.pipelines.len() == 2 and host.uniform_buffers.len() == 2 and host.buffers.len() == 2
		_ => Bool.False
	}

## An index range past the end of its index buffer is rejected with the
## validator's message.
expect {
	(range_planner, params) = ResourcePlanner.uniform_buffer(planner2, "range", BasicTriangle.params)
	(range_pipeline_planner, pipeline) = ResourcePlanner.graphics_pipeline(
		range_planner,
		{ name: "range", shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default },
	)
	node = RenderGraph.draw_index_range(pipeline, indices, params, 2, 5, { vertices: vertices.addr() })

	match RenderGraph.new(range_pipeline_planner, node) {
		Err(InvalidRenderGraph(message)) => message.contains("index range [2, 2 + 5) is out of bounds for an index buffer of 3 indices")
		Ok(_) => Bool.False
	}
}

## An index at or past the vertex count is rejected with the validator's
## message, naming the buffer, position, index, and vertex count.
expect {
	(bounds_planner, out_of_bounds) = ResourcePlanner.index_buffer(planner1, "out_of_bounds", [0, 1, 3], 3)
	(bounds_params_planner, params) = ResourcePlanner.uniform_buffer(bounds_planner, "bounds", BasicTriangle.params)
	(bounds_pipeline_planner, pipeline) = ResourcePlanner.graphics_pipeline(
		bounds_params_planner,
		{ name: "bounds", shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default },
	)
	node = RenderGraph.draw_indexed(pipeline, out_of_bounds, params, { vertices: vertices.addr() })

	match RenderGraph.new(bounds_pipeline_planner, node) {
		Err(InvalidRenderGraph(message)) => message.contains("index buffer out_of_bounds index 3 at position 2 is not below its vertex count 3")
		Ok(_) => Bool.False
	}
}
