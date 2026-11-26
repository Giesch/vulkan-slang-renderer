import ShaderReflection exposing [GraphBinding]
import RenderGraphDesc exposing [GraphDesc, LeafPass, SchemaDesc]
import RenderGraphValidate exposing [GraphError]
import ResourcePlanner

## Lowering of typed graph nodes into the plain-data description.
##
## Mirrors `crates/render-graph/src/runtime/lower.rs`: uniform sources intern
## by planner slot, pipelines by planner id, consecutive top-level draws merge
## into the one main raster pass, and `optional` and `repeat` open value
## scopes. Unlike the typed Rust path, every block schema carries its field
## layout, and every planner buffer, texture, and import lowers one-to-one so
## a slot's id is its description id.
##
## Each node also reports the per-frame value sites its packer emits, in the
## same order lowering declares values. A later node sharing a uniform slot
## declares no value of its own; its site repeats the first node's.
RenderGraphLower := {}.{

	## One typed node, with its types erased.
	Node := [
		Command(CommandNode),
		Upload(UploadNode),
		Optional(List(Node)),
		Repeat(List(Node)),
	]

	CommandNode : {
		call : CommandCall,
		pipeline : ResourcePlanner.PipelineRef,
		uniform : UniformUse,
		push : [None, Some(PushUse)],
	}

	CommandCall : [
		Dispatch({ x : U32, y : U32, z : U32 }),
		VertexCount(U32),
		WholeIndexed(ResourcePlanner.BufferRef),
		IndexRange({ indices : ResourcePlanner.BufferRef, first_index : U32, index_count : U32 }),
		IndexedIndirect({ indices : ResourcePlanner.BufferRef, args : ResourcePlanner.BufferRef, first_command : U32, draw_count : U32 }),
	]

	## A command's uniform slot, its block layout, and its build-time bindings.
	UniformUse : {
		slot : U32,
		provenance : ShaderReflection.Provenance,
		layout : ResourcePlanner.BlockLayout,
		bindings : List((U16, GraphBinding)),
	}

	## A command's push block, with its data packed at build time.
	PushUse : {
		layout : ResourcePlanner.BlockLayout,
		bindings : List((U16, GraphBinding)),
		data : List(U8),
	}

	UploadNode : { buffer : ResourcePlanner.BufferRef, element_size : U32 }

	## How each per-frame value site maps to the description's values: its
	## own value, or the earlier site of a shared uniform's first node.
	SitePlan : [Value(U32), SameAs(U64)]

	## The number of per-frame value sites `nodes` emit.
	site_count : List(Node) -> U64
	site_count = |nodes| nodes.fold(0, |count, node| count + node_site_count(node))

	node_site_count : Node -> U64
	node_site_count = |node|
		match node {
			Command(command) => {
				uniform_sites = if (command.uniform.layout.data_size > 0) 1 else 0
				push_sites =
					match command.push {
						Some(push) if push.layout.data_size > 0 => 1
						_ => 0
					}

				uniform_sites + push_sites
			}

			Upload(_) => 1
			Optional(body) => site_count(body)
			Repeat(body) => 1 + site_count(body)
		}

	Output : {
		desc : GraphDesc,
		schemas : RenderGraphDesc.SchemaTable,
		errors : List(GraphError),

		## planner uniform slot per description uniform
		uniform_slots : List(U32),

		## planner pipeline id per description pipeline
		pipeline_indices : List(U32),
		sites : List(SitePlan),
	}

	lower : ResourcePlanner, List(Node) -> Output
	lower = |planner, nodes| {
		initial = {
			planner,
			desc: {
				..RenderGraphDesc.empty,
				textures: planner.textures().map(|texture| texture.decl),
				buffers: planner.buffers().map(buffer_decl),
				import_decls: planner.import_decls().map(|import_decl| { name: import_decl.name }),
			},
			schemas: [],
			errors: [],
			scope: Top,
			scope_id: 0,
			next_scope_id: 1,
			body: [],
			draws: 0,
			dispatches: 0,
			uniforms: [],
			pipeline_indices: [],
			sites: [],
		}
		state = nodes.fold(initial, lower_node)

		{
			desc: state.desc,
			schemas: { schemas: state.schemas },
			errors: state.errors,
			uniform_slots: state.uniforms.map(|uniform| uniform.slot),
			pipeline_indices: state.pipeline_indices,
			sites: state.sites,
		}
	}
}

