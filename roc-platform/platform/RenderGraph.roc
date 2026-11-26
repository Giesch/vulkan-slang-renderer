import ShaderReflection exposing [NoPush, PushSpec, UniformSlot]
import RenderGraphLower
import ResourcePlanner exposing [
	ComputePipelineKey,
	CpuUpdatedSlot,
	DrawIndexedIndirectCommand,
	GraphicsIndirectPipelineKey,
	GraphicsPipelineKey,
	ImmutableSlot,
	IndexBufferSlot,
]
import ValidatedRenderGraph exposing [HostFrameValue]

## Typed graph nodes and their per-frame packer.
##
## Mirrors the Rust `mltrs-render-graph` node API. A node's `frame` is the
## value it consumes each frame: a command's uniform `*Data`, an upload's
## element list, `[None, Some(f)]` for `optional`, `{ count, body }` for
## `repeat`, and tuples or lists for composed graphs. Bindings are fixed at
## build time.
##
## A pipeline key carries its shader's push type. Commands on a `NoPush`
## pipeline take no push block; commands on a `PushSpec` pipeline use the
## `*_with_push` constructors, which take the complete push input. Either
## mistake is a type error.
RenderGraph(frame) :: {
	nodes : List(RenderGraphLower.Node),
	pack : frame -> List(HostFrameValue),
}.{

	## A push block's complete input, fixed at build time.
	PushInput(data, bindings) : { data : data, bindings : bindings }

	## Workgroups per dispatch.
	GroupCount : { x : U32, y : U32, z : U32 }

	## An argument buffer of indexed-indirect records.
	IndirectArgs : ImmutableSlot(DrawIndexedIndirectCommand)

	## Lowers and validates `graph` against the declarations in `planner`.
	new : ResourcePlanner, RenderGraph(frame) -> Try(ValidatedRenderGraph(frame), [InvalidRenderGraph(Str)])
	new = |planner, graph| ValidatedRenderGraph.new(planner, graph.nodes, graph.pack)

	## Dispatches `group_count` workgroups.
	dispatch : ComputePipelineKey(NoPush), UniformSlot(data, bindings), GroupCount, bindings -> RenderGraph(data)
	dispatch = |pipeline, params, group_count, bindings|
		command(
			pipeline.pipeline_ref(),
			Dispatch(group_count),
			params,
			bindings,
			None,
		)

	dispatch_with_push : ComputePipelineKey(PushSpec(push_data, push_bindings)), UniformSlot(data, bindings), GroupCount, bindings, PushInput(push_data, push_bindings) -> RenderGraph(data)
	dispatch_with_push = |pipeline, params, group_count, bindings, push|
		command(
			pipeline.pipeline_ref(),
			Dispatch(group_count),
			params,
			bindings,
			Some(push_use(pipeline.push_spec(), push)),
		)

	## Draws `vertex_count` vertices with no index buffer; the shader reads
	## `SV_VertexID` to pull its vertex data.
	draw_vertex_count : GraphicsPipelineKey(NoPush), UniformSlot(data, bindings), U32, bindings -> RenderGraph(data)
	draw_vertex_count = |pipeline, params, vertex_count, bindings|
		command(
			pipeline.pipeline_ref(),
			VertexCount(vertex_count),
			params,
			bindings,
			None,
		)

	draw_vertex_count_with_push : GraphicsPipelineKey(PushSpec(push_data, push_bindings)), UniformSlot(data, bindings), U32, bindings, PushInput(push_data, push_bindings) -> RenderGraph(data)
	draw_vertex_count_with_push = |pipeline, params, vertex_count, bindings, push|
		command(
			pipeline.pipeline_ref(),
			VertexCount(vertex_count),
			params,
			bindings,
			Some(push_use(pipeline.push_spec(), push)),
		)

	## Draws every index in `indices`.
	draw_indexed : GraphicsPipelineKey(NoPush), IndexBufferSlot, UniformSlot(data, bindings), bindings -> RenderGraph(data)
	draw_indexed = |pipeline, indices, params, bindings|
		command(
			pipeline.pipeline_ref(),
			WholeIndexed(indices.buffer_ref()),
			params,
			bindings,
			None,
		)

	draw_indexed_with_push : GraphicsPipelineKey(PushSpec(push_data, push_bindings)), IndexBufferSlot, UniformSlot(data, bindings), bindings, PushInput(push_data, push_bindings) -> RenderGraph(data)
	draw_indexed_with_push = |pipeline, indices, params, bindings, push|
		command(
			pipeline.pipeline_ref(),
			WholeIndexed(indices.buffer_ref()),
			params,
			bindings,
			Some(push_use(pipeline.push_spec(), push)),
		)

	## Draws `index_count` indices of `indices` from `first_index`. A range
	## past the end of the index buffer fails validation.
	draw_index_range : GraphicsPipelineKey(NoPush), IndexBufferSlot, UniformSlot(data, bindings), U32, U32, bindings -> RenderGraph(data)
	draw_index_range = |pipeline, indices, params, first_index, index_count, bindings| {
		call = IndexRange({ indices: indices.buffer_ref(), first_index, index_count })

		command(pipeline.pipeline_ref(), call, params, bindings, None)
	}

	draw_index_range_with_push : GraphicsPipelineKey(PushSpec(push_data, push_bindings)), IndexBufferSlot, UniformSlot(data, bindings), U32, U32, bindings, PushInput(push_data, push_bindings) -> RenderGraph(data)
	draw_index_range_with_push = |pipeline, indices, params, first_index, index_count, bindings, push| {
		call = IndexRange({ indices: indices.buffer_ref(), first_index, index_count })

		command(
			pipeline.pipeline_ref(),
			call,
			params,
			bindings,
			Some(push_use(pipeline.push_spec(), push)),
		)
	}

	## Draws `draw_count` records of `args` from `first_command`, each with
	## `indices` bound. The records must lie inside `args`, which
	## `ResourcePlanner.indirect_buffer` declares.
	draw_indexed_indirect : GraphicsIndirectPipelineKey(NoPush), IndexBufferSlot, UniformSlot(data, bindings), IndirectArgs, U32, U32, bindings -> RenderGraph(data)
	draw_indexed_indirect = |pipeline, indices, params, args, first_command, draw_count, bindings| {
		call = indirect_call(indices, args, first_command, draw_count)

		command(pipeline.pipeline_ref(), call, params, bindings, None)
	}

	draw_indexed_indirect_with_push : GraphicsIndirectPipelineKey(PushSpec(push_data, push_bindings)), IndexBufferSlot, UniformSlot(data, bindings), IndirectArgs, U32, U32, bindings, PushInput(push_data, push_bindings) -> RenderGraph(data)
	draw_indexed_indirect_with_push = |pipeline, indices, params, args, first_command, draw_count, bindings, push| {
		call = indirect_call(indices, args, first_command, draw_count)

		command(
			pipeline.pipeline_ref(),
			call,
			params,
			bindings,
			Some(push_use(pipeline.push_spec(), push)),
		)
	}

	## Uploads the frame's elements into a CPU-updated buffer. More elements
	## than the buffer's capacity fail the frame on the host.
	upload : CpuUpdatedSlot(elem) -> RenderGraph(List(elem))
	upload = |slot| {
		elem_packer = slot.packer()
		node = Upload({ buffer: slot.buffer_ref(), element_size: elem_packer.stride })
		pack = |items| {
			count = items.len()
			if count > U32.highest.to_u64() {
				crash "render graph: an upload of ${count.to_str()} elements exceeds U32"
			}

			[Array({ count: count.to_u32_wrap(), bytes: items.join_map(elem_packer.to_bytes) })]
		}

		{ nodes: [node], pack }
	}

	## Skips `inner` on a frame whose value is `None`.
	optional : RenderGraph(frame) -> RenderGraph([None, Some(frame)])
	optional = |inner| {
		absent = List.repeat(Absent, RenderGraphLower.site_count(inner.nodes))
		pack = |maybe_frame|
			match maybe_frame {
				None => absent
				Some(inner_frame) => (inner.pack)(inner_frame)
			}

		{ nodes: [Optional(inner.nodes)], pack }
	}

	## Replays `body` `count` times per frame; zero and odd counts are legal.
	repeat : RenderGraph(frame) -> RenderGraph({ count : U32, body : frame })
	repeat = |body| {
		pack = |repeated| [Count(repeated.count)].concat((body.pack)(repeated.body))

		{ nodes: [Repeat(body.nodes)], pack }
	}

	## Composes graphs of one frame type in list order. The frame list must
	## have one frame per graph.
	list : List(RenderGraph(frame)) -> RenderGraph(List(frame))
	list = |graphs| {
		packers = graphs.map(|graph| graph.pack)
		pack = |frames| {
			if frames.len() != packers.len() {
				crash "render graph: a list of ${packers.len().to_str()} graphs received ${frames.len().to_str()} frames"
			}

			packers.map_with_index(|graph_pack, index| (graph_pack, index)).join_map(
				|(graph_pack, index)|
					match frames.get(index) {
						Ok(frame) => graph_pack(frame)
						Err(_) => []
					},
			)
		}

		{ nodes: graphs.join_map(|graph| graph.nodes), pack }
	}

	## Compose 2 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_2 : (
		RenderGraph(a),
		RenderGraph(b),
	) -> RenderGraph((a, b))
	from_tuple_2 = |(graph0, graph1)| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
		].join(),
		pack: |(frame0, frame1)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
			].join(),
	}

	## Compose 3 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_3 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
	) -> RenderGraph((a, b, c))
	from_tuple_3 = |(graph0, graph1, graph2)| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
		].join(),
		pack: |(frame0, frame1, frame2)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
			].join(),
	}

	## Compose 4 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_4 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
	) -> RenderGraph((a, b, c, d))
	from_tuple_4 = |(graph0, graph1, graph2, graph3)| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
		].join(),
		pack: |(frame0, frame1, frame2, frame3)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
			].join(),
	}

	## Compose 5 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_5 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
	) -> RenderGraph((a, b, c, d, e))
	from_tuple_5 = |(graph0, graph1, graph2, graph3, graph4)| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
		].join(),
		pack: |(frame0, frame1, frame2, frame3, frame4)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
			].join(),
	}

	## Compose 6 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_6 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
	) -> RenderGraph((a, b, c, d, e, f))
	from_tuple_6 = |(graph0, graph1, graph2, graph3, graph4, graph5)| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
		].join(),
		pack: |(frame0, frame1, frame2, frame3, frame4, frame5)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
			].join(),
	}

	## Compose 7 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_7 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
		RenderGraph(g),
	) -> RenderGraph((a, b, c, d, e, f, g))
	from_tuple_7 = |(graph0, graph1, graph2, graph3, graph4, graph5, graph6)| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
			graph6.nodes,
		].join(),
		pack: |(frame0, frame1, frame2, frame3, frame4, frame5, frame6)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
				(graph6.pack)(frame6),
			].join(),
	}

	## Compose 8 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_8 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
		RenderGraph(g),
		RenderGraph(h),
	) -> RenderGraph((a, b, c, d, e, f, g, h))
	from_tuple_8 = |
		(
			graph0,
			graph1,
			graph2,
			graph3,
			graph4,
			graph5,
			graph6,
			graph7,
		),
	| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
			graph6.nodes,
			graph7.nodes,
		].join(),
		pack: |(frame0, frame1, frame2, frame3, frame4, frame5, frame6, frame7)|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
				(graph6.pack)(frame6),
				(graph7.pack)(frame7),
			].join(),
	}

	## Compose 9 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_9 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
		RenderGraph(g),
		RenderGraph(h),
		RenderGraph(i),
	) -> RenderGraph((a, b, c, d, e, f, g, h, i))
	from_tuple_9 = |
		(
			graph0,
			graph1,
			graph2,
			graph3,
			graph4,
			graph5,
			graph6,
			graph7,
			graph8,
		),
	| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
			graph6.nodes,
			graph7.nodes,
			graph8.nodes,
		].join(),
		pack: |
			(
				frame0,
				frame1,
				frame2,
				frame3,
				frame4,
				frame5,
				frame6,
				frame7,
				frame8,
			),
		|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
				(graph6.pack)(frame6),
				(graph7.pack)(frame7),
				(graph8.pack)(frame8),
			].join(),
	}

	## Compose 10 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_10 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
		RenderGraph(g),
		RenderGraph(h),
		RenderGraph(i),
		RenderGraph(j),
	) -> RenderGraph((a, b, c, d, e, f, g, h, i, j))
	from_tuple_10 = |
		(
			graph0,
			graph1,
			graph2,
			graph3,
			graph4,
			graph5,
			graph6,
			graph7,
			graph8,
			graph9,
		),
	| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
			graph6.nodes,
			graph7.nodes,
			graph8.nodes,
			graph9.nodes,
		].join(),
		pack: |
			(
				frame0,
				frame1,
				frame2,
				frame3,
				frame4,
				frame5,
				frame6,
				frame7,
				frame8,
				frame9,
			),
		|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
				(graph6.pack)(frame6),
				(graph7.pack)(frame7),
				(graph8.pack)(frame8),
				(graph9.pack)(frame9),
			].join(),
	}

	## Compose 11 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_11 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
		RenderGraph(g),
		RenderGraph(h),
		RenderGraph(i),
		RenderGraph(j),
		RenderGraph(k),
	) -> RenderGraph((a, b, c, d, e, f, g, h, i, j, k))
	from_tuple_11 = |
		(
			graph0,
			graph1,
			graph2,
			graph3,
			graph4,
			graph5,
			graph6,
			graph7,
			graph8,
			graph9,
			graph10,
		),
	| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
			graph6.nodes,
			graph7.nodes,
			graph8.nodes,
			graph9.nodes,
			graph10.nodes,
		].join(),
		pack: |
			(
				frame0,
				frame1,
				frame2,
				frame3,
				frame4,
				frame5,
				frame6,
				frame7,
				frame8,
				frame9,
				frame10,
			),
		|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
				(graph6.pack)(frame6),
				(graph7.pack)(frame7),
				(graph8.pack)(frame8),
				(graph9.pack)(frame9),
				(graph10.pack)(frame10),
			].join(),
	}

	## Compose 12 graphs in tuple order; the frame is a tuple of theirs.
	from_tuple_12 : (
		RenderGraph(a),
		RenderGraph(b),
		RenderGraph(c),
		RenderGraph(d),
		RenderGraph(e),
		RenderGraph(f),
		RenderGraph(g),
		RenderGraph(h),
		RenderGraph(i),
		RenderGraph(j),
		RenderGraph(k),
		RenderGraph(l),
	) -> RenderGraph((a, b, c, d, e, f, g, h, i, j, k, l))
	from_tuple_12 = |
		(
			graph0,
			graph1,
			graph2,
			graph3,
			graph4,
			graph5,
			graph6,
			graph7,
			graph8,
			graph9,
			graph10,
			graph11,
		),
	| {
		nodes: [
			graph0.nodes,
			graph1.nodes,
			graph2.nodes,
			graph3.nodes,
			graph4.nodes,
			graph5.nodes,
			graph6.nodes,
			graph7.nodes,
			graph8.nodes,
			graph9.nodes,
			graph10.nodes,
			graph11.nodes,
		].join(),
		pack: |
			(
				frame0,
				frame1,
				frame2,
				frame3,
				frame4,
				frame5,
				frame6,
				frame7,
				frame8,
				frame9,
				frame10,
				frame11,
			),
		|
			[
				(graph0.pack)(frame0),
				(graph1.pack)(frame1),
				(graph2.pack)(frame2),
				(graph3.pack)(frame3),
				(graph4.pack)(frame4),
				(graph5.pack)(frame5),
				(graph6.pack)(frame6),
				(graph7.pack)(frame7),
				(graph8.pack)(frame8),
				(graph9.pack)(frame9),
				(graph10.pack)(frame10),
				(graph11.pack)(frame11),
			].join(),
	}
}

