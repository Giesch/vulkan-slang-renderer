# Not an example: a two-node graph drawn with a mistyped tuple element.
# `roc check` must fail with a type mismatch at the `draw` call.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import pf.ShaderReflection
import Generated/BasicTriangle
import Generated/Mltrs

G : Graphs.ValidatedGraph((Mltrs.MvpMatrices, Mltrs.MvpMatrices))

game : Game
game = Game.new({ init!, draw, graphs })

graphs = Graphs.or_crash(Graphs.single(blueprint))

init! : {} => Game.Init
init! = |_| { window_title: "invalid values" }

draw : Game.Frame -> Graphs.Submission(G)
draw = |_frame| graphs.draw((zero, "wrong"))

vertex : BasicTriangle.Vertex
vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }

triangle : RenderGraph.IndexedPipeline(BasicTriangle.Vertex, Mltrs.MvpMatrices)
triangle = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
	vertices: List.repeat(vertex, 3),
	indices: [0, 1, 2],
})

blueprint = RenderGraph.from_tuple_2((
	RenderGraph.draw_indexed(triangle),
	RenderGraph.draw_indexed(triangle),
))

zero : Mltrs.MvpMatrices
zero = { model: filled, view: filled, proj: filled }

filled : ShaderReflection.Float4x4
filled = {
	row_0: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_1: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_2: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_3: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
}
