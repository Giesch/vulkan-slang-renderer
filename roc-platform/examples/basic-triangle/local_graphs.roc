# Not an example: draw uses its own record collection with inferred types.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph
import pf.Graphs
import pf.ShaderReflection
import Generated/BasicTriangle
import Generated/Mltrs

pair = Graphs.or_crash({ first: render_graph, second: render_graph }.Graphs)

graphs = Graphs.or_crash({ pair, single: RenderGraph.draw_indexed(triangle) }.Graphs)

game : Game
game = Game.new({ init!, draw, graphs })

init! : {} => Game.Init
init! = |_| { window_title: "two draws" }

first = graphs.select(|local| local.pair.first)

second = graphs.select(|local| local.pair.second)

single = graphs.select(|local| local.single)

draw = |frame| {
	if frame.aspect_ratio < 1.0 {
		single.draw(zero)
	} else {
		graph = if frame.aspect_ratio > 1.0 {
			second
		} else {
			first
		}
		graph.draw((zero, one))
	}
}

vertex : BasicTriangle.Vertex
vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }

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
expect Game.host_config(game).graphs.len() == 3

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

## Both branches produce the registered collection's type. Distinct graph
## ordinals must survive type erasure even when payloads are identical.
expect Game.draw(game, { aspect_ratio: 2.0 }) != Game.draw(game, { aspect_ratio: 1.0 })

expect {
	Game.draw(game, { aspect_ratio: 2.0 }) == second.draw((zero, one)).to_host()
}

## A different uniform shape still returns the same collection's submission.
expect {
	registered = graphs.register()
	Game.draw(game, { aspect_ratio: 0.5 }) == registered.single.draw(zero)
}
