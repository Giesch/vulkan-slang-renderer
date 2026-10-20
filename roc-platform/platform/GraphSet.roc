import RenderGraph
import ValidatedRenderGraph

coll.ToGraphSet(args) :
	where [
		coll.to_graph_set : coll -> Try(GraphSet(args), GraphSet.Invalid),
	]

# avoids a RenderGraph-to-GraphSet import cycle.
RenderGraphToGraphSetExtension := [].{
	to_graph_set : RenderGraph(frame) -> Try(
		GraphSet(GraphSet.ValidatedGraph(frame)),
		GraphSet.Invalid,
	)
	to_graph_set = |render_graph| GraphSet.single(render_graph)
}

## The application's collection of RenderGraphs.
## Defined and validated at compile time.
GraphSet(args) :: {
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
	select : GraphSet(args), (args -> ValidatedGraph(frame)) -> SelectedGraph(args, frame)
	select = |graphs, choose| SelectedGraph.(choose(graphs.register()))

	## A single-graph collection needs no selector.
	draw : GraphSet(ValidatedGraph(frame)), frame -> Submission(ValidatedGraph(frame))
	draw = |graphs, values| Submission.(graphs.register().draw(values))

	Draw :: { graph_id : U32, values : List(List(U8)) }.{
		is_eq : _
	}

	Invalid : [InvalidRenderGraph(Str)]

	single : RenderGraph(frame) -> Try(GraphSet(ValidatedGraph(frame)), Invalid)
	single = |render_graph| {
		graph = ValidatedRenderGraph.new(render_graph)?

		definitions = [graph.definition()]
		pack = graph.packer()
		build = |id| ValidatedGraph.({ id, pack })

		Ok(GraphSet.({ definitions, build }))
	}

	to_graph_set : GraphSet(args) -> Try(GraphSet(args), Invalid)
	to_graph_set = |graphs| Ok(graphs)

	map2 : left, right, (a, b -> c) -> Try(GraphSet(c), Invalid)
		where [left.ToGraphSet(a), right.ToGraphSet(b)]
	map2 = |left, right, combine| {
		GraphSet.(l) = left.to_graph_set()?
		GraphSet.(r) = right.to_graph_set()?

		count = l.definitions.len() + r.definitions.len()
		if count > 4294967295 {
			crash "render graph: graph count exceeds U32"
		}

		definitions = List.concat(l.definitions, r.definitions)

		build = |base| {
			right_offset = base + l.definitions.len().to_u32_wrap()
			combine((l.build)(base), (r.build)(right_offset))
		}

		Ok(GraphSet.({ definitions, build }))
	}

	register : GraphSet(args) -> args
	register = |GraphSet.(graphs)| (graphs.build)(0)

	definitions : GraphSet(args) -> List(ValidatedRenderGraph.HostGraph)
	definitions = |GraphSet.(graphs)| graphs.definitions
}