## The lowering state. `scope_id` names the open scope (0 is the top level)
## so a uniform shared across scopes is caught; `body` collects the open
## scope's leaves.
State : {
	planner : ResourcePlanner,
	desc : GraphDesc,
	schemas : List(SchemaDesc),
	errors : List(GraphError),
	scope : Scope,
	scope_id : U64,
	next_scope_id : U64,
	body : List(LeafPass),
	draws : U64,
	dispatches : U64,
	uniforms : List(InternedUniform),
	pipeline_indices : List(U32),
	sites : List(RenderGraphLower.SitePlan),
}

Scope : [Top, Repeat({ count : U32 }), Optional({ first : [None, Some(U32)] })]

## A description uniform: its planner slot, the scope that first wrote it,
## and the site carrying its data.
InternedUniform : { slot : U32, scope_id : U64, data_site : [None, Some(U64)] }

buffer_decl : ResourcePlanner.PlannedBuffer -> RenderGraphDesc.BufferDecl
buffer_decl = |buffer| {
	kind =
		match buffer.role {
			CpuUpdated => Storage
			GpuUpdated => GpuUpdatedFlight
			Immutable => Immutable
			IndirectArgs => Immutable
			Singleton => Singleton
			Index => Index
		}

	{ name: buffer.name, kind, capacity: Some(buffer.len), elem_size: Some(buffer.element_size) }
}

lower_node : State, RenderGraphLower.Node -> State
lower_node = |state, node|
	match node {
		Command(command) => lower_command(state, command)
		Upload(upload) => lower_upload(state, upload)
		Optional(body) => lower_scope(state, Optional, body)
		Repeat(body) => lower_scope(state, Repeat, body)
	}

add_error : State, GraphError -> State
add_error = |state, error| { ..state, errors: state.errors.append(error) }

## Declares a value in the open scope; the first value of an optional scope
## is its gate.
add_value : State, Str, RenderGraphDesc.ValueKind -> (State, U32)
add_value = |state, name, kind| {
	id = state.desc.values.len().to_u32_wrap()
	(scope, value_scope) =
		match state.scope {
			Top => (Top, Top)
			Repeat({ count }) => (state.scope, Repeat({ count: count }))
			Optional({ first: Some(gate) }) => (state.scope, When({ gate: gate }))
			Optional({ first: None }) => (Optional({ first: Some(id) }), When({ gate: id }))
		}

	optional =
		match state.scope {
			Optional(_) => Bool.True
			_ => Bool.False
		}

	decl = { name, kind, optional, scope: value_scope }
	grown = {
		..state,
		scope,
		desc: { ..state.desc, values: state.desc.values.append(decl) },
		sites: state.sites.append(Value(id)),
	}

	(grown, id)
}

add_schema : State, SchemaDesc -> (State, U32)
add_schema = |state, schema| {
	id = state.schemas.len().to_u32_wrap()

	({ ..state, schemas: state.schemas.append(schema) }, id)
}

## A schema of plain bytes with no fields, for data values and upload
## elements.
bytes_schema : Str, U32 -> SchemaDesc
bytes_schema = |name, size| { name, size, resource_fields: [], layout: None }

block_schema : ResourcePlanner.BlockLayout -> SchemaDesc
block_schema = |layout| {
	resource_fields = layout.resource_fields.map(
		|kind|
			match kind {
				SampledTex => SampledTex
				StorageTex => StorageTex
				BufAddr => BufAddr
			},
	)
	fields = layout.fields.map(
		|field| {
			kind =
				match field.kind {
					Data({ src_offset }) => Data({ src_offset: src_offset })
					SampledTex => SampledTex
					StorageTex => StorageTex
					BufAddr => BufAddr
				}

			{ key: field.key, offset: field.offset, len: field.len, kind }
		},
	)

	{ name: layout.name, size: layout.gpu_size, resource_fields, layout: Some({ fields: fields }) }
}

## Resolves bindings to description resources, reporting each one whose
## provenance differs from its planner entry.
lower_bindings : State, List((U16, GraphBinding)) -> (State, List(RenderGraphDesc.ResourceBinding))
lower_bindings = |state, bindings|
	bindings.fold(
		(state, []),
		|(current, lowered), (key, binding)| {
			(checked, resource) = lower_binding(current, binding)

			(checked, lowered.append({ key, resource }))
		},
	)

