import RenderGraphDesc
import RenderGraphValidate

## Lowering of typed draw nodes into the plain-data description.
##
## Mirrors the draw path of `crates/render-graph/src/runtime/lower.rs`: uniform
## sources intern by slot, pipelines by index, and consecutive draws merge into
## the one main raster pass.
RenderGraphLower := {}.{

	## One draw, with its uniform source and the host indices it resolves to.
	DrawInput : {
		pipeline : U32,
		call : RenderGraphDesc.DrawCall,
		uniform : UniformInput,
	}

	UniformInput : {
		slot : U32,
		gpu_size : U32,
		data_size : U32,
		bindings : List((RenderGraphDesc.FieldKey, RenderGraphDesc.ResourceRef)),
	}

	Output : {
		desc : RenderGraphDesc.GraphDesc,
		schemas : RenderGraphDesc.SchemaTable,
		errors : List(RenderGraphValidate.GraphError),

		## host uniform slot per desc uniform id
		uniform_slots : List(U32),

		## host pipeline index per desc pipeline id
		pipeline_indices : List(U32),
	}

	State : {
		desc : RenderGraphDesc.GraphDesc,
		schemas : List(RenderGraphDesc.SchemaDesc),
		errors : List(RenderGraphValidate.GraphError),
		uniform_slots : List(U32),
		pipeline_indices : List(U32),
		draws : List(RenderGraphDesc.DrawDesc),
	}

	lower : List(DrawInput) -> Output
	lower = |inputs| {
		initial = {
			desc: RenderGraphDesc.empty,
			schemas: [],
			errors: [],
			uniform_slots: [],
			pipeline_indices: [],
			draws: [],
		}
		state = List.fold(inputs, initial, lower_draw)
		passes = if List.is_empty(state.draws) {
			[]
		} else {
			[Leaf(Raster({ name: "main", raster_targets: Main, draws: state.draws }))]
		}
		{
			desc: with_passes(state.desc, passes),
			schemas: { schemas: state.schemas },
			errors: state.errors,
			uniform_slots: state.uniform_slots,
			pipeline_indices: state.pipeline_indices,
		}
	}

	lower_draw : State, DrawInput -> State
	lower_draw = |state, input| {
		uniform = intern_uniform(state, input.uniform)
		params = match List.get(uniform.state.desc.uniforms, U32.to_u64(uniform.id)) {
			Ok(decl) => decl.schema
			Err(_) => crash "render graph lowering: interned uniform is missing"
		}
		pipeline = intern_pipeline(uniform.state, input.pipeline, params)
		after = pipeline.state
		draw = {
			name: "draw${U64.to_str(List.len(after.draws))}",
			pipeline: pipeline.id,
			uniform: uniform.id,
			call: input.call,
			push: None,
		}
		{
			desc: after.desc,
			schemas: after.schemas,
			errors: after.errors,
			uniform_slots: after.uniform_slots,
			pipeline_indices: after.pipeline_indices,
			draws: List.append(after.draws, draw),
		}
	}

	## The resolved binding list and the resource field kinds it implies.
	resource_fields : List((RenderGraphDesc.FieldKey, RenderGraphDesc.ResourceRef)) -> List(RenderGraphDesc.ResourceFieldKind)
	resource_fields = |bindings|
		List.map(
			bindings,
			|(_key, resource)|
				match resource {
					Tex(_, Read) => SampledTex
					Tex(_, ReadPrevious) => SampledTex
					Tex(_, Write) => StorageTex
					Tex(_, Mutate) => StorageTex
					Buf(_, _) => BufAddr
					External(_) => SampledTex
				},
		)

	intern_uniform : State, UniformInput -> { state : State, id : RenderGraphDesc.UniformId }
	intern_uniform = |state, input|
		match List.find_first_index(state.uniform_slots, |slot| slot == input.slot) {
			Ok(index) => {
				id = U64.to_u32_wrap(index)
				same_source = match List.get(state.desc.uniforms, index) {
					Err(_) => False
					Ok(existing) => uniform_data_size(state, existing) == Some(input.data_size) and existing.source.bindings == input.bindings
				}
				errors = if same_source {
					state.errors
				} else {
					List.append(state.errors, UniformSourceConflict({ slot: input.slot }))
				}
				{
					state: {
						desc: state.desc,
						schemas: state.schemas,
						errors,
						uniform_slots: state.uniform_slots,
						pipeline_indices: state.pipeline_indices,
						draws: state.draws,
					},
					id,
				}
			}
			Err(_) => {
				slot = U32.to_str(input.slot)
				data_schema = U64.to_u32_wrap(List.len(state.schemas))
				schema = data_schema + 1
				value = U64.to_u32_wrap(List.len(state.desc.values))
				id = U64.to_u32_wrap(List.len(state.desc.uniforms))
				schemas = List.concat(
					state.schemas,
					[
						{ name: "uniform${slot}.data", size: input.data_size, resource_fields: [], layout: None },
						{ name: "uniform${slot}", size: input.gpu_size, resource_fields: resource_fields(input.bindings), layout: None },
					],
				)
				value_decl = { name: "uniform${slot}.value", kind: Bytes({ schema: data_schema }), optional: False }
				uniform_decl = {
					name: "uniform${slot}",
					schema,
					source: { data: Some(value), bindings: input.bindings },
				}
				desc = state.desc
				{
					state: {
						desc: {
							textures: desc.textures,
							buffers: desc.buffers,
							import_decls: desc.import_decls,
							values: List.append(desc.values, value_decl),
							uniforms: List.append(desc.uniforms, uniform_decl),
							pipelines: desc.pipelines,
							uploads: desc.uploads,
							passes: desc.passes,
						},
						schemas,
						errors: state.errors,
						uniform_slots: List.append(state.uniform_slots, input.slot),
						pipeline_indices: state.pipeline_indices,
						draws: state.draws,
					},
					id,
				}
			}
		}

	uniform_data_size : State, RenderGraphDesc.UniformDecl -> [None, Some(U32)]
	uniform_data_size = |state, uniform|
		match uniform.source.data {
			None => None
			Some(value) =>
				match List.get(state.desc.values, U32.to_u64(value)) {
					Err(_) => None
					Ok(decl) =>
						match decl.kind {
							Bytes(bytes) =>
								match List.get(state.schemas, U32.to_u64(bytes.schema)) {
									Ok(schema) => Some(schema.size)
									Err(_) => None
								}
							_ => None
						}
					}
			}

	intern_pipeline : State, U32, RenderGraphDesc.SchemaId -> { state : State, id : RenderGraphDesc.PipelineId }
	intern_pipeline = |state, index, params|
		match List.find_first_index(state.pipeline_indices, |existing| existing == index) {
			Ok(found) => { state, id: U64.to_u32_wrap(found) }
			Err(_) => {
				id = U64.to_u32_wrap(List.len(state.desc.pipelines))
				decl = { name: "pipeline.Graphics.${U32.to_str(index)}", kind: Graphics, params, push: None }
				desc = state.desc
				{
					state: {
						desc: {
							textures: desc.textures,
							buffers: desc.buffers,
							import_decls: desc.import_decls,
							values: desc.values,
							uniforms: desc.uniforms,
							pipelines: List.append(desc.pipelines, decl),
							uploads: desc.uploads,
							passes: desc.passes,
						},
						schemas: state.schemas,
						errors: state.errors,
						uniform_slots: state.uniform_slots,
						pipeline_indices: List.append(state.pipeline_indices, index),
						draws: state.draws,
					},
					id,
				}
			}
		}

	with_passes : RenderGraphDesc.GraphDesc, List(RenderGraphDesc.PassDesc) -> RenderGraphDesc.GraphDesc
	with_passes = |desc, passes| {
		textures: desc.textures,
		buffers: desc.buffers,
		import_decls: desc.import_decls,
		values: desc.values,
		uniforms: desc.uniforms,
		pipelines: desc.pipelines,
		uploads: desc.uploads,
		passes,
	}

	two_draws : Output
	two_draws =
		lower([
			{ pipeline: 3, call: WholeIndexed, uniform: { slot: 7, gpu_size: 192, data_size: 192, bindings: [] } },
			{ pipeline: 4, call: VertexCount(3), uniform: { slot: 8, gpu_size: 16, data_size: 16, bindings: [] } },
			{ pipeline: 3, call: WholeIndexed, uniform: { slot: 7, gpu_size: 192, data_size: 192, bindings: [] } },
		])

	expect two_draws.errors == []
	expect two_draws.uniform_slots == [7, 8]
	expect two_draws.pipeline_indices == [3, 4]
	expect List.len(two_draws.desc.uniforms) == 2
	expect List.len(two_draws.schemas.schemas) == 4
	expect List.map(two_draws.desc.pipelines, |p| p.name) == ["pipeline.Graphics.3", "pipeline.Graphics.4"]
	expect
		match two_draws.desc.passes {
			[Leaf(Raster(raster))] => raster.name == "main" and List.map(raster.draws, |d| d.name) == ["draw0", "draw1", "draw2"]
			_ => False
		}
	expect RenderGraphValidate.validate(two_draws.desc, two_draws.schemas) == []

	expect
		lower([
			{ pipeline: 3, call: WholeIndexed, uniform: { slot: 7, gpu_size: 192, data_size: 192, bindings: [] } },
			{ pipeline: 3, call: WholeIndexed, uniform: { slot: 7, gpu_size: 64, data_size: 64, bindings: [] } },
		]).errors
			== [UniformSourceConflict({ slot: 7 })]
}
