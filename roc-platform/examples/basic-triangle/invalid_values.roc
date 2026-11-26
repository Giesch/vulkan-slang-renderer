# Not an example: a two-draw graph drawn with a value for one draw only.
# `roc check` must fail with a type mismatch at the `draw` call.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner
import pf.ShaderReflection exposing [Float4x4]

import Generated/BasicTriangle

G : GraphSet.ValidatedGraph((BasicTriangle.TriangleParamsData, BasicTriangle.TriangleParamsData))

game : Game
game = Game.new({ init!, graphs, draw })

init! : {} => Game.Init
init! = |_| Game.Init.titled("invalid values")

draw : Game.Frame -> GraphSet.Submission(G)
draw = |_frame| graphs.draw(zero)

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

zero : BasicTriangle.TriangleParamsData
zero = {
	matrix : Float4x4
	matrix = {
		row_0: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
		row_1: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
		row_2: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
		row_3: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	}

	{ mvp: { model: matrix, view: matrix, proj: matrix } }
}
