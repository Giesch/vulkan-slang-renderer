# Not an example: a pipeline whose uniform handle does not belong to its
# shader. `roc check` must fail at the top-level Ok destructure.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import pf.ShaderReflection
import Generated/BasicTriangle
import Generated/Mltrs

G : Graphs.ValidatedGraph(Mltrs.MvpMatrices)

Ok(graphs) = Graphs.single(RenderGraph.draw_indexed(triangle))

game : Game
game = Game.new({ init!, draw, graphs })

init! : {} => Game.Init
init! = |_| { window_title: "invalid" }

draw : Game.Frame -> Graphs.Submission(G)
draw = |_frame| graphs.draw({ model: zero, view: zero, proj: zero })

## Hand-built, as if taken from a shader with two constant buffers. The
## validator rejects the position and the size against `BasicTriangle`.
foreign : RenderGraph.Uniform(Mltrs.MvpMatrices)
foreign = { name: "foreign", index: 1, size: 64, to_bytes: Mltrs.MvpMatrices.to_bytes }

vertex : BasicTriangle.Vertex
vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }

triangle : RenderGraph.IndexedPipeline(BasicTriangle.Vertex, Mltrs.MvpMatrices)
triangle = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: { ..BasicTriangle.shader, uniform: foreign },
	vertices: List.repeat(vertex, 3),
	indices: [0, 1, 2],
})

zero : ShaderReflection.Float4x4
zero = {
	row_0: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_1: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_2: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_3: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
}
