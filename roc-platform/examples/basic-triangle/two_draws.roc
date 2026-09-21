# Not an example: one pipeline drawn by two nodes, built from a tuple
# of render graphs. `roc test` runs its expectations; `roc check` must accept it.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import pf.ShaderReflection

import Generated/BasicTriangle
import Generated/Mltrs

graphs = Graphs.or_crash(Graphs.single(render_graph))

game : Game
game = Game.new({ init!, draw, graphs })

init! : {} => Game.Init
init! = |_| { window_title: "two draws" }

draw = |_frame| two_draws(zero, one)

two_draws : Mltrs.MvpMatrices, Mltrs.MvpMatrices -> Graphs.Submission(_)
two_draws = |a, b| graphs.draw((a, b))

vertex : BasicTriangle.Vertex
vertex = {
	position: { x: 0.0, y: 0.0, z: 0.0 },
	color: { x: 0.0, y: 0.0, z: 0.0 },
}

triangle : RenderGraph.IndexedPipeline(BasicTriangle.Vertex, Mltrs.MvpMatrices)
triangle = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
	vertices: List.repeat(vertex, 3),
	indices: [0, 1, 2],
})

## Each node owns a pipeline and a uniform buffer, so one declaration can be
## drawn twice with different values.
render_graph = RenderGraph.from_tuple_2((
	RenderGraph.draw_indexed(triangle),
	RenderGraph.draw_indexed(triangle),
))

zero : Mltrs.MvpMatrices
zero = { model: filled(0.0), view: filled(0.0), proj: filled(0.0) }

one : Mltrs.MvpMatrices
one = { model: filled(1.0), view: filled(1.0), proj: filled(1.0) }

filled : F32 -> ShaderReflection.Float4x4
filled = |v| {
	row_0: { x: v, y: v, z: v, w: v },
	row_1: { x: v, y: v, z: v, w: v },
	row_2: { x: v, y: v, z: v, w: v },
	row_3: { x: v, y: v, z: v, w: v },
}

## Node order and frame packing are checked through the host ABI.
expect Game.host_config(game).graphs.len() == 1

## An invalid render graph is rejected with the validator's message.
expect {
	foreign : RenderGraph.Uniform(Mltrs.MvpMatrices)
	foreign = { name: "foreign", index: 1, size: 64, to_bytes: Mltrs.MvpMatrices.to_bytes }
	invalid = RenderGraph.indexed_pipeline({
		name: "basic_triangle",
		shader: { ..BasicTriangle.shader, uniform: foreign },
		vertices: List.repeat(vertex, 3),
		indices: [0, 1, 2],
	})
	match Graphs.single(RenderGraph.draw_indexed(invalid)) {
		Err(InvalidRenderGraph(message)) => message.contains("binds uniform foreign")
		Ok(_) => False
	}
}

## Each tuple position supplies a distinct node even for identical uniform types.
expect {
	pack = render_graph.packer()
	pack((zero, one)) == [Mltrs.MvpMatrices.to_bytes(zero), Mltrs.MvpMatrices.to_bytes(one)]
}

expect two_draws(zero, one).to_host() != two_draws(one, zero).to_host()
