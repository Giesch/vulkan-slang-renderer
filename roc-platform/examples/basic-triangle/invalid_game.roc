# Not an example: draw uses a record collection, but Game.new registers a single graph.
# `roc check` must reject this mismatch at Game.new.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import pf.ShaderReflection
import Generated/BasicTriangle

game : Game
game = Game.new({ init!, draw, graphs })

init! : {} => Game.Init
init! = |_| { window_title: "invalid game" }

draw = |_frame| selected.draw({ model: zero, view: zero, proj: zero })

selected = other_graphs.select(|g| g.first)

Ok(other_graphs) = { first: graphs, second: graphs }.Graphs

zero : ShaderReflection.Float4x4
zero = {
	row_0: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_1: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_2: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_3: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
}

Ok(graphs) = Graphs.single(RenderGraph.draw_indexed(triangle))

triangle = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
	vertices: List.repeat(vertex, 3),
	indices: [0, 1, 2],
})

vertex : BasicTriangle.Vertex
vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }
