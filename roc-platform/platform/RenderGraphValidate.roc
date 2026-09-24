import RenderGraphDesc

## Pure validation of a graph description.
##
## Mirrors `crates/render-graph/src/runtime/validate.rs`: the same error
## vocabulary and messages, with the rules the platform's node set can reach.
## Texture hazard analysis, scopes, uploads, picking, and indirect draws are
## not ported yet; a graph using them fails on the Rust side at setup.
RenderGraphValidate := {}.{
	TableKind : [Texture, Buffer, Import, Value, Uniform, Pipeline, Schema]

	table_kind_str : TableKind -> Str
	table_kind_str = |table|
		match table {
			Texture => "texture"
			Buffer => "buffer"
			Import => "import"
			Value => "value"
			Uniform => "uniform"
			Pipeline => "pipeline"
			Schema => "schema"
		}

	UnsupportedFeature : [
		WindowSizeClass,
		ColorAttachmentUsage,
		DepthAttachmentUsage,
		OffscreenTargets,
		MultipleRasterPasses,
		RasterInWhen,
		GroupSourceValue,
	]

	unsupported_feature_str : UnsupportedFeature -> Str
	unsupported_feature_str = |feature|
		match feature {
			WindowSizeClass => "window-relative texture sizes (phase 5, ledger S7)"
			ColorAttachmentUsage => "color-attachment texture usage (phase 5, ledger S6)"
			DepthAttachmentUsage => "depth-attachment texture usage (phase 5, ledger S6)"
			OffscreenTargets => "offscreen raster targets (phase 5, ledger S6)"
			MultipleRasterPasses => "a second raster pass (phase 5, ledger S6)"
			RasterInWhen => "a raster pass inside an optional scope (phase 5, ledger S6)"
			GroupSourceValue => "per-frame dispatch group counts (phase 3a, ledger S1)"
		}

	ScopeKind : [Repeat, Optional]

	scope_kind_str : ScopeKind -> Str
	scope_kind_str = |scope|
		match scope {
			Repeat => "repeat"
			Optional => "optional"
		}

	pipeline_kind_str : RenderGraphDesc.PipelineKind -> Str
	pipeline_kind_str = |kind|
		match kind {
			Compute => "Compute"
			Graphics => "Graphics"
		}

	## Every error the Rust validator reports, plus the platform's own
	## declaration checks (the last group), which Rust enforces with types.
	GraphError : [
		IdOutOfRange({ table : TableKind, id : U32 }),
		DuplicateWrite({ command : Str, tex : Str }),
		MutateAndRead({ command : Str, tex : Str }),
		MutateAndWrite({ command : Str, tex : Str }),
		WriteAndPrevRead({ command : Str, tex : Str }),
		PrevReadWithoutWrite({ command : Str, tex : Str }),
		RepeatUniformRotatesTexture({ repeat : Str, uniform : Str, tex : Str }),
		RasterInRepeat({ repeat : Str }),
		PassAfterMainRaster({ pass : Str }),
		DrawWritesTexture({ draw : Str, tex : Str }),
		TextureExtentZero({ texture : Str, width : U32, height : U32 }),
		TextureExtentTooLarge({ texture : Str, width : U32, height : U32, max : U32 }),
		PipelineKindMismatch({ command : Str, expected : RenderGraphDesc.PipelineKind }),
		IndirectArgsNotImmutable({ draw : Str }),
		WhenGateNotOptional({ when : Str }),
		ValueKindMismatch({ value : Str, expected : Str, found : Str }),
		UploadTargetKind({ upload : Str, kind : Str }),
		UploadTooLarge({ upload : Str, max_len : U32, capacity : U32 }),
		BindingCountMismatch({ uniform : Str, expected : U64, found : U64 }),
		BindingKindMismatch({ uniform : Str, field : U16 }),
		UnsupportedInPhase1({ feature : UnsupportedFeature, at : Str }),
		NestedControlFlow({ outer : ScopeKind, inner : ScopeKind }),
		UniformSourceConflict({ slot : U32 }),
		MultiplePickingNodes,
		PickingWithoutDraw,
		EmptyOptionalScope,
		MutableExternalImport,
		BufferOffsetOverflow({ offset : U64 }),
		UniformIndexOutOfRange({ pipeline : Str, uniform : Str, index : U32, count : U64 }),
		PipelineUniformCount({ pipeline : Str, expected : U64, found : U64 }),
		PipelineUniformSize({ pipeline : Str, uniform : Str, size : U32, expected : U64 }),
		EmptyMesh({ pipeline : Str }),
		MeshStride({ pipeline : Str, bytes : U64, stride : U32 }),
	]

	to_str : GraphError -> Str
	to_str = |error|
		match error {
			IdOutOfRange(e) => "${table_kind_str(e.table)} id ${U32.to_str(e.id)} is undeclared"
			DuplicateWrite(e) => "${e.command} writes texture ${e.tex} more than once"
			MutateAndRead(e) => "${e.command} mutates and reads texture ${e.tex}"
			MutateAndWrite(e) => "${e.command} mutates and writes texture ${e.tex}"
			WriteAndPrevRead(e) => "${e.command} writes texture ${e.tex} and reads its previous version"
			PrevReadWithoutWrite(e) => "${e.command} reads texture ${e.tex}'s previous version, but no command writes ${e.tex}"
			RepeatUniformRotatesTexture(e) => "${e.repeat}: ${e.uniform}'s uniform block references rotating texture ${e.tex}; move the reference into the node's push block"
			RasterInRepeat(e) => "a draw node cannot appear inside repeat ${e.repeat}"
			PassAfterMainRaster(e) => "pass ${e.pass} must precede the main raster pass"
			DrawWritesTexture(e) => "draw ${e.draw} can only read graph textures (texture ${e.tex})"
			TextureExtentZero(e) => "texture ${e.texture} has a zero extent: ${U32.to_str(e.width)}x${U32.to_str(e.height)}"
			TextureExtentTooLarge(e) => "texture ${e.texture} extent ${U32.to_str(e.width)}x${U32.to_str(e.height)} exceeds the device maximum ${U32.to_str(e.max)}"
			PipelineKindMismatch(e) => "${e.command} uses the wrong pipeline kind; expected ${pipeline_kind_str(e.expected)}"
			IndirectArgsNotImmutable(e) => "draw ${e.draw}'s indirect argument buffer is not immutable"
			WhenGateNotOptional(e) => "when ${e.when} gate is not optional"
			ValueKindMismatch(e) => "value ${e.value} has kind ${e.found}, expected ${e.expected}"
			UploadTargetKind(e) => "upload ${e.upload} cannot target ${e.kind}"
			UploadTooLarge(e) => "upload ${e.upload} maximum ${U32.to_str(e.max_len)} exceeds capacity ${U32.to_str(e.capacity)}"
			BindingCountMismatch(e) => "${e.uniform} has ${U64.to_str(e.found)} bindings, expected ${U64.to_str(e.expected)}"
			BindingKindMismatch(e) => "${e.uniform} binding ${U16.to_str(e.field)} has the wrong kind"
			UnsupportedInPhase1(e) => "${e.at}: ${unsupported_feature_str(e.feature)} is unsupported in phase 1"
			NestedControlFlow(e) => "nested ${scope_kind_str(e.inner)} inside ${scope_kind_str(e.outer)} is not supported"
			UniformSourceConflict(e) => "uniform slot ${U32.to_str(e.slot)} has more than one data source"
			MultiplePickingNodes => "at most one picking node is allowed"
			PickingWithoutDraw => "a picking node needs at least one draw node"
			EmptyOptionalScope => "optional scope has no frame value"
			MutableExternalImport => "mutable external texture imports are unsupported"
			BufferOffsetOverflow(e) => "buffer offset ${U64.to_str(e.offset)} exceeds u32"
			UniformIndexOutOfRange(e) => "pipeline ${e.pipeline} binds uniform ${e.uniform} at constant buffer ${U32.to_str(e.index)}; its shader declares ${U64.to_str(e.count)}"
			PipelineUniformCount(e) => "pipeline ${e.pipeline} writes ${U64.to_str(e.found)} uniform; its shader declares ${U64.to_str(e.expected)} constant buffers"
			PipelineUniformSize(e) => "pipeline ${e.pipeline} binds uniform ${e.uniform} of ${U32.to_str(e.size)} bytes to a ${U64.to_str(e.expected)}-byte constant buffer"
			EmptyMesh(e) => "pipeline ${e.pipeline} has no vertex or index data"
			MeshStride(e) => "pipeline ${e.pipeline} has ${U64.to_str(e.bytes)} vertex bytes, not a multiple of the vertex stride ${U32.to_str(e.stride)}"
		}

	## The aggregated message the Rust facade reports for the same errors.
	validation_message : List(GraphError) -> Str
	validation_message = |errors|
		errors.fold("render graph validation failed:", |message, error| "${message}\n  - ${to_str(error)}")

	value_kind_str : RenderGraphDesc.ValueKind -> Str
	value_kind_str = |kind|
		match kind {
			Count => "count"
			Groups => "groups"
			Bytes(_) => "bytes"
			Array(_) => "array"
		}

	## Every logical problem in the description, in table order then pass
	## order. An empty list means the graph is valid.
	validate : RenderGraphDesc.GraphDesc, RenderGraphDesc.SchemaTable -> List(GraphError)
	validate = |desc, schemas| {
		schema_count = schemas.schemas.len()

		valid = |id, len, table|
			if U32.to_u64(id) >= len
				[IdOutOfRange({ table, id })]
			else
				[]

		valid_bindings = |bindings|
			bindings.join_map(
				|(_key, resource)|
					match resource {
						Tex(tex, _) => valid(tex, desc.textures.len(), Texture)
						Buf(buffer_ref, _) => valid(
							buffer_ref.buffer,
							desc.buffers.len(),
							Buffer,
						)
						External(import_id) => valid(
							import_id,
							desc.import_decls.len(),
							Import,
						)
					},
			)

		check_shape = |name, schema, bindings|
			match schemas.schemas.get(U32.to_u64(schema)) {
				Err(_) => []
				Ok(schema_desc) => {
					expected = schema_desc.resource_fields.len()
					found = bindings.len()
					count_errors =
						if found != expected
							[BindingCountMismatch({ uniform: name, expected, found })]
						else
							[]

					kind_errors = bindings
						.map_with_index(|binding, index| (binding, index))
						.join_map(
							|((key, resource), index)| {
								field_ok = match schema_desc.resource_fields.get(index) {
									Err(_) => False
									Ok(field_kind) =>
										match (field_kind, resource) {
											(SampledTex, Tex(_, Read)) => True
											(SampledTex, Tex(_, ReadPrevious)) => True
											(SampledTex, External(_)) => True
											(StorageTex, Tex(_, Write)) => True
											(StorageTex, Tex(_, Mutate)) => True
											(BufAddr, Buf(_, _)) => True
											_ => False
										}
									}

								if U16.to_u64(key) == index and field_ok {
									return []
								}

								[BindingKindMismatch({ uniform: name, field: key })]
							},
						)
					count_errors.concat(kind_errors)
				}
			}

		valid_value_bytes = |value|
			match desc.values.get(U32.to_u64(value)) {
				Err(_) => valid(value, desc.values.len(), Value)
				Ok(decl) =>
					match decl.kind {
						Bytes(_) => []
						other =>
							[
								ValueKindMismatch({
									value: decl.name,
									expected: "bytes",
									found: value_kind_str(other),
								}),
							]
						}
				}

		valid_push = |command_name, push|
			match push {
				None => []
				Some(push_desc) =>
					[
						valid(push_desc.schema, schema_count, Schema),
						match push_desc.data {
							None => []
							Some(value) => valid_value_bytes(value)
						},
						valid_bindings(push_desc.bindings),
						check_shape(command_name, push_desc.schema, push_desc.bindings),
					].join()
				}

		valid_command = |command_name, pipeline, uniform, push, expected| {
			kind_errors = match desc.pipelines.get(U32.to_u64(pipeline)) {
				Err(_) => valid(pipeline, desc.pipelines.len(), Pipeline)
				Ok(decl) =>
					if decl.kind == expected
						[]
					else
						[PipelineKindMismatch({ command: command_name, expected })]
				}

			[
				kind_errors,
				valid(uniform, desc.uniforms.len(), Uniform),
				valid_push(command_name, push),
			].join()
		}

		texture_errors = desc.textures.join_map(
			|tex| {
				size_errors = match tex.size {
					Window => [
						UnsupportedInPhase1({ feature: WindowSizeClass, at: tex.name }),
					]
					WindowDiv(_) => [
						UnsupportedInPhase1({ feature: WindowSizeClass, at: tex.name }),
					]
					Fixed(width, height) => {
						if width == 0 or height == 0 {
							[TextureExtentZero({ texture: tex.name, width, height })]
						} else {
							[]
						}
					}
				}

				usage_errors = match tex.usage {
					Storage => []
					Color => [
						UnsupportedInPhase1({ feature: ColorAttachmentUsage, at: tex.name }),
					]
					Depth => [
						UnsupportedInPhase1({ feature: DepthAttachmentUsage, at: tex.name }),
					]
				}

				size_errors.concat(usage_errors)
			},
		)

		value_errors = desc.values.join_map(
			|value|
				match value.kind {
					Bytes(bytes) => valid(bytes.schema, schema_count, Schema)
					Array(array) => valid(array.elem, schema_count, Schema)
					_ => []
				},
		)

		uniform_errors = desc.uniforms.join_map(
			|uniform|
				[
					valid(uniform.schema, schema_count, Schema),
					match uniform.source.data {
						None => []
						Some(value) => valid_value_bytes(value)
					},
					valid_bindings(uniform.source.bindings),
					check_shape(uniform.name, uniform.schema, uniform.source.bindings),
				].join(),
		)

		pipeline_errors = desc.pipelines.join_map(
			|pipeline|
				valid(pipeline.params, schema_count, Schema).concat(
					match pipeline.push {
						None => []
						Some(push) => valid(push, schema_count, Schema)
					},
				),
		)

		upload_errors = desc.uploads.join_map(
			|upload|
				valid(upload.buffer, desc.buffers.len(), Buffer).concat(
					valid(upload.value, desc.values.len(), Value),
				),
		)

		leaf_errors = |leaf|
			match leaf {
				Compute(dispatch) =>
					valid_command(
						dispatch.name,
						dispatch.pipeline,
						dispatch.uniform,
						dispatch.push,
						Compute,
					).concat(
						match dispatch.groups {
							Fixed(_, _, _) => []
							Value(_) => [
								UnsupportedInPhase1({ feature: GroupSourceValue, at: dispatch.name }),
							]
						},
					)

				Raster(raster) =>
					match raster.raster_targets {
						Main => []
						Offscreen(_) => [
							UnsupportedInPhase1({ feature: OffscreenTargets, at: raster.name }),
						]
					}.concat(
						raster.draws.join_map(
							|draw|
								valid_command(
									draw.name,
									draw.pipeline,
									draw.uniform,
									draw.push,
									Graphics,
								),
						),
					)
				}

		pass_step = |state, pass| {
			errors = match pass {
				Leaf(leaf) => {
					order =
						if state.seen_main {
							match leaf {
								Raster(_) =>
									[UnsupportedInPhase1({ feature: MultipleRasterPasses, at: "main" })]
								Compute(_) =>
									[PassAfterMainRaster({ pass: leaf_name(leaf) })]
								}
						} else {
							[]
						}
					order.concat(leaf_errors(leaf))
				}

				When(when) =>
					[
						if state.seen_main {
							[PassAfterMainRaster({ pass: when.name })]
						} else {
							[]
						},
						valid(when.value, desc.values.len(), Value),
						when.body.join_map(
							|leaf|
								match leaf {
									Raster(_) => [UnsupportedInPhase1({ feature: RasterInWhen, at: when.name })]
									Compute(_) => []
								},
						),
						when.body.join_map(leaf_errors),
					].join()

				Repeat(repeat) =>
					[
						if state.seen_main {
							[PassAfterMainRaster({ pass: repeat.name })]
						} else {
							[]
						},
						valid(repeat.count, desc.values.len(), Value),
						repeat.body.join_map(
							|leaf|
								match leaf {
									Raster(_) => [RasterInRepeat({ repeat: repeat.name })]
									Compute(_) => []
								},
						),
						repeat.body.join_map(leaf_errors),
					].join()
				}

			seen_main =
				match pass {
					Leaf(Raster(_)) => True
					_ => state.seen_main
				}

			{ seen_main, errors: state.errors.concat(errors) }
		}

		pass_errors = desc.passes.fold({ seen_main: False, errors: [] }, pass_step).errors

		[texture_errors, value_errors, uniform_errors, pipeline_errors, upload_errors, pass_errors].join()
	}

	leaf_name : RenderGraphDesc.LeafPass -> Str
	leaf_name = |leaf|
		match leaf {
			Compute(dispatch) => dispatch.name
			Raster(raster) => raster.name
		}

	expect validation_message([]) == "render graph validation failed:"
	expect validation_message([IdOutOfRange({ table: Uniform, id: 1 }), PassAfterMainRaster({ pass: "dispatch0" })]) == "render graph validation failed:\n  - uniform id 1 is undeclared\n  - pass dispatch0 must precede the main raster pass"
	expect validate(RenderGraphDesc.empty, { schemas: [] }) == []
}