## One command node and its packer: the uniform's data bytes, then the
## push block's build-time data bytes, each only when its block has data.
command : ResourcePlanner.PipelineRef, RenderGraphLower.CommandCall, UniformSlot(data, bindings), bindings, [None, Some(RenderGraphLower.PushUse)] -> RenderGraph(data)
command = |pipeline, call, params, bindings, push| {
	schema = params.schema()
	layout = ResourcePlanner.block_layout(schema)
	uniform = {
		slot: params.index(),
		provenance: params.provenance(),
		layout,
		bindings: (schema.extract_bindings)(bindings),
	}
	push_values =
		match push {
			Some(push_block) if push_block.layout.data_size > 0 => [Bytes(push_block.data)]
			_ => []
		}

	pack = |data| {
		if layout.data_size == 0 {
			return push_values
		}

		bytes = (schema.pack_data)(data)
		if bytes.len() != layout.data_size.to_u64() {
			crash "render graph: ${layout.name} packed ${bytes.len().to_str()} data bytes for a ${layout.data_size.to_str()}-byte data block"
		}

		[Bytes(bytes)].concat(push_values)
	}

	{ nodes: [Command({ call, pipeline, uniform, push })], pack }
}

push_use : PushSpec(push_data, push_bindings), RenderGraph.PushInput(push_data, push_bindings) -> RenderGraphLower.PushUse
push_use = |spec, input| {
	schema = spec.schema

	{
		layout: ResourcePlanner.block_layout(schema),
		bindings: (schema.extract_bindings)(input.bindings),
		data: (schema.pack_data)(input.data),
	}
}

indirect_call : IndexBufferSlot, ImmutableSlot(DrawIndexedIndirectCommand), U32, U32 -> RenderGraphLower.CommandCall
indirect_call = |indices, args, first_command, draw_count|
	IndexedIndirect({ indices: indices.buffer_ref(), args: args.buffer_ref(), first_command, draw_count })
