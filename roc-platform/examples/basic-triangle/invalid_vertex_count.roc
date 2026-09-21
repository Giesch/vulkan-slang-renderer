# Not an example: a shader that reads a vertex struct declared as a
# vertex-count pipeline. `roc check` must fail with a type mismatch.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import pf.ShaderReflection
import Generated/BasicTriangle
import Generated/Mltrs

G : Graphs.ValidatedGraph(Mltrs.MvpMatrices)

game : Game
game = Game.new({ init!, draw, graphs })

graphs = Graphs.or_crash(Graphs.single(RenderGraph.draw_vertex_count(no_vertices, 3)))

init! : {} => Game.Init
init! = |_| { window_title: "invalid vertex count" }

draw : Game.Frame -> Graphs.Submission(G)
draw = |_frame| graphs.draw({ model: zero, view: zero, proj: zero })

no_vertices : RenderGraph.VertexCountPipeline(Mltrs.MvpMatrices)
no_vertices = RenderGraph.vertex_count_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
})

zero : ShaderReflection.Float4x4
zero = {
	row_0: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_1: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_2: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
	row_3: { x: 0.0, y: 0.0, z: 0.0, w: 0.0 },
}
