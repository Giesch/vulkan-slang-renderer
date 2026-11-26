# Not an example: an index range past the end of its index buffer.
# `roc check` must fail at the top-level Ok destructure.
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
init! = |_| Game.Init.titled("invalid graph")

draw = |_frame| graphs.draw(zero)

vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }

(planner1, vertices) = ResourcePlanner.singleton_buffer(ResourcePlanner.empty, "vertices", BasicTriangle.Vertex.packer, [vertex, vertex, vertex])

(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)

(planner3, params) = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)

(planner4, pipeline) = ResourcePlanner.graphics_pipeline(
	planner3,
	{ name: "basic_triangle", shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default },
)

node = RenderGraph.draw_index_range(pipeline, indices, params, 2, 5, { vertices: vertices.addr() })

Ok(graphs) = RenderGraph.new(planner4, node).map_ok(GraphSet.single)

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
