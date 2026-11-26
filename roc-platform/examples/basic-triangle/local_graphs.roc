# Not an example: `draw` selects from a set of three graphs with inferred
# types. `roc test` runs its expectations; `roc check` must accept it.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner
import pf.ShaderReflection exposing [Float4x4]

import Generated/BasicTriangle

game : Game
game = Game.new({ init!, graphs: local, draw })

init! : {} => Game.Init
init! = |_| Game.Init.titled("local graphs")

## Two graphs with one frame type and one with another, registered by name.
local =
	GraphSet.map2(GraphSet.map2(graphs, graphs, |first, second| { first, second }), single, |pair, one| { pair, one })

draw_first = local.select(|registered| registered.pair.first)

draw_second = local.select(|registered| registered.pair.second)

draw_one = local.select(|registered| registered.one)

draw = |frame| {
	if frame.aspect_ratio < 1.0 {
		draw_one.draw(filled(0.0))
	} else {
		graph = if (frame.aspect_ratio > 1.0) draw_second else draw_first

		graph.draw((filled(0.0), filled(1.0)))
	}
}

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

(planner5, third) = draw_triangle(planner4, "third")

Ok(single) = RenderGraph.new(planner5, third).map_ok(GraphSet.single)

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

expect Game.host_config(game).graphs.len() == 3

## Both branches produce the registered set's type. Distinct graph ids
## survive type erasure even when the values are identical.
expect Game.draw(game, { aspect_ratio: 2.0, elapsed: 0.0 }) != Game.draw(game, { aspect_ratio: 1.0, elapsed: 0.0 })

expect Game.draw(game, { aspect_ratio: 2.0, elapsed: 0.0 }) == draw_second.draw((filled(0.0), filled(1.0))).to_host()

expect Game.draw(game, { aspect_ratio: 2.0, elapsed: 0.0 }).graph_id == 1

## A different frame type still returns the same set's submission.
expect Game.draw(game, { aspect_ratio: 0.5, elapsed: 0.0 }) == { graph_id: 2, values: [Bytes(filled(0.0).to_bytes())] }