lower_binding : State, GraphBinding -> (State, RenderGraphDesc.ResourceRef)
lower_binding = |state, binding|
	match binding {
		SampledTex(Read(tex)) => (check_texture(state, tex), Tex(tex.id, Read))
		SampledTex(ReadPrevious(tex)) => (check_texture(state, tex), Tex(tex.id, ReadPrevious))
		SampledTex(Imported(import_ref)) => {
			planned = state.planner.import_decls().get(import_ref.id.to_u64())
			matches = planned.map_ok(|found| found.provenance) == Ok(import_ref.provenance)

			(check_provenance(state, matches, Import, import_ref), External(import_ref.id))
		}

		StorageTex(Write(tex)) => (check_texture(state, tex), Tex(tex.id, Write))
		StorageTex(Mutate(tex)) => (check_texture(state, tex), Tex(tex.id, Mutate))
		Buffer(target, access) => {
			planned = state.planner.buffers().get(target.buffer.id.to_u64())
			matches = planned.map_ok(|found| found.provenance) == Ok(target.buffer.provenance)
			slot =
				match target.slot {
					Current => Current
					Previous => Previous
				}

			buffer = { buffer: target.buffer.id, slot, offset: target.offset, range: None }
			buffer_access =
				match access {
					Read => Read
					Mutate => Mutate
				}

			(check_provenance(state, matches, Buffer, target.buffer), Buf(buffer, buffer_access))
		}
	}

check_texture : State, ShaderReflection.DeclaredResource -> State
check_texture = |state, tex| {
	planned = state.planner.textures().get(tex.id.to_u64())
	matches = planned.map_ok(|found| found.provenance) == Ok(tex.provenance)

	check_provenance(state, matches, Texture, tex)
}

check_provenance : State, Bool, RenderGraphValidate.TableKind, ShaderReflection.DeclaredResource -> State
check_provenance = |state, matches, table, resource|
	if (matches)
		state
	else
		add_error(state, ForeignDeclaration({ table, id: resource.id, name: resource.provenance.name }))

## The planner's declaration at `id` is the one a slot was minted for.
check_buffer_ref : State, ResourcePlanner.BufferRef -> State
check_buffer_ref = |state, buffer|
	if (state.planner.buffers().get(buffer.id.to_u64()).map_ok(|found| found.provenance) == Ok(buffer.decl.provenance))
		state
	else
		add_error(state, ForeignDeclaration({ table: Buffer, id: buffer.id, name: buffer.decl.name }))

check_pipeline_ref : State, ResourcePlanner.PipelineRef -> State
check_pipeline_ref = |state, pipeline|
	if (state.planner.pipelines().get(pipeline.id.to_u64()) == Ok(pipeline.decl))
		state
	else
		add_error(state, ForeignDeclaration({ table: Pipeline, id: pipeline.id, name: pipeline.decl.name }))

## Interns a command's uniform by planner slot. The first node writing a
## slot declares its data value; a later one must agree on data size and
## bindings, and stay in the same scope.
intern_uniform : State, RenderGraphLower.UniformUse -> (State, U32)
intern_uniform = |state, uniform| {
	(with_bindings, bindings) = lower_bindings(state, uniform.bindings)
	data_size = uniform.layout.data_size
	planned = with_bindings.planner.uniforms().get(uniform.slot.to_u64())
	checked =
		if (planned.map_ok(|found| found.provenance) == Ok(uniform.provenance))
			with_bindings
		else
			add_error(with_bindings, ForeignDeclaration({ table: Uniform, id: uniform.slot, name: uniform.provenance.name }))

	name =
		match planned {
			Ok(found) => found.name
			Err(_) => "uniform${uniform.slot.to_str()}"
		}

	match state.uniforms.find_first_index(|interned| interned.slot == uniform.slot) {
		Ok(index) => {
			id = index.to_u32_wrap()
			existing = state.uniforms.get(index) ?? { slot: uniform.slot, scope_id: 0, data_site: None }
			same_source =
				match state.desc.uniforms.get(index) {
					Ok(decl) => uniform_data_size(checked, decl) == data_size and decl.source.bindings == bindings
					Err(_) => Bool.False
				}

			with_conflict = if (same_source) checked else add_error(checked, UniformSourceConflict({ slot: uniform.slot }))
			with_scope =
				if (existing.scope_id == checked.scope_id)
					with_conflict
				else
					add_error(with_conflict, SharedUniformScope({ uniform: name }))

			with_site =
				match existing.data_site {
					Some(site) if data_size > 0 => { ..with_scope, sites: with_scope.sites.append(SameAs(site)) }
					_ => with_scope
				}

			(with_site, id)
		}

		Err(_) => {
			(with_data, data) =
				if data_size > 0 {
					site = checked.sites.len()
					(with_schema, data_schema) = add_schema(checked, bytes_schema("${name}.data", data_size))
					(with_value, value) = add_value(with_schema, "${name}.data", Bytes({ schema: data_schema }))

					(with_value, (Some(value), Some(site)))
				} else {
					(checked, (None, None))
				}

			(data_value, data_site) = data
			(with_block, schema) = add_schema(with_data, block_schema(uniform.layout))
			id = with_block.desc.uniforms.len().to_u32_wrap()
			decl = { name, schema, source: { data: data_value, bindings } }
			interned = { slot: uniform.slot, scope_id: with_block.scope_id, data_site }
			grown = {
				..with_block,
				desc: { ..with_block.desc, uniforms: with_block.desc.uniforms.append(decl) },
				uniforms: with_block.uniforms.append(interned),
			}

			(grown, id)
		}
	}
}

