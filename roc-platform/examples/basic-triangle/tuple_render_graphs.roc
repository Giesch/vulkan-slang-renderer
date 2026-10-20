# Tuple composition preserves declaration and payload order for every arity.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.Graphs
import pf.RenderGraph
import Generated/BasicTriangle

game : Game
game = Game.new({ init!: |_| { window_title: "tuple render graphs" }, graphs, draw: |_| graphs.draw((1, "two")) })

Ok(graphs) = Graphs.single(RenderGraph.from_tuple_2((byte_node, text_node)))

vertex : BasicTriangle.Vertex
vertex = { position: { x: 0.0, y: 0.0, z: 0.0 }, color: { x: 0.0, y: 0.0, z: 0.0 } }

node : Str, (a -> List(U8)) -> RenderGraph(a)
node = |name, to_bytes| RenderGraph.draw_indexed(
	RenderGraph.indexed_pipeline({
		name,
		shader: {
			name: BasicTriangle.shader.name,
			vertex_spv: BasicTriangle.shader.vertex_spv,
			fragment_spv: BasicTriangle.shader.fragment_spv,
			reflection_json: BasicTriangle.shader.reflection_json,
			reflection: BasicTriangle.shader.reflection,
			vertex: BasicTriangle.shader.vertex,
			uniform: { name, index: 0, size: 192, to_bytes },
		},
		vertices: List.repeat(vertex, 3),
		indices: [0, 1, 2],
	}),
)

bytes : U8 -> List(U8)
bytes = |v| List.repeat(v, 192)

byte_node = node("byte", bytes)

text_node = node("text", |text| bytes(text.to_utf8().len().to_u8_wrap()))

expect {
	render_graph = RenderGraph.from_tuple_2((byte_node, text_node))
	pack = render_graph.packer()
	pack((1, "two")) == [bytes(1), bytes(3)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text"]
}

expect {
	render_graph = RenderGraph.from_tuple_3((byte_node, text_node, node("byte_3", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3)) == [bytes(1), bytes(3), bytes(3)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3"]
}

expect {
	render_graph = RenderGraph.from_tuple_4((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4)) == [bytes(1), bytes(3), bytes(3), bytes(4)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4"]
}

expect {
	render_graph = RenderGraph.from_tuple_5((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5"]
}

expect {
	render_graph = RenderGraph.from_tuple_6((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6"]
}

expect {
	render_graph = RenderGraph.from_tuple_7((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes), node("byte_7", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6, 7)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6), bytes(7)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6", "byte_7"]
}

expect {
	render_graph = RenderGraph.from_tuple_8((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes), node("byte_7", bytes), node("byte_8", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6, 7, 8)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6), bytes(7), bytes(8)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6", "byte_7", "byte_8"]
}

expect {
	render_graph = RenderGraph.from_tuple_9((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes), node("byte_7", bytes), node("byte_8", bytes), node("byte_9", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6, 7, 8, 9)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6), bytes(7), bytes(8), bytes(9)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6", "byte_7", "byte_8", "byte_9"]
}

expect {
	render_graph = RenderGraph.from_tuple_10((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes), node("byte_7", bytes), node("byte_8", bytes), node("byte_9", bytes), node("byte_10", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6, 7, 8, 9, 10)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6), bytes(7), bytes(8), bytes(9), bytes(10)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6", "byte_7", "byte_8", "byte_9", "byte_10"]
}

expect {
	render_graph = RenderGraph.from_tuple_11((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes), node("byte_7", bytes), node("byte_8", bytes), node("byte_9", bytes), node("byte_10", bytes), node("byte_11", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6, 7, 8, 9, 10, 11)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6), bytes(7), bytes(8), bytes(9), bytes(10), bytes(11)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6", "byte_7", "byte_8", "byte_9", "byte_10", "byte_11"]
}

expect {
	render_graph = RenderGraph.from_tuple_12((byte_node, text_node, node("byte_3", bytes), node("byte_4", bytes), node("byte_5", bytes), node("byte_6", bytes), node("byte_7", bytes), node("byte_8", bytes), node("byte_9", bytes), node("byte_10", bytes), node("byte_11", bytes), node("byte_12", bytes)))
	pack = render_graph.packer()
	pack((1, "two", 3, 4, 5, 6, 7, 8, 9, 10, 11, 12)) == [bytes(1), bytes(3), bytes(3), bytes(4), bytes(5), bytes(6), bytes(7), bytes(8), bytes(9), bytes(10), bytes(11), bytes(12)] and
		render_graph.decls().map(|decl| decl.name) == ["byte", "text", "byte_3", "byte_4", "byte_5", "byte_6", "byte_7", "byte_8", "byte_9", "byte_10", "byte_11", "byte_12"]
}

## Empty children emit no payloads; nested tuples preserve depth-first order.
expect {
	render_graph = RenderGraph.from_tuple_3((RenderGraph.empty, RenderGraph.from_tuple_2((byte_node, text_node)), byte_node))
	pack = render_graph.packer()
	pack(({}, (7, "hello"), 9)) == [bytes(7), bytes(5), bytes(9)]
}

## Each graph retains its own packer even when frame types are identical.
Ok(left) = Graphs.single(byte_node)

Ok(right) = Graphs.single(node("offset", |v| bytes(v + 1)))

expect {
	left.draw(7).to_host() != right.draw(7).to_host()
}

expect {
	pack = RenderGraph.empty.packer()
	pack({}) == [] and RenderGraph.empty.decls().is_empty()
}
