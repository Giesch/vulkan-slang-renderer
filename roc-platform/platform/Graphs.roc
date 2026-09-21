import RenderGraph
import ValidatedRenderGraph

## A type converts to a graph collection when it provides
## `to_graphs : coll -> Try(Graphs(g), Graphs.Invalid)`.
coll.ToGraphs(g) :  where [coll.to_graphs : coll -> Try(Graphs(g), Graphs.Invalid)]

## Receiver extension without a RenderGraph-to-Graphs import cycle.
RenderGraphExtension := [].{
	to_graphs : RenderGraph(frame) -> Try(
		Graphs(Graphs.ValidatedGraph(frame)),
		Graphs.Invalid,
	)
	to_graphs = |render_graph| Graphs.single(render_graph)
}

## Deferred graph definitions. A collection holds validated graphs only:
## `single` and `map2` reject an invalid render graph before it can be registered.
Graphs(g) :: { definitions : List(ValidatedRenderGraph.HostGraph), build : U32 -> g }.{

	## A registered graph retains its typed packer, not host assets.
	ValidatedGraph(frame) :: { id : U32, pack : frame -> List(List(U8)) }.{
		draw : ValidatedGraph(frame), frame -> Draw
		draw = |ValidatedGraph.(graph), values|
			Draw.({ graph_id: graph.id, values: (graph.pack)(values) })
	}

	## A frame retains the collection type until Game.new erases it.
	Submission(g) :: Draw.{
		to_host : Submission(g) -> Draw
		to_host = |Submission.(draw)| draw
	}

	## A selected graph retains its collection type without retaining host assets.
	SelectedGraph(g, frame) :: ValidatedGraph(frame).{
		draw : SelectedGraph(g, frame), frame -> Submission(g)
		draw = |SelectedGraph.(graph), values| Submission.(graph.draw(values))
	}

	## Define selections at module scope to retain the selected packer.
	## This checks collection types, not the identity of captured graphs.
	select : Graphs(g), (g -> ValidatedGraph(frame)) -> SelectedGraph(g, frame)
	select = |graphs, choose| SelectedGraph.(choose(graphs.register()))

	## A single-graph collection needs no selector.
	draw : Graphs(ValidatedGraph(frame)), frame -> Submission(ValidatedGraph(frame))
	draw = |graphs, values| Submission.(graphs.register().draw(values))

	## The entire per-frame ABI: collection ordinal and ordered payloads.
	Draw :: { graph_id : U32, values : List(List(U8)) }.{
		is_eq : Draw, Draw -> Bool
		is_eq = |Draw.(left), Draw.(right)| left == right
	}

	## The aggregated validation message for an invalid render graph.
	Invalid : [InvalidRenderGraph(Str)]

	single : RenderGraph(frame) -> Try(Graphs(ValidatedGraph(frame)), Invalid)
	single = |render_graph| {
		graph = ValidatedRenderGraph.new(render_graph)?

		definitions = [graph.definition()]
		pack = graph.packer()
		build = |id| ValidatedGraph.({ id, pack })

		Ok(Graphs.({ definitions, build }))
	}

	# TODO: Remove this function once we can use an Ok(...) destructure instead:
	# https://github.com/roc-lang/roc/issues/11532
	## An invalid render graph becomes a compile-time error that carries the
	## validation message.
	or_crash : Try(Graphs(g), Invalid) -> Graphs(g)
	or_crash = |result| match result {
		Ok(graphs) => graphs
		Err(InvalidRenderGraph(message)) => crash message
	}

	to_graphs : Graphs(g) -> Try(Graphs(g), Invalid)
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

	register : Graphs(g) -> g
	register = |Graphs.(graphs)| (graphs.build)(0)

	definitions : Graphs(g) -> List(ValidatedRenderGraph.HostGraph)
	definitions = |Graphs.(graphs)| graphs.definitions
}