## The byte size of a uniform's data value; 0 without one.
uniform_data_size : State, RenderGraphDesc.UniformDecl -> U32
uniform_data_size = |state, uniform|
	match uniform.source.data {
		None => 0
		Some(value) =>
			match state.desc.values.get(value.to_u64()) {
				Ok({ kind: Bytes({ schema }), .. }) =>
					match state.schemas.get(schema.to_u64()) {
						Ok(found) => found.size
						Err(_) => 0
					}

				_ => 0
			}
	}

## A command's push block: a schema of its own and, when it has data, a
## value carrying the bytes packed at build time.
lower_push : State, Str, [None, Some(RenderGraphLower.PushUse)] -> (State, RenderGraphDesc.MaybePush)
lower_push = |state, command_name, push|
	match push {
		None => (state, None)
		Some(push_use) => {
			(with_schema, schema) = add_schema(state, block_schema(push_use.layout))
			(with_data, data) =
				if push_use.layout.data_size > 0 {
					(with_data_schema, data_schema) = add_schema(with_schema, bytes_schema("${command_name}.push.data", push_use.layout.data_size))
					(with_value, value) = add_value(with_data_schema, "${command_name}.push.data", Bytes({ schema: data_schema }))

					(with_value, Some(value))
				} else {
					(with_schema, None)
				}

			(with_bindings, bindings) = lower_bindings(with_data, push_use.bindings)

			(with_bindings, Some({ data, schema, bindings }))
		}
	}

## Interns a pipeline by planner id; its params and push schemas are the
## first command's.
intern_pipeline : State, ResourcePlanner.PipelineRef, U32, RenderGraphDesc.MaybeU32 -> (State, U32)
intern_pipeline = |state, pipeline, params, push|
	match state.pipeline_indices.find_first_index(|index| index == pipeline.id) {
		Ok(found) => (state, found.to_u32_wrap())
		Err(_) => {
			id = state.desc.pipelines.len().to_u32_wrap()
			decl = { name: pipeline.decl.name, kind: pipeline.decl.kind, params, push }
			grown = {
				..state,
				desc: { ..state.desc, pipelines: state.desc.pipelines.append(decl) },
				pipeline_indices: state.pipeline_indices.append(pipeline.id),
			}

			(grown, id)
		}
	}

## Appends a leaf to the open scope, merging consecutive top-level draws into
## one raster pass.
add_leaf : State, LeafPass -> State
add_leaf = |state, leaf|
	match state.scope {
		Top =>
			match (leaf, state.desc.passes.last()) {
				(Raster(raster), Ok(Leaf(Raster(last)))) => {
					merged = Leaf(Raster({ ..last, draws: last.draws.concat(raster.draws) }))
					passes = state.desc.passes.drop_last(1).append(merged)

					{ ..state, desc: { ..state.desc, passes } }
				}

				_ => { ..state, desc: { ..state.desc, passes: state.desc.passes.append(Leaf(leaf)) } }
			}

		_ => { ..state, body: state.body.append(leaf) }
	}

lower_command : State, RenderGraphLower.CommandNode -> State
lower_command = |state, command| {
	checked_pipeline = check_pipeline_ref(state, command.pipeline)
	(with_uniform, uniform) = intern_uniform(checked_pipeline, command.uniform)
	# interning always leaves the uniform declared
	(params, uniform_name) =
		match with_uniform.desc.uniforms.get(uniform.to_u64()) {
			Ok(decl) => (decl.schema, decl.name)
			Err(_) => (0, "")
		}

	command_name =
		match command.call {
			Dispatch(_) => "dispatch${with_uniform.dispatches.to_str()}"
			_ => "draw${with_uniform.draws.to_str()}"
		}

	in_pipeline =
		if (command.pipeline.decl.uniform_slots.contains(command.uniform.slot))
			with_uniform
		else
			add_error(with_uniform, CommandUniformNotInPipeline({ command: command_name, uniform: uniform_name, pipeline: command.pipeline.decl.name }))

	(with_push, push) = lower_push(in_pipeline, command_name, command.push)
	push_schema =
		match push {
			Some(push_desc) => Some(push_desc.schema)
			None => None
		}

	(with_pipeline, pipeline) = intern_pipeline(with_push, command.pipeline, params, push_schema)

	match command.call {
		Dispatch(groups) => {
			dispatch = { name: command_name, pipeline, uniform, groups: Fixed({ x: groups.x, y: groups.y, z: groups.z }), push }

			add_leaf({ ..with_pipeline, dispatches: with_pipeline.dispatches + 1 }, Compute(dispatch))
		}

		_ => {
			(with_call, call) = lower_draw_call(with_pipeline, command.call)
			draw = { name: command_name, pipeline, uniform, call, push }
			raster = { name: "main", raster_targets: Main, draws: [draw] }

			add_leaf({ ..with_call, draws: with_call.draws + 1 }, Raster(raster))
		}
	}
}

