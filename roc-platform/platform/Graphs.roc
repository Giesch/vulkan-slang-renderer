import RenderGraph
import ValidatedRenderGraph

coll.ToGraphs(args) :
	where [
		coll.to_graphs : coll -> Try(Graphs(args), Graphs.Invalid),
	]

# avoids a RenderGraph-to-Graphs import cycle.
RenderGraphToGraphsExtension := [].{
	to_graphs : RenderGraph(frame) -> Try(
		Graphs(Graphs.ValidatedGraph(frame)),
		Graphs.Invalid,
	)
	to_graphs = |render_graph| Graphs.single(render_graph)
}

## The application's collection of RenderGraphs.
## Defined and validated at compile time.
Graphs(args) :: {
	definitions : List(ValidatedRenderGraph.HostGraph),
	build : U32 -> args,
}.{

	ValidatedGraph(frame) :: { id : U32, pack : frame -> List(List(U8)) }.{
		draw : ValidatedGraph(frame), frame -> Draw
		draw = |ValidatedGraph.(graph), values|
			Draw.({ graph_id: graph.id, values: (graph.pack)(values) })
	}

	## The type returned by `Graph.draw`.
	## Converted into GPU commands by the platform.
	Submission(args) :: Draw.{
		to_host : Submission(args) -> Draw
		to_host = |Submission.(draw)| draw
	}

	## A selected graph retains its collection type without retaining host assets.
	SelectedGraph(args, frame) :: ValidatedGraph(frame).{
		draw : SelectedGraph(args, frame), frame -> Submission(args)
		draw = |SelectedGraph.(graph), values| Submission.(graph.draw(values))
	}

	## Define selections at module scope to retain the selected packer.
	## This checks collection types, not the identity of captured graphs.
	select : Graphs(args), (args -> ValidatedGraph(frame)) -> SelectedGraph(args, frame)
	select = |graphs, choose| SelectedGraph.(choose(graphs.register()))

	## A single-graph collection needs no selector.
	draw : Graphs(ValidatedGraph(frame)), frame -> Submission(ValidatedGraph(frame))
	draw = |graphs, values| Submission.(graphs.register().draw(values))

	Draw :: { graph_id : U32, values : List(List(U8)) }.{
		is_eq : _
	}

	Invalid : [InvalidRenderGraph(Str)]

	single : RenderGraph(frame) -> Try(Graphs(ValidatedGraph(frame)), Invalid)
	single = |render_graph| {
		graph = ValidatedRenderGraph.new(render_graph)?

		definitions = [graph.definition()]
		pack = graph.packer()
		build = |id| ValidatedGraph.({ id, pack })

		Ok(Graphs.({ definitions, build }))
	}

	to_graphs : Graphs(args) -> Try(Graphs(args), Invalid)
	to_graphs = |graphs| Ok(graphs)

	map2 : left, right, (a, b -> c) -> Try(Graphs(c), Invalid)
		where [left.ToGraphs(a), right.ToGraphs(b)]
	map2 = |left, right, combine| {
		Graphs.(l) = left.to_graphs()?
		Graphs.(r) = right.to_graphs()?

		count = l.definitions.len() + r.definitions.len()
		if count > 4294967295 {
			crash "render graph: graph count exceeds U32"
		}

		definitions = List.concat(l.definitions, r.definitions)

		build = |base| {
			right_offset = base + l.definitions.len().to_u32_wrap()
			combine((l.build)(base), (r.build)(right_offset))
		}

		Ok(Graphs.({ definitions, build }))
	}

	register : Graphs(args) -> args
	register = |Graphs.(graphs)| (graphs.build)(0)

	definitions : Graphs(args) -> List(ValidatedRenderGraph.HostGraph)
	definitions = |Graphs.(graphs)| graphs.definitions
}
