import ValidatedRenderGraph exposing [HostFrameValue, HostGraph]

## The application's collection of validated render graphs, built once at
## setup. `args` is what the app selects a graph from each frame: a single
## graph's `ValidatedGraph(frame)`, or whatever `map2` combines.
##
## `fingerprints` holds each definition's digest, by graph id, so a selection
## can prove its graph belongs to this set.
GraphSet(args) :: {
	definitions : List(HostGraph),
	fingerprints : List(List(U8)),
	build : U32 -> args,
}.{

	## A registered graph: its id in the set, the digest of its definition,
	## and its packer, without the host assets it was validated with.
	ValidatedGraph(frame) :: {
		id : U32,
		fingerprint : List(U8),
		pack : frame -> List(HostFrameValue),
	}.{
		to_draw : ValidatedGraph(frame), frame -> Draw
		to_draw = |graph, frame| { graph_id: graph.id, values: (graph.pack)(frame) }
	}

	## One frame's submission: the selected graph and its values.
	Draw := { graph_id : U32, values : List(HostFrameValue) }.{
		is_eq : _
	}

	## What `draw` returns, tagged with the set it came from: statically by
	## `args`, and at runtime by the submitted graph's digest. The platform
	## converts it into GPU commands.
	Submission(args) :: { draw : Draw, fingerprint : List(U8) }.{
		to_host : Submission(args) -> Draw
		to_host = |Submission.(submitted)| submitted.draw
	}

	## The submission's draw, if this set registered its graph. Two sets with
	## the same `args` type share a `Submission` type, so a submission from
	## the other set would execute this set's graph at that id with the other
	## graph's values; accepting one crashes.
	accept_submission : GraphSet(args), Submission(args) -> Draw
	accept_submission = |graphs, Submission.(submitted)| {
		graph_id = submitted.draw.graph_id
		if graphs.fingerprints.get(graph_id.to_u64()) != Ok(submitted.fingerprint) {
			crash "render graph: draw submitted a graph (id ${graph_id.to_str()}) from a set other than the one Game.new registered; draw from the registered set"
		}

		submitted.draw
	}

	## A selected graph keeps its set's type without its host assets.
	SelectedGraph(args, frame) : SelectedGraphOf(args, frame)

	## Selects one graph of the set. Define selections at module scope so
	## they keep the selected packer. `choose` must return a graph of this
	## set; another set's graph would submit an id this set's host graphs do
	## not match, so selecting one crashes.
	select : GraphSet(args), (args -> ValidatedGraph(frame)) -> SelectedGraph(args, frame)
	select = |graphs, choose| {
		graph = choose(graphs.register())
		if !graphs.contains(graph) {
			crash "render graph: GraphSet chose a graph (id ${graph.id.to_str()}) this set did not register; choose from the set's own registered graphs"
		}

		SelectedGraphOf.(graph)
	}

	## True if `graph` is this set's graph at its id: same id, same
	## definition.
	contains : GraphSet(args), ValidatedGraph(frame) -> Bool
	contains = |graphs, graph| graphs.fingerprints.get(graph.id.to_u64()) == Ok(graph.fingerprint)

	## A single-graph set needs no selector. Its registered graph goes through
	## `select`, so a set whose build returns another set's graph crashes too.
	draw : GraphSet(ValidatedGraph(frame)), frame -> Submission(ValidatedGraph(frame))
	draw = |graphs, frame| graphs.select(|graph| graph).draw(frame)

	## A set of one graph.
	single : ValidatedRenderGraph(frame) -> GraphSet(ValidatedGraph(frame))
	single = |graph| {
		pack = graph.packer()
		fingerprint = graph.fingerprint()

		{
			definitions: [graph.definition()],
			fingerprints: [fingerprint],
			build: |id| { id, fingerprint, pack },
		}
	}

	## The union of two sets, numbering `right`'s graphs after `left`'s.
	map2 : GraphSet(a), GraphSet(b), (a, b -> c) -> GraphSet(c)
	map2 = |left, right, combine| {
		left_count = left.definitions.len()
		if left_count + right.definitions.len() > U32.highest.to_u64() {
			crash "render graph: a graph set holds at most ${U32.highest.to_str()} graphs"
		}

		build = |base| {
			right_base = base + left_count.to_u32_wrap()

			combine((left.build)(base), (right.build)(right_base))
		}

		{
			definitions: left.definitions.concat(right.definitions),
			fingerprints: left.fingerprints.concat(right.fingerprints),
			build,
		}
	}

	## The registered graphs, numbered from 0.
	register : GraphSet(args) -> args
	register = |graphs| (graphs.build)(0)

	## The host definition of every graph, by graph id.
	definitions : GraphSet(args) -> List(HostGraph)
	definitions = |graphs| graphs.definitions
}

## Defined outside `GraphSet` so its `draw` does not shadow `GraphSet.draw`.
SelectedGraphOf(args, frame) :: GraphSet.ValidatedGraph(frame).{
	draw : SelectedGraphOf(args, frame), frame -> GraphSet.Submission(args)
	draw = |SelectedGraphOf.(graph), frame|
		GraphSet.Submission.({ draw: graph.to_draw(frame), fingerprint: graph.fingerprint })
}