lower_draw_call : State, RenderGraphLower.CommandCall -> (State, RenderGraphDesc.DrawCall)
lower_draw_call = |state, call|
	match call {
		Dispatch(_) => (state, VertexCount(0))
		VertexCount(count) => (state, VertexCount(count))
		WholeIndexed(indices) => (check_buffer_ref(state, indices), WholeIndexed({ indices: indices.id }))
		IndexRange({ indices, first_index, index_count }) =>
			(check_buffer_ref(state, indices), IndexRange({ indices: indices.id, first_index, index_count }))

		IndexedIndirect({ indices, args, first_command, draw_count }) => {
			checked = check_buffer_ref(check_buffer_ref(state, indices), args)
			offset = first_command.to_u64() * RenderGraphValidate.indirect_command_bytes.to_u64()
			(with_offset, args_offset) =
				if (offset > U32.highest.to_u64())
					(add_error(checked, BufferOffsetOverflow({ offset: offset })), U32.highest)
				else
					(checked, offset.to_u32_wrap())

			args_ref = { buffer: args.id, slot: Current, offset: args_offset, range: None }

			(with_offset, IndexedIndirect({ indices: indices.id, args: args_ref, draw_count }))
		}
	}

lower_upload : State, RenderGraphLower.UploadNode -> State
lower_upload = |state, upload| {
	checked = check_buffer_ref(state, upload.buffer)
	upload_index = checked.desc.uploads.len().to_str()
	(with_schema, elem) = add_schema(checked, bytes_schema("upload${upload_index}.elem", upload.element_size))
	(with_value, value) = add_value(
		with_schema,
		"upload${upload_index}.value",
		Array({ elem, max_len: upload.buffer.decl.len }),
	)
	decl = { name: "upload${upload_index}", buffer: upload.buffer.id, value }

	{ ..with_value, desc: { ..with_value.desc, uploads: with_value.desc.uploads.append(decl) } }
}

## Lowers a repeat or optional body. A scope inside another reports
## `NestedControlFlow` and lowers its body into the outer scope, as the Rust
## lowering does.
lower_scope : State, RenderGraphValidate.ScopeKind, List(RenderGraphLower.Node) -> State
lower_scope = |state, kind, body| {
	outer =
		match state.scope {
			Top => None
			Repeat(_) => Some(Repeat)
			Optional(_) => Some(Optional)
		}

	match outer {
		Some(outer_kind) => {
			nested = add_error(state, NestedControlFlow({ outer: outer_kind, inner: kind }))

			body.fold(nested, lower_node)
		}

		None => {
			opened = {
				..state,
				scope_id: state.next_scope_id,
				next_scope_id: state.next_scope_id + 1,
				body: [],
			}
			begun =
				match kind {
					Repeat => {
						(with_count, count) = add_value(opened, "repeat${opened.desc.passes.len().to_str()}.count", Count)

						{ ..with_count, scope: Repeat({ count: count }) }
					}

					Optional => { ..opened, scope: Optional({ first: None }) }
				}

			lowered = body.fold(begun, lower_node)
			closed = { ..lowered, scope: Top, scope_id: 0, body: [] }
			pass_index = closed.desc.passes.len().to_str()

			match lowered.scope {
				Repeat({ count }) => {
					pass = Repeat({ name: "repeat${pass_index}", count, body: lowered.body })

					{ ..closed, desc: { ..closed.desc, passes: closed.desc.passes.append(pass) } }
				}

				Optional({ first: Some(value) }) =>
					if (lowered.body.is_empty())
						closed
					else {
						pass = When({ name: "when${pass_index}", value, body: lowered.body })

						{ ..closed, desc: { ..closed.desc, passes: closed.desc.passes.append(pass) } }
					}

				Optional({ first: None }) => add_error(closed, EmptyOptionalScope)
				Top => closed
			}
		}
	}
}
