import RenderGraphDesc exposing [
	BufferKind,
	BufferRef,
	Command,
	GraphDesc,
	LeafPass,
	PipelineKind,
	ResourceBinding,
	SchemaDesc,
	SchemaTable,
	ValueKind,
]
import ResourcePlanner

## Pure validation of a graph description and its resource declarations.
##
## `validate` mirrors `crates/render-graph/src/runtime/validate.rs` with the
## same error vocabulary and messages, plus the checks the Rust plain-data
## path makes when it binds a description to backend resources: block sizes,
## field layouts, and buffer address bounds. `validate_resources` checks the
## contents a `ResourcePlanner` declares, which the Rust renderer checks when
## it creates the buffers: index bounds, capacities, and indirect commands.
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

	pipeline_kind_str : PipelineKind -> Str
	pipeline_kind_str = |kind|
		match kind {
			Compute => "Compute"
			Graphics => "Graphics"
			GraphicsIndirect => "GraphicsIndirect"
		}

	buffer_kind_str : BufferKind -> Str
	buffer_kind_str = |kind|
		match kind {
			GpuUpdatedFlight => "GpuUpdatedFlight"
			Singleton => "Singleton"
			Immutable => "Immutable"
			Storage => "Storage"
			Index => "Index"
		}

	## Every error the Rust validator reports, then the checks the Rust
	## plain-data path makes at binding time, then the Roc platform's own
	## declaration checks, which Rust enforces with types or at resource
	## creation.
	GraphError : [
		IdOutOfRange({ table : TableKind, id : U32 }),
		DuplicateWrite({ command : Str, tex : Str }),
		MutateAndRead({ command : Str, tex : Str }),
		MutateAndWrite({ command : Str, tex : Str }),
		WriteAndPrevRead({ command : Str, tex : Str }),
		PrevReadWithoutWrite({ command : Str, tex : Str }),
		RepeatUniformRotatesTexture({ repeat : Str, uniform : Str, tex : Str }),
		SharedUniformRotatesTexture({ uniform : Str, writer : Str, tex : Str }),
		RasterInRepeat({ repeat : Str }),
		PassAfterMainRaster({ pass : Str }),
		DrawWritesTexture({ draw : Str, tex : Str }),
		TextureExtentZero({ texture : Str, width : U32, height : U32 }),
		PipelineKindMismatch({ command : Str, expected : PipelineKind }),
		IndirectArgsNotImmutable({ draw : Str }),
		DrawIndicesNotIndexBuffer({ draw : Str }),
		IndexRangeOutOfBounds({ draw : Str, first_index : U32, index_count : U32, capacity : U32 }),
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
		LayoutFieldOutOfBounds({ schema : Str, field : U16, offset : U32, len : U32, size : U32 }),
		LayoutFieldsOverlap({ schema : Str, first : U16, second : U16 }),
		LayoutResourceFieldSize({ schema : Str, field : U16, len : U32 }),
		LayoutResourceFieldMismatch({ schema : Str, field : U16 }),
		LayoutDataWithoutValue({ block : Str }),
		LayoutDataOutOfBounds({ block : Str, field : U16, src_offset : U32, len : U32, data_size : U32 }),
		PushConstantSize({ schema : Str, size : U32 }),
		ValueScopeInvalid({ value : Str, reason : Str }),
		UploadElemSizeMismatch({ upload : Str, value_elem_size : U32, buffer_elem_size : U32 }),
		BufferAccessMismatch({ block : Str, field : U16, kind : BufferKind }),
		PickingCursorShape,

		## Rust plain-data binding checks.
		SchemaWithoutLayout({ schema : Str }),
		GroupsValueUnsupported({ value : Str }),
		ZeroSizedArrayElement({ value : Str }),
		CommandBlockSize({ command : Str, block : Str, size : U32, pipeline : Str, expected : U32 }),
		BufferOffsetOutOfBounds({ block : Str, field : U16, offset : U32, buffer : Str, byte_len : U64 }),
		IndirectCommandsOutOfBounds({ draw : Str, first_command : U32, draw_count : U32, len : U32 }),

		## Roc declaration checks.
		EmptyIndexBuffer({ buffer : Str }),
		ZeroVertexCount({ buffer : Str }),
		IndexOutOfBounds({ buffer : Str, position : U64, index : U32, vertex_count : U32 }),
		EmptyBuffer({ buffer : Str }),
		ZeroElementSize({ buffer : Str }),
		BufferTooLarge({ buffer : Str, len : U32, element_size : U32 }),
		InitialItemsExceedCapacity({ buffer : Str, count : U64, capacity : U32 }),
		IndirectArgsNotIndirectBuffer({ draw : Str, buffer : Str }),
		ZeroSizedUniform({ uniform : Str }),
		PipelineUniformUndeclared({ pipeline : Str, slot : U32 }),
		PipelineUniformCount({ pipeline : Str, count : U64 }),
		PipelineUniformMismatch({ pipeline : Str, uniform : Str, found : Str, expected : Str }),
		CommandUniformNotInPipeline({ command : Str, uniform : Str, pipeline : Str }),
		SharedUniformScope({ uniform : Str }),
		ForeignDeclaration({ table : TableKind, id : U32, name : Str }),
	]

	to_str : GraphError -> Str
	to_str = |error|
		match error {
			IdOutOfRange(e) => "${table_kind_str(e.table)} id ${e.id.to_str()} is undeclared"
			DuplicateWrite(e) => "${e.command} writes texture ${e.tex} more than once"
			MutateAndRead(e) => "${e.command} mutates and reads texture ${e.tex}"
			MutateAndWrite(e) => "${e.command} mutates and writes texture ${e.tex}"
			WriteAndPrevRead(e) => "${e.command} writes texture ${e.tex} and reads its previous version"
			PrevReadWithoutWrite(e) => "${e.command} reads texture ${e.tex}'s previous version, but no command writes ${e.tex}"
			RepeatUniformRotatesTexture(e) => "${e.repeat}: ${e.uniform}'s uniform block references rotating texture ${e.tex}; move the reference into the node's push block"
			SharedUniformRotatesTexture(e) => "${e.uniform}'s uniform block is shared by several commands, but ${e.writer} writes texture ${e.tex}, which the block references, between them; bind ${e.tex} through each command's push block instead"
			RasterInRepeat(e) => "a draw node cannot appear inside repeat ${e.repeat}"
			PassAfterMainRaster(e) => "pass ${e.pass} must precede the main raster pass"
			DrawWritesTexture(e) => "draw ${e.draw} can only read graph textures (texture ${e.tex})"
			TextureExtentZero(e) => "texture ${e.texture} has a zero extent: ${e.width.to_str()}x${e.height.to_str()}"
			PipelineKindMismatch(e) => "${e.command} uses the wrong pipeline kind; expected ${pipeline_kind_str(e.expected)}"
			IndirectArgsNotImmutable(e) => "draw ${e.draw}'s indirect argument buffer is not immutable"
			DrawIndicesNotIndexBuffer(e) => "draw ${e.draw}'s indices are not an index buffer"
			IndexRangeOutOfBounds(e) => "draw ${e.draw}'s index range [${e.first_index.to_str()}, ${e.first_index.to_str()} + ${e.index_count.to_str()}) is out of bounds for an index buffer of ${e.capacity.to_str()} indices"
			WhenGateNotOptional(e) => "when ${e.when} gate is not optional"
			ValueKindMismatch(e) => "value ${e.value} has kind ${e.found}, expected ${e.expected}"
			UploadTargetKind(e) => "upload ${e.upload} cannot target ${e.kind}"
			UploadTooLarge(e) => "upload ${e.upload} maximum ${e.max_len.to_str()} exceeds capacity ${e.capacity.to_str()}"
			BindingCountMismatch(e) => "${e.uniform} has ${e.found.to_str()} bindings, expected ${e.expected.to_str()}"
			BindingKindMismatch(e) => "${e.uniform} binding ${e.field.to_str()} has the wrong kind"
			UnsupportedInPhase1(e) => "${e.at}: ${unsupported_feature_str(e.feature)} is unsupported in phase 1"
			NestedControlFlow(e) => "nested ${scope_kind_str(e.inner)} inside ${scope_kind_str(e.outer)} is not supported"
			UniformSourceConflict(e) => "uniform slot ${e.slot.to_str()} has more than one data source"
			MultiplePickingNodes => "at most one picking node is allowed"
			PickingWithoutDraw => "a picking node needs at least one draw node"
			EmptyOptionalScope => "optional scope has no frame value"
			MutableExternalImport => "mutable external texture imports are unsupported"
			BufferOffsetOverflow(e) => "buffer offset ${e.offset.to_str()} exceeds u32"
			LayoutFieldOutOfBounds(e) => "schema ${e.schema} field ${e.field.to_str()} [${e.offset.to_str()}, ${e.offset.to_str()} + ${e.len.to_str()}) does not fit in its ${e.size.to_str()}-byte block"
			LayoutFieldsOverlap(e) => "schema ${e.schema} fields ${e.first.to_str()} and ${e.second.to_str()} overlap"
			LayoutResourceFieldSize(e) => "schema ${e.schema} resource field ${e.field.to_str()} is ${e.len.to_str()} bytes; handles and addresses are 8 bytes"
			LayoutResourceFieldMismatch(e) => "schema ${e.schema} layout field ${e.field.to_str()} does not match its resource fields; each resource field needs exactly one layout field of the same kind"
			LayoutDataWithoutValue(e) => "${e.block} lays out data fields but names no data value"
			LayoutDataOutOfBounds(e) => "${e.block} data field ${e.field.to_str()} reads [${e.src_offset.to_str()}, ${e.src_offset.to_str()} + ${e.len.to_str()}) past its ${e.data_size.to_str()}-byte data value"
			PushConstantSize(e) => "push block ${e.schema} is ${e.size.to_str()} bytes; vkCmdPushConstants needs a nonzero multiple of 4 of at most ${max_push_constant_bytes.to_str()}"
			ValueScopeInvalid(e) => "value ${e.value} has an invalid scope: ${e.reason}"
			UploadElemSizeMismatch(e) => "upload ${e.upload} elements are ${e.value_elem_size.to_str()} bytes, but its buffer's elements are ${e.buffer_elem_size.to_str()} bytes"
			BufferAccessMismatch(e) => "${e.block} binding ${e.field.to_str()} names a ${buffer_kind_str(e.kind)} buffer with an access or slot it does not support"
			PickingCursorShape => "the picking cursor must be an 8-byte bytes value"
			SchemaWithoutLayout(e) => "schema ${e.schema} has no field layout"
			GroupsValueUnsupported(e) => "value ${e.value}: per-frame group counts are unsupported"
			ZeroSizedArrayElement(e) => "value ${e.value} has zero-sized elements"
			CommandBlockSize(e) => "${e.command} writes a ${e.size.to_str()}-byte ${e.block} block to pipeline ${e.pipeline}, which expects ${e.expected.to_str()} bytes"
			BufferOffsetOutOfBounds(e) => "${e.block} binding ${e.field.to_str()} offset ${e.offset.to_str()} is past the end of buffer ${e.buffer} (${e.byte_len.to_str()} bytes)"
			IndirectCommandsOutOfBounds(e) => "draw ${e.draw}'s indirect commands [${e.first_command.to_str()}, ${e.first_command.to_str()} + ${e.draw_count.to_str()}) are not a nonempty range inside an argument buffer of ${e.len.to_str()} commands"
			EmptyIndexBuffer(e) => "index buffer ${e.buffer} has no indices"
			ZeroVertexCount(e) => "index buffer ${e.buffer} has a vertex count of 0; pass the number of vertices its indices address"
			IndexOutOfBounds(e) => "index buffer ${e.buffer} index ${e.index.to_str()} at position ${e.position.to_str()} is not below its vertex count ${e.vertex_count.to_str()}"
			EmptyBuffer(e) => "buffer ${e.buffer} has no elements"
			ZeroElementSize(e) => "buffer ${e.buffer} has zero-sized elements"
			BufferTooLarge(e) => "buffer ${e.buffer} of ${e.len.to_str()} ${e.element_size.to_str()}-byte elements does not fit in a u32 byte offset"
			InitialItemsExceedCapacity(e) => "buffer ${e.buffer} starts with ${e.count.to_str()} elements but holds at most ${e.capacity.to_str()}"
			IndirectArgsNotIndirectBuffer(e) => "draw ${e.draw}'s argument buffer ${e.buffer} was not declared with ResourcePlanner.indirect_buffer"
			ZeroSizedUniform(e) => "uniform buffer ${e.uniform} has a zero-sized block"
			PipelineUniformUndeclared(e) => "pipeline ${e.pipeline} reads uniform slot ${e.slot.to_str()}, which its resource planner does not declare"
			PipelineUniformCount(e) => "pipeline ${e.pipeline}'s shader declares ${e.count.to_str()} ParameterBlock globals; a render graph shader needs exactly one ParameterBlock<T>"
			PipelineUniformMismatch(e) => "pipeline ${e.pipeline} reads uniform ${e.uniform} (${e.found}) for its shader's ParameterBlock (${e.expected})"
			CommandUniformNotInPipeline(e) => "${e.command} writes uniform ${e.uniform}, which pipeline ${e.pipeline} does not read"
			SharedUniformScope(e) => "uniform ${e.uniform} is written by commands in different repeat or optional scopes; give each scope its own uniform buffer"
			ForeignDeclaration(e) => "${table_kind_str(e.table)} ${e.name} (id ${e.id.to_str()}) was declared by a different resource planner than the one passed to RenderGraph.new"
		}

	## The aggregated message the Rust validator reports for the same errors.
	validation_message : List(GraphError) -> Str
	validation_message = |errors|
		errors.fold("render graph validation failed:", |message, error| "${message}\n  - ${to_str(error)}")

	## Vulkan's guaranteed `maxPushConstantsSize`.
	max_push_constant_bytes : U32
	max_push_constant_bytes = 128

	## The byte size of every resource field: a bindless handle is a Slang
	## `uint2`, and a buffer device address is a `uint64_t`.
	resource_field_bytes : U32
	resource_field_bytes = 8

	## Bytes per `vkCmdDrawIndexedIndirect` record.
	indirect_command_bytes : U32
	indirect_command_bytes = 20

	## Physical images per logical texture, by index.
	Analysis : { tex_phys : List(U32) }

	value_kind_str : ValueKind -> Str
	value_kind_str = |kind|
		match kind {
			Count => "count"
			Groups => "groups"
			Bytes(_) => "bytes"
			Array(_) => "array"
		}

	## Every logical problem in the description, or the physical image count
	## of each logical texture.
	validate : GraphDesc, SchemaTable -> Try(Analysis, List(GraphError))
	validate = |desc, schemas| {
		schema_count = schemas.schemas.len()
		texture_errors = desc.textures.join_map(check_texture)
		layout_errors = schemas.schemas.join_map(check_layout)
		value_schema_errors = desc.values.join_map(
			|value|
				match value.kind {
					Bytes({ schema }) => valid(schema, schema_count, Schema)
					Array({ elem, .. }) => valid(elem, schema_count, Schema)
					Count => []
					Groups => []
				},
		)
		pipeline_errors = desc.pipelines.join_map(
			|pipeline| {
				push_errors =
					match pipeline.push {
						Some(push) => valid(push, schema_count, Schema)
						None => []
					}

				valid(pipeline.params, schema_count, Schema).concat(push_errors)
			},
		)
		uniform_errors = desc.uniforms.join_map(
			|uniform| {
				data_errors =
					match uniform.source.data {
						Some(value) => value_is_bytes(desc, value)
						None => []
					}

				[
					valid(uniform.schema, schema_count, Schema),
					data_errors,
					valid_bindings(desc, uniform.name, uniform.source.bindings),
					check_layout_data(uniform.name, uniform.schema, uniform.source.data, desc, schemas),
					check_shape(uniform.name, uniform.schema, uniform.source.bindings, schemas),
				].join()
			},
		)
		upload_errors = desc.uploads.join_map(|upload| check_upload(desc, schemas, upload))
		pass_errors = check_passes(desc)
		hazards = analyze_textures(desc)
		leaf_errors = RenderGraphDesc.leaves(desc).join_map(|leaf| check_leaf(desc, schemas, leaf))

		errors = [
			texture_errors,
			layout_errors,
			check_push_sizes(desc, schemas),
			check_value_scopes(desc),
			value_schema_errors,
			pipeline_errors,
			uniform_errors,
			upload_errors,
			pass_errors,
			hazards.errors,
			leaf_errors,
			check_picking(desc, schemas),
			check_shared_uniforms(desc),
			hazards.prev_read_errors,
			check_plain_bindings(desc, schemas),
		].join()

		if (errors.is_empty()) Ok({ tex_phys: hazards.tex_phys }) else Err(errors)
	}

	## The contents each declaration carries, checked before the host creates
	## anything: index bounds, capacities, element sizes, uniform sizes, the
	## uniform slots each pipeline reads, and each indirect draw's argument
	## buffer and command range.
	validate_resources : ResourcePlanner, GraphDesc -> List(GraphError)
	validate_resources = |planner, desc| {
		buffer_errors = planner.buffers().join_map(check_planned_buffer)
		uniform_errors = planner.uniforms().join_map(
			|uniform|
				if (uniform.layout.gpu_size == 0)
					[ZeroSizedUniform({ uniform: uniform.name })]
				else
					[],
		)
		pipeline_errors = planner.pipelines().join_map(|pipeline| check_pipeline_uniform(planner, pipeline))
		indirect_errors = RenderGraphDesc.leaves(desc).join_map(
			|leaf|
				match leaf {
					Raster(raster) => raster.draws.join_map(|draw| check_indirect_args(planner, draw))
					Compute(_) => []
				},
		)

		[buffer_errors, uniform_errors, pipeline_errors, indirect_errors].join()
	}
}

## `[id]` if `id` indexes a table of `len` entries, else an
## `IdOutOfRange` error.
valid : U32, U64, RenderGraphValidate.TableKind -> List(RenderGraphValidate.GraphError)
valid = |id, len, table| if (id.to_u64() >= len) [IdOutOfRange({ table, id })] else []

in_range : U32, U64 -> Bool
in_range = |id, len| id.to_u64() < len

## True if `[first, first + count)` fits in `total` elements.
range_in_bounds : U32, U32, U32 -> Bool
range_in_bounds = |first, count, total| first.to_u64() + count.to_u64() <= total.to_u64()

tex_name : GraphDesc, U32 -> Str
tex_name = |desc, tex|
	match desc.textures.get(tex.to_u64()) {
		Ok(decl) => decl.name
		Err(_) => "tex${tex.to_str()}"
	}

check_texture : RenderGraphDesc.TexDecl -> List(RenderGraphValidate.GraphError)
check_texture = |tex| {
	size_errors =
		match tex.size {
			Window => [UnsupportedInPhase1({ feature: WindowSizeClass, at: tex.name })]
			WindowDiv(_) => [UnsupportedInPhase1({ feature: WindowSizeClass, at: tex.name })]
			Fixed(width, height) =>
				if (width == 0 or height == 0)
					[TextureExtentZero({ texture: tex.name, width, height })]
				else
					[]
		}

	usage_errors =
		match tex.usage {
			Storage => []
			Color => [UnsupportedInPhase1({ feature: ColorAttachmentUsage, at: tex.name })]
			Depth => [UnsupportedInPhase1({ feature: DepthAttachmentUsage, at: tex.name })]
		}

	size_errors.concat(usage_errors)
}

layout_resource_kind : RenderGraphDesc.SchemaFieldKind -> [None, Some(RenderGraphDesc.ResourceFieldKind)]
layout_resource_kind = |kind|
	match kind {
		Data(_) => None
		SampledTex => Some(SampledTex)
		StorageTex => Some(StorageTex)
		BufAddr => Some(BufAddr)
	}

## A layout's fields fit in the block without overlapping, and its resource
## fields are 8-byte slots that correspond one-to-one with `resource_fields`.
check_layout : SchemaDesc -> List(RenderGraphValidate.GraphError)
check_layout = |schema| {
	layout =
		match schema.layout {
			Some(fields) => fields
			None => return []
		}

	var $errors = []
	var $resource_seen = List.repeat(Bool.False, schema.resource_fields.len())
	var $spans = []
	for field in layout.fields {
		end = field.offset.to_u64() + field.len.to_u64()
		if end > schema.size.to_u64() {
			$errors = $errors.append(
				LayoutFieldOutOfBounds({
					schema: schema.name,
					field: field.key,
					offset: field.offset,
					len: field.len,
					size: schema.size,
				}),
			)
		}
		$spans = $spans.append((field.offset.to_u64(), end, field.key))

		match layout_resource_kind(field.kind) {
			None => {}
			Some(kind) => {
				if field.len != RenderGraphValidate.resource_field_bytes {
					$errors = $errors.append(
						LayoutResourceFieldSize({ schema: schema.name, field: field.key, len: field.len }),
					)
				}
				index = field.key.to_u64()
				first_match =
					schema.resource_fields.get(index) == Ok(kind)
						and $resource_seen.get(index) == Ok(Bool.False)
				if first_match {
					$resource_seen = $resource_seen.set(index, Bool.True) ?? $resource_seen
				} else {
					$errors = $errors.append(
						LayoutResourceFieldMismatch({ schema: schema.name, field: field.key }),
					)
				}
			}
		}
	}

	unseen = $resource_seen.map_with_index(|seen, index| (seen, index)).join_map(
		|(seen, index)|
			if (seen)
				[]
			else
				[LayoutResourceFieldMismatch({ schema: schema.name, field: index.to_u16_wrap() })],
	)

	sorted = $spans.sort_with(compare_spans)
	var $furthest = None
	for (start, end, key) in sorted {
		match $furthest {
			Some((furthest_end, furthest_key)) => {
				if start < furthest_end {
					$errors = $errors.append(
						LayoutFieldsOverlap({ schema: schema.name, first: furthest_key, second: key }),
					)
				}
				if end > furthest_end {
					$furthest = Some((end, key))
				}
			}
			None => {
				$furthest = Some((end, key))
			}
		}
	}

	$errors.concat(unseen)
}

compare_spans : (U64, U64, U16), (U64, U64, U16) -> [Before, Same, After]
compare_spans = |(start_a, end_a, key_a), (start_b, end_b, key_b)|
	if (start_a != start_b)
		compare_u64(start_a, start_b)
	else if (end_a != end_b)
		compare_u64(end_a, end_b)
	else
		compare_u64(key_a.to_u64(), key_b.to_u64())

compare_u64 : U64, U64 -> [Before, Same, After]
compare_u64 = |left, right|
	if (left < right)
		Before
	else if (left > right)
		After
	else
		Same

## `vkCmdPushConstants` takes a nonzero size that is a multiple of 4, and
## Vulkan guarantees 128 bytes.
push_size_valid : U32 -> Bool
push_size_valid = |size|
	size > 0 and size % 4 == 0 and size <= RenderGraphValidate.max_push_constant_bytes

## Each distinct schema id among `ids`, in ascending order.
dedup_ids : List(U32) -> List(U32)
dedup_ids = |ids|
	ids.sort_with(compare_u32).fold(
		[],
		|unique, id|
			if (unique.last() == Ok(id))
				unique
			else
				unique.append(id),
	)

compare_u32 : U32, U32 -> [Before, Same, After]
compare_u32 = |left, right| compare_u64(left.to_u64(), right.to_u64())

all_commands : GraphDesc -> List(Command)
all_commands = |desc| RenderGraphDesc.leaves(desc).join_map(RenderGraphDesc.commands)

## Every schema a pipeline or command uses as a push block, once each.
push_schema_ids : GraphDesc -> List(U32)
push_schema_ids = |desc| {
	pipeline_push = desc.pipelines.join_map(
		|pipeline|
			match pipeline.push {
				Some(schema) => [schema]
				None => []
			},
	)
	command_push = all_commands(desc).join_map(
		|command|
			match command.push {
				Some(push) => [push.schema]
				None => []
			},
	)

	dedup_ids(pipeline_push.concat(command_push))
}

check_push_sizes : GraphDesc, SchemaTable -> List(RenderGraphValidate.GraphError)
check_push_sizes = |desc, schemas|
	push_schema_ids(desc).join_map(
		|id|
			match schemas.schemas.get(id.to_u64()) {
				Ok(schema) if !push_size_valid(schema.size) =>
					[PushConstantSize({ schema: schema.name, size: schema.size })]
				_ => []
			},
	)

## A value's scope names a gate or count of the right shape: a `When` gate is
## an optional value gated by itself, and a `Repeat` count is a top-level
## count. Values inside a repeat cannot be optional.
check_value_scopes : GraphDesc -> List(RenderGraphValidate.GraphError)
check_value_scopes = |desc|
	desc.values.join_map(
		|value| {
			reason =
				match value.scope {
					Top => None
					When({ gate }) =>
						match desc.values.get(gate.to_u64()) {
							Err(_) => Some("its gate is undeclared")
							Ok(_) if !value.optional => Some("a value in an optional scope must be optional")
							Ok(gate_decl) if gate_decl.scope != When({ gate: gate }) => Some("its gate is not gated by itself")
							Ok(_) => None
						}

					Repeat({ count }) =>
						match desc.values.get(count.to_u64()) {
							Err(_) => Some("its repeat count is undeclared")
							Ok(_) if value.optional => Some("a value in a repeat cannot be optional")
							Ok(count_decl) if count_decl.kind != Count or count_decl.scope != Top => Some("its repeat count is not a top-level count")
							Ok(_) => None
						}
				}

			match reason {
				Some(text) => [ValueScopeInvalid({ value: value.name, reason: text })]
				None => []
			}
		},
	)

## The value is declared as a bytes value.
value_is_bytes : GraphDesc, U32 -> List(RenderGraphValidate.GraphError)
value_is_bytes = |desc, value|
	match desc.values.get(value.to_u64()) {
		Err(_) => valid(value, desc.values.len(), Value)
		Ok(decl) =>
			match decl.kind {
				Bytes(_) => []
				other =>
					[
						ValueKindMismatch({
							value: decl.name,
							expected: "bytes",
							found: RenderGraphValidate.value_kind_str(other),
						}),
					]
			}
	}

## A layout's data fields read inside the block's bytes data value.
check_layout_data : Str, U32, RenderGraphDesc.MaybeU32, GraphDesc, SchemaTable -> List(RenderGraphValidate.GraphError)
check_layout_data = |block, schema, data, desc, schemas| {
	fields =
		match schemas.schemas.get(schema.to_u64()) {
			Ok({ layout: Some(layout), .. }) => layout.fields
			_ => return []
		}

	data_fields = fields.join_map(
		|field|
			match field.kind {
				Data({ src_offset }) => [(field, src_offset)]
				_ => []
			},
	)
	value =
		match data {
			Some(id) => id
			None =>
				return if (data_fields.is_empty()) [] else [LayoutDataWithoutValue({ block: block })]
		}

	# a missing or mistyped value is reported where the value is checked
	data_size =
		match desc.values.get(value.to_u64()) {
			Ok({ kind: Bytes({ schema: data_schema }), .. }) =>
				match schemas.schemas.get(data_schema.to_u64()) {
					Ok(found) => found.size
					Err(_) => return []
				}

			_ => return []
		}

	data_fields.join_map(
		|(field, src_offset)|
			if (src_offset.to_u64() + field.len.to_u64() <= data_size.to_u64())
				[]
			else
				[
					LayoutDataOutOfBounds({
						block,
						field: field.key,
						src_offset,
						len: field.len,
						data_size,
					}),
				],
	)
}

## A buffer binding's slot and access must be ones its buffer kind provides:
## index buffers have no address, only GPU-updated buffers keep a previous
## slot, and immutable and singleton buffers are read-only.
buffer_access_supported : BufferKind, BufferRef, RenderGraphDesc.BufAccess -> Bool
buffer_access_supported = |kind, buffer, access|
	match kind {
		Index => Bool.False
		Singleton => buffer.slot == Current and access == Read
		Immutable => buffer.slot == Current and access == Read
		Storage => buffer.slot == Current
		GpuUpdatedFlight => buffer.slot == Current or access == Read
	}

## Resource ids are checked on the declaration that carries them, so an
## unconsumed source is still checked and a shared one reports once.
valid_bindings : GraphDesc, Str, List(ResourceBinding) -> List(RenderGraphValidate.GraphError)
valid_bindings = |desc, block, bindings|
	bindings.join_map(
		|{ key, resource }|
			match resource {
				Tex(tex, _) => valid(tex, desc.textures.len(), Texture)
				External(import_id) => valid(import_id, desc.import_decls.len(), Import)
				Buf(buffer, access) =>
					match desc.buffers.get(buffer.buffer.to_u64()) {
						Err(_) => valid(buffer.buffer, desc.buffers.len(), Buffer)
						Ok(decl) =>
							if (buffer_access_supported(decl.kind, buffer, access))
								[]
							else
								[BufferAccessMismatch({ block, field: key, kind: decl.kind })]
					}
			},
	)

check_shape : Str, U32, List(ResourceBinding), SchemaTable -> List(RenderGraphValidate.GraphError)
check_shape = |name, schema, bindings, schemas| {
	schema_desc =
		match schemas.schemas.get(schema.to_u64()) {
			Ok(found) => found
			Err(_) => return []
		}

	expected = schema_desc.resource_fields.len()
	found = bindings.len()
	count_errors =
		if (found != expected)
			[BindingCountMismatch({ uniform: name, expected, found })]
		else
			[]

	kind_errors = bindings.map_with_index(|binding, index| (binding, index)).join_map(
		|({ key, resource }, index)| {
			field_ok =
				match schema_desc.resource_fields.get(index) {
					Err(_) => Bool.False
					Ok(field_kind) =>
						match (field_kind, resource) {
							(SampledTex, Tex(_, Read)) => Bool.True
							(SampledTex, Tex(_, ReadPrevious)) => Bool.True
							(SampledTex, External(_)) => Bool.True
							(StorageTex, Tex(_, Write)) => Bool.True
							(StorageTex, Tex(_, Mutate)) => Bool.True
							(BufAddr, Buf(_, _)) => Bool.True
							_ => Bool.False
						}
				}

			if (key.to_u64() == index and field_ok)
				[]
			else
				[BindingKindMismatch({ uniform: name, field: key })]
		},
	)

	count_errors.concat(kind_errors)
}

check_upload : GraphDesc, SchemaTable, RenderGraphDesc.UploadDesc -> List(RenderGraphValidate.GraphError)
check_upload = |desc, schemas, upload| {
	id_errors = valid(upload.buffer, desc.buffers.len(), Buffer).concat(valid(upload.value, desc.values.len(), Value))
	(buffer, value) =
		match (desc.buffers.get(upload.buffer.to_u64()), desc.values.get(upload.value.to_u64())) {
			(Ok(found_buffer), Ok(found_value)) => (found_buffer, found_value)
			_ => return id_errors
		}

	# only CPU-updated storage takes per-frame writes; an immutable buffer is
	# never written while a frame may read it
	target_errors =
		match buffer.kind {
			Storage => []
			GpuUpdatedFlight => [UploadTargetKind({ upload: upload.name, kind: "gpu-updated" })]
			Singleton => [UploadTargetKind({ upload: upload.name, kind: "singleton" })]
			Index => [UploadTargetKind({ upload: upload.name, kind: "index" })]
			Immutable => [UploadTargetKind({ upload: upload.name, kind: "immutable" })]
		}

	shape_errors =
		match value.kind {
			Array({ elem, max_len }) => {
				size_errors =
					match (buffer.elem_size, schemas.schemas.get(elem.to_u64())) {
						(Some(buffer_elem_size), Ok(elem_schema)) if elem_schema.size != buffer_elem_size =>
							[
								UploadElemSizeMismatch({
									upload: upload.name,
									value_elem_size: elem_schema.size,
									buffer_elem_size,
								}),
							]

						_ => []
					}

				capacity_errors =
					match buffer.capacity {
						Some(capacity) if max_len > capacity =>
							[UploadTooLarge({ upload: upload.name, max_len, capacity })]

						_ => []
					}

				size_errors.concat(capacity_errors)
			}

			other =>
				[
					ValueKindMismatch({
						value: value.name,
						expected: "array",
						found: RenderGraphValidate.value_kind_str(other),
					}),
				]
		}

	[id_errors, target_errors, shape_errors].join()
}

pass_name : RenderGraphDesc.PassDesc -> Str
pass_name = |pass|
	match pass {
		Leaf(Compute(dispatch)) => dispatch.name
		Leaf(Raster(raster)) => raster.name
		When({ name, .. }) => name
		Repeat({ name, .. }) => name
	}

is_raster : LeafPass -> Bool
is_raster = |leaf|
	match leaf {
		Raster(_) => Bool.True
		Compute(_) => Bool.False
	}

check_passes : GraphDesc -> List(RenderGraphValidate.GraphError)
check_passes = |desc| {
	var $errors = []
	var $raster_seen = Bool.False
	for pass in desc.passes {
		is_leaf_raster =
			match pass {
				Leaf(leaf) => is_raster(leaf)
				_ => Bool.False
			}
		if $raster_seen and !is_leaf_raster {
			$errors = $errors.append(PassAfterMainRaster({ pass: pass_name(pass) }))
		}

		pass_errors =
			match pass {
				When({ name, value, body }) => {
					gate_errors =
						match desc.values.get(value.to_u64()) {
							Err(_) => valid(value, desc.values.len(), Value)
							Ok(gate) =>
								if (!gate.optional)
									[WhenGateNotOptional({ when: name })]
								else if (gate.scope != When({ gate: value }))
									[ValueScopeInvalid({ value: gate.name, reason: "a when pass's gate must be gated by itself" })]
								else
									[]
						}

					raster_errors =
						if (body.any(is_raster))
							[UnsupportedInPhase1({ feature: RasterInWhen, at: name })]
						else
							[]

					gate_errors.concat(raster_errors)
				}

				Repeat({ name, count, body }) => check_repeat(desc, name, count, body)
				Leaf(Raster(raster)) =>
					if ($raster_seen)
						[UnsupportedInPhase1({ feature: MultipleRasterPasses, at: raster.name })]
					else
						[]

				Leaf(Compute(_)) => []
			}

		$errors = $errors.concat(pass_errors)
		if is_leaf_raster {
			$raster_seen = Bool.True
		}
	}

	$errors
}

check_repeat : GraphDesc, Str, U32, List(LeafPass) -> List(RenderGraphValidate.GraphError)
check_repeat = |desc, name, count, body| {
	count_errors =
		match desc.values.get(count.to_u64()) {
			Err(_) => valid(count, desc.values.len(), Value)
			Ok(value) if value.kind != Count =>
				[
					ValueKindMismatch({
						value: value.name,
						expected: "count",
						found: RenderGraphValidate.value_kind_str(value.kind),
					}),
				]

			Ok(_) => []
		}

	raster_errors = if (body.any(is_raster)) [RasterInRepeat({ repeat: name })] else []

	rotating = body
		.join_map(RenderGraphDesc.commands)
		.join_map(|command| RenderGraphDesc.command_bindings(desc, command))
		.join_map(
			|{ resource, .. }|
				match resource {
					Tex(tex, Write) => [tex]
					_ => []
				},
		)

	# a uniform shared by several body commands reports once
	var $reported = []
	var $rotation_errors = []
	for leaf in body {
		match leaf {
			Compute(dispatch) =>
				match desc.uniforms.get(dispatch.uniform.to_u64()) {
					Ok(uniform) =>
						for { resource, .. } in uniform.source.bindings {
							match resource {
								Tex(tex, _) => {
									pair = (dispatch.uniform, tex)
									if rotating.contains(tex) and !$reported.contains(pair) {
										$reported = $reported.append(pair)
										$rotation_errors = $rotation_errors.append(
											RepeatUniformRotatesTexture({
												repeat: name,
												uniform: uniform.name,
												tex: tex_name(desc, tex),
											}),
										)
									}
								}

								_ => {}
							}
						}

					Err(_) => {}
				}

			Raster(_) => {}
		}
	}

	[count_errors, raster_errors, $rotation_errors].join()
}

## The texture accesses one command binds, by access.
TexAccesses : { reads : List(U32), prev_reads : List(U32), writes : List(U32), mutates : List(U32) }

tex_accesses : List(ResourceBinding) -> TexAccesses
tex_accesses = |bindings| {
	ids = |access|
		bindings.join_map(
			|{ resource, .. }|
				match resource {
					Tex(tex, found) if found == access => [tex]
					_ => []
				},
		)

	{ reads: ids(Read), prev_reads: ids(ReadPrevious), writes: ids(Write), mutates: ids(Mutate) }
}

## The per-command texture hazards and physical image counts. A texture
## needs a second image when a command reads and writes it or anything reads
## its previous version. Previous-read errors are kept apart because the
## Rust validator reports them last.
analyze_textures : GraphDesc -> { tex_phys : List(U32), errors : List(RenderGraphValidate.GraphError), prev_read_errors : List(RenderGraphValidate.GraphError) }
analyze_textures = |desc| {
	tex_count = desc.textures.len()
	var $phys = List.repeat(1, tex_count)
	var $errors = []
	var $prev_readers = []
	var $written_anywhere = []
	for command in all_commands(desc) {
		accesses = tex_accesses(RenderGraphDesc.command_bindings(desc, command))
		for tex in accesses.prev_reads {
			if in_range(tex, tex_count) {
				$phys = $phys.set(tex.to_u64(), 2) ?? $phys
				if !$prev_readers.any(|(reader_tex, _)| reader_tex == tex) {
					$prev_readers = $prev_readers.append((tex, command.name))
				}
			}
		}

		for (tex, i) in accesses.writes.map_with_index(|tex, i| (tex, i)) {
			if !$written_anywhere.contains(tex) {
				$written_anywhere = $written_anywhere.append(tex)
			}
			if accesses.writes.drop_first(i + 1).contains(tex) {
				$errors = $errors.append(DuplicateWrite({ command: command.name, tex: tex_name(desc, tex) }))
			}
			if accesses.reads.contains(tex) and in_range(tex, tex_count) {
				$phys = $phys.set(tex.to_u64(), 2) ?? $phys
			}
			if accesses.prev_reads.contains(tex) {
				$errors = $errors.append(WriteAndPrevRead({ command: command.name, tex: tex_name(desc, tex) }))
			}
		}

		for tex in accesses.mutates {
			if accesses.reads.contains(tex) {
				$errors = $errors.append(MutateAndRead({ command: command.name, tex: tex_name(desc, tex) }))
			}
			if accesses.writes.contains(tex) {
				$errors = $errors.append(MutateAndWrite({ command: command.name, tex: tex_name(desc, tex) }))
			}
		}
	}

	# Only a `Write` advances the version cursor, so a previous version exists
	# only for a texture some command writes; a mutate edits in place.
	prev_read_errors = $prev_readers.join_map(
		|(tex, command)|
			if ($written_anywhere.contains(tex))
				[]
			else
				[PrevReadWithoutWrite({ command, tex: tex_name(desc, tex) })],
	)

	{ tex_phys: $phys, errors: $errors, prev_read_errors }
}

valid_command : GraphDesc, SchemaTable, Command, PipelineKind -> List(RenderGraphValidate.GraphError)
valid_command = |desc, schemas, command, expected| {
	kind_errors =
		match desc.pipelines.get(command.pipeline.to_u64()) {
			Err(_) => valid(command.pipeline, desc.pipelines.len(), Pipeline)
			Ok(pipeline) =>
				if (pipeline.kind == expected)
					[]
				else
					[PipelineKindMismatch({ command: command.name, expected })]
		}

	uniform_errors = valid(command.uniform, desc.uniforms.len(), Uniform)
	push_errors =
		match command.push {
			None => []
			Some(push) => {
				data_errors =
					match push.data {
						Some(value) => value_is_bytes(desc, value)
						None => []
					}

				[
					valid(push.schema, schemas.schemas.len(), Schema),
					data_errors,
					valid_bindings(desc, command.name, push.bindings),
					check_layout_data(command.name, push.schema, push.data, desc, schemas),
					check_shape(command.name, push.schema, push.bindings, schemas),
				].join()
			}
		}

	[kind_errors, uniform_errors, push_errors].join()
}

check_leaf : GraphDesc, SchemaTable, LeafPass -> List(RenderGraphValidate.GraphError)
check_leaf = |desc, schemas, leaf|
	match leaf {
		Compute(dispatch) => {
			group_errors =
				match dispatch.groups {
					Fixed(_) => []
					Value(value) => {
						kind_errors =
							match desc.values.get(value.to_u64()) {
								Err(_) => valid(value, desc.values.len(), Value)
								Ok(decl) if decl.kind != Groups =>
									[
										ValueKindMismatch({
											value: decl.name,
											expected: "groups",
											found: RenderGraphValidate.value_kind_str(decl.kind),
										}),
									]

								Ok(_) => []
							}

						kind_errors.append(UnsupportedInPhase1({ feature: GroupSourceValue, at: dispatch.name }))
					}
				}

			command = {
				name: dispatch.name,
				pipeline: dispatch.pipeline,
				uniform: dispatch.uniform,
				push: dispatch.push,
			}

			group_errors.concat(valid_command(desc, schemas, command, Compute))
		}

		Raster(raster) => {
			target_errors =
				match raster.raster_targets {
					Main => []
					Offscreen({ color, depth }) => {
						depth_ids =
							match depth {
								Some(id) => [id]
								None => []
							}

						color
							.concat(depth_ids)
							.join_map(|tex| valid(tex, desc.textures.len(), Texture))
							.prepend(UnsupportedInPhase1({ feature: OffscreenTargets, at: raster.name }))
					}
				}

			target_errors.concat(raster.draws.join_map(|draw| check_draw(desc, schemas, draw)))
		}
	}

check_draw : GraphDesc, SchemaTable, RenderGraphDesc.DrawDesc -> List(RenderGraphValidate.GraphError)
check_draw = |desc, schemas, draw| {
	command = { name: draw.name, pipeline: draw.pipeline, uniform: draw.uniform, push: draw.push }
	written = RenderGraphDesc.command_bindings(desc, command).fold(
		[],
		|found, { resource, .. }|
			match resource {
				Tex(tex, Write) | Tex(tex, Mutate) if !found.contains(tex) => found.append(tex)
				_ => found
			},
	)
	write_errors = written.map(|tex| DrawWritesTexture({ draw: draw.name, tex: tex_name(desc, tex) }))
	expected =
		match draw.call {
			IndexedIndirect(_) => GraphicsIndirect
			_ => Graphics
		}

	args_errors =
		match draw.call {
			IndexedIndirect({ args, .. }) =>
				match desc.buffers.get(args.buffer.to_u64()) {
					Err(_) => valid(args.buffer, desc.buffers.len(), Buffer)
					Ok(buffer) if buffer.kind != Immutable => [IndirectArgsNotImmutable({ draw: draw.name })]
					Ok(_) => []
				}

			_ => []
		}

	[
		write_errors,
		valid_command(desc, schemas, command, expected),
		validate_draw_indices(desc, draw),
		args_errors,
	].join()
}

## An indexed draw must name an index buffer, and an index sub-range must fit
## in it. The index count is the buffer's `capacity`.
validate_draw_indices : GraphDesc, RenderGraphDesc.DrawDesc -> List(RenderGraphValidate.GraphError)
validate_draw_indices = |desc, draw| {
	indices =
		match RenderGraphDesc.index_buffer(draw.call) {
			Some(id) => id
			None => return []
		}

	buffer =
		match desc.buffers.get(indices.to_u64()) {
			Ok(found) => found
			Err(_) => return [IdOutOfRange({ table: Buffer, id: indices })]
		}

	if buffer.kind != Index {
		return [DrawIndicesNotIndexBuffer({ draw: draw.name })]
	}

	match draw.call {
		IndexRange({ first_index, index_count, .. }) => {
			capacity =
				match buffer.capacity {
					Some(count) => count
					None => 0
				}

			if (range_in_bounds(first_index, index_count, capacity))
				[]
			else
				[IndexRangeOutOfBounds({ draw: draw.name, first_index, index_count, capacity })]
		}

		_ => []
	}
}

check_picking : GraphDesc, SchemaTable -> List(RenderGraphValidate.GraphError)
check_picking = |desc, schemas| {
	picking =
		match desc.picking {
			Some(found) => found
			None => return []
		}

	cursor_errors =
		match desc.values.get(picking.cursor.to_u64()) {
			Err(_) => valid(picking.cursor, desc.values.len(), Value)
			Ok(value) => {
				cursor_is_8_bytes =
					match value.kind {
						Bytes({ schema }) =>
							match schemas.schemas.get(schema.to_u64()) {
								Ok(found) => found.size == 8
								Err(_) => Bool.False
							}

						_ => Bool.False
					}

				if (cursor_is_8_bytes) [] else [PickingCursorShape]
			}
		}

	has_draw = RenderGraphDesc.leaves(desc).any(
		|leaf|
			match leaf {
				Raster(raster) => !raster.draws.is_empty()
				Compute(_) => Bool.False
			},
	)

	if (has_draw) cursor_errors else cursor_errors.append(PickingWithoutDraw)
}

## A uniform block is written once on the CPU before any command executes,
## so every consumer must resolve it to the same bytes. A `Write` between a
## shared block's first and last consumer moves the version its texture
## references resolve to. A repeat body's own rotation is reported as
## `RepeatUniformRotatesTexture` instead.
check_shared_uniforms : GraphDesc -> List(RenderGraphValidate.GraphError)
check_shared_uniforms = |desc| {
	commands = all_commands(desc)
	repeat_reports = check_passes(desc).join_map(
		|error|
			match error {
				RepeatUniformRotatesTexture({ uniform, tex, .. }) => [(uniform, tex)]
				_ => []
			},
	)

	desc.uniforms.map_with_index(|uniform, id| (uniform, id)).join_map(
		|(uniform, id)| {
			consumers = commands.map_with_index(|command, position| (command, position)).join_map(
				|(command, position)| if (command.uniform.to_u64() == id) [position] else [],
			)
			(first, last) =
				match (consumers.first(), consumers.last()) {
					(Ok(first_position), Ok(last_position)) => (first_position, last_position)
					_ => return []
				}

			writers = commands.sublist({ start: first, len: last - first })
			referenced = |tex|
				uniform.source.bindings.any(
					|{ resource, .. }|
						match resource {
							Tex(bound, _) => bound == tex
							_ => Bool.False
						},
				)

			var $reported = []
			var $errors = []
			for writer in writers {
				for { resource, .. } in RenderGraphDesc.command_bindings(desc, writer) {
					match resource {
						Tex(tex, Write) => {
							name = tex_name(desc, tex)
							already_reported = $reported.contains(tex) or repeat_reports.contains((uniform.name, name))
							if referenced(tex) and !already_reported {
								$reported = $reported.append(tex)
								$errors = $errors.append(
									SharedUniformRotatesTexture({ uniform: uniform.name, writer: writer.name, tex: name }),
								)
							}
						}

						_ => {}
					}
				}
			}

			$errors
		},
	)
}

## The checks the Rust plain-data path makes when it binds a description:
## per-frame group counts and zero-sized array elements are unsupported,
## every block has a field layout, each command writes blocks of its
## pipeline's sizes, and every buffer binding's offset lies inside the
## buffer, since a buffer device address bypasses `robustBufferAccess`.
check_plain_bindings : GraphDesc, SchemaTable -> List(RenderGraphValidate.GraphError)
check_plain_bindings = |desc, schemas| {
	schema = |id| schemas.schemas.get(id.to_u64())
	value_errors = desc.values.join_map(
		|value|
			match value.kind {
				Groups => [GroupsValueUnsupported({ value: value.name })]
				Array({ elem, .. }) =>
					match schema(elem) {
						Ok(found) if found.size == 0 => [ZeroSizedArrayElement({ value: value.name })]
						_ => []
					}

				_ => []
			},
	)
	block_ids = dedup_ids(desc.uniforms.map(|uniform| uniform.schema).concat(push_schema_ids(desc)))
	layout_errors = block_ids.join_map(
		|id|
			match schema(id) {
				Ok({ layout: None, name, .. }) => [SchemaWithoutLayout({ schema: name })]
				_ => []
			},
	)
	uniform_offset_errors = desc.uniforms.join_map(|uniform| check_buffer_offsets(desc, uniform.name, uniform.source.bindings))
	command_errors = all_commands(desc).join_map(
		|command| {
			push_offset_errors =
				match command.push {
					Some(push) => check_buffer_offsets(desc, command.name, push.bindings)
					None => []
				}

			check_command_sizes(desc, schemas, command).concat(push_offset_errors)
		},
	)

	[value_errors, layout_errors, uniform_offset_errors, command_errors].join()
}

## A command writes a uniform and push block of the sizes its pipeline
## expects.
check_command_sizes : GraphDesc, SchemaTable, Command -> List(RenderGraphValidate.GraphError)
check_command_sizes = |desc, schemas, command| {
	(pipeline, uniform) =
		match (desc.pipelines.get(command.pipeline.to_u64()), desc.uniforms.get(command.uniform.to_u64())) {
			(Ok(found_pipeline), Ok(found_uniform)) => (found_pipeline, found_uniform)
			_ => return []
		}

	size = |id|
		match schemas.schemas.get(id.to_u64()) {
			Ok(found) => found.size
			Err(_) => 0
		}

	maybe_size = |maybe_id|
		match maybe_id {
			Some(id) => size(id)
			None => 0
		}

	command_push =
		match command.push {
			Some(push) => size(push.schema)
			None => 0
		}

	params_errors =
		if (size(uniform.schema) != size(pipeline.params))
			[
				CommandBlockSize({
					command: command.name,
					block: "uniform",
					size: size(uniform.schema),
					pipeline: pipeline.name,
					expected: size(pipeline.params),
				}),
			]
		else
			[]

	push_errors =
		if (command_push != maybe_size(pipeline.push))
			[
				CommandBlockSize({
					command: command.name,
					block: "push",
					size: command_push,
					pipeline: pipeline.name,
					expected: maybe_size(pipeline.push),
				}),
			]
		else
			[]

	params_errors.concat(push_errors)
}

buffer_byte_len : RenderGraphDesc.BufferDecl -> U64
buffer_byte_len = |buffer| {
	count =
		match buffer.capacity {
			Some(capacity) => capacity.to_u64()
			None => 0
		}

	element_size =
		match buffer.elem_size {
			Some(size) => size.to_u64()
			None => 0
		}

	count * element_size
}

check_buffer_offsets : GraphDesc, Str, List(ResourceBinding) -> List(RenderGraphValidate.GraphError)
check_buffer_offsets = |desc, block, bindings|
	bindings.join_map(
		|{ key, resource }|
			match resource {
				Buf(buffer, _) =>
					match desc.buffers.get(buffer.buffer.to_u64()) {
						Ok(decl) => {
							byte_len = buffer_byte_len(decl)

							if (buffer.offset.to_u64() < byte_len)
								[]
							else
								[
									BufferOffsetOutOfBounds({
										block,
										field: key,
										offset: buffer.offset,
										buffer: decl.name,
										byte_len,
									}),
								]
						}

						Err(_) => []
					}

				_ => []
			},
	)

## The contents of one buffer declaration.
check_planned_buffer : ResourcePlanner.PlannedBuffer -> List(RenderGraphValidate.GraphError)
check_planned_buffer = |buffer| {
	name = buffer.name
	size_errors =
		if (buffer.element_size == 0)
			[ZeroElementSize({ buffer: name })]
		else if (buffer.len.to_u64() * buffer.element_size.to_u64() > U32.highest.to_u64())
			[BufferTooLarge({ buffer: name, len: buffer.len, element_size: buffer.element_size })]
		else
			[]

	content_errors =
		match buffer.contents {
			Indices({ indices, vertex_count }) => check_indices(name, indices, vertex_count)
			Bytes(bytes) => {
				count = if (buffer.element_size == 0) 0 else bytes.len() // buffer.element_size.to_u64()

				match buffer.role {
					CpuUpdated | GpuUpdated =>
						if (buffer.len == 0)
							[EmptyBuffer({ buffer: name })]
						else if (count > buffer.len.to_u64())
							[InitialItemsExceedCapacity({ buffer: name, count, capacity: buffer.len })]
						else
							[]

					_ => if (count == 0) [EmptyBuffer({ buffer: name })] else []
				}
			}
		}

	size_errors.concat(content_errors)
}

## Every index is below `vertex_count`; the first out-of-bounds index is
## reported with its position.
check_indices : Str, List(U32), U32 -> List(RenderGraphValidate.GraphError)
check_indices = |buffer, indices, vertex_count| {
	if indices.is_empty() {
		return [EmptyIndexBuffer({ buffer: buffer })]
	}

	if vertex_count == 0 {
		return [ZeroVertexCount({ buffer: buffer })]
	}

	match indices.find_first_index(|index| index >= vertex_count) {
		Ok(position) => {
			index = indices.get(position) ?? 0

			[IndexOutOfBounds({ buffer, position, index, vertex_count })]
		}

		Err(_) => []
	}
}

## An indexed-indirect draw reads whole records from a buffer declared with
## `ResourcePlanner.indirect_buffer`, and at least one of them.
check_indirect_args : ResourcePlanner, RenderGraphDesc.DrawDesc -> List(RenderGraphValidate.GraphError)
check_indirect_args = |planner, draw| {
	(args, draw_count) =
		match draw.call {
			IndexedIndirect(call) => (call.args, call.draw_count)
			_ => return []
		}

	buffer =
		match planner.buffers().get(args.buffer.to_u64()) {
			Ok(found) => found
			Err(_) => return []
		}

	if buffer.role != IndirectArgs {
		return [IndirectArgsNotIndirectBuffer({ draw: draw.name, buffer: buffer.name })]
	}

	stride = RenderGraphValidate.indirect_command_bytes
	first_command = args.offset // stride
	whole_records = args.slot == Current and args.offset % stride == 0
	in_bounds = draw_count > 0 and range_in_bounds(first_command, draw_count, buffer.len)

	if (whole_records and in_bounds)
		[]
	else
		[IndirectCommandsOutOfBounds({ draw: draw.name, first_command, draw_count, len: buffer.len })]
}

## One compute pass binding `bindings` through its uniform, over two 8x8
## textures, with a 16-byte block of two 8-byte resource fields.
one_dispatch : List(ResourceBinding), List(RenderGraphDesc.ResourceFieldKind) -> (GraphDesc, SchemaTable)
one_dispatch = |bindings, resource_fields| {
	texture = |name| { name, format: R32Float, size: Fixed(8, 8), usage: Storage }
	fields = resource_fields.map_with_index(
		|kind, index| {
			field_kind =
				match kind {
					SampledTex => SampledTex
					StorageTex => StorageTex
					BufAddr => BufAddr
				}

			{ key: index.to_u16_wrap(), offset: index.to_u32_wrap() * 8, len: 8, kind: field_kind }
		},
	)
	desc = {
		..RenderGraphDesc.empty,
		textures: [texture("tex0"), texture("tex1")],
		uniforms: [{ name: "uniform0", schema: 0, source: { data: None, bindings } }],
		pipelines: [{ name: "pipeline0", kind: Compute, params: 0, push: None }],
		passes: [Leaf(Compute({ name: "d0", pipeline: 0, uniform: 0, groups: Fixed({ x: 1, y: 1, z: 1 }), push: None }))],
	}
	schema = { name: "block", size: 16, resource_fields, layout: Some({ fields: fields }) }

	(desc, { schemas: [schema] })
}

expect {
	(desc, schemas) = one_dispatch(
		[{ key: 0, resource: Tex(0, Read) }, { key: 1, resource: Tex(0, Write) }],
		[SampledTex, StorageTex],
	)

	RenderGraphValidate.validate(desc, schemas) == Ok({ tex_phys: [2, 1] })
}

expect {
	(desc, schemas) = one_dispatch(
		[{ key: 0, resource: Tex(1, Read) }, { key: 1, resource: Tex(0, Write) }],
		[SampledTex, StorageTex],
	)

	RenderGraphValidate.validate(desc, schemas) == Ok({ tex_phys: [1, 1] })
}

expect {
	(desc, schemas) = one_dispatch(
		[{ key: 0, resource: Tex(1, ReadPrevious) }, { key: 1, resource: Tex(1, Mutate) }],
		[SampledTex, StorageTex],
	)

	RenderGraphValidate.validate(desc, schemas) == Err([PrevReadWithoutWrite({ command: "d0", tex: "tex1" })])
}

expect {
	(desc, schemas) = one_dispatch(
		[{ key: 0, resource: Tex(2, Read) }, { key: 1, resource: Tex(0, Read) }],
		[SampledTex, StorageTex],
	)

	RenderGraphValidate.validate(desc, schemas)
		== Err([IdOutOfRange({ table: Texture, id: 2 }), BindingKindMismatch({ uniform: "uniform0", field: 1 })])
}

expect {
	(desc, _) = one_dispatch([{ key: 0, resource: Tex(0, Read) }], [SampledTex])
	overlapping = {
		name: "block",
		size: 10,
		resource_fields: [SampledTex],
		layout: Some({ fields: [{ key: 0, offset: 0, len: 8, kind: SampledTex }, { key: 1, offset: 4, len: 8, kind: Data({ src_offset: 0 }) }] }),
	}

	RenderGraphValidate.validate(desc, { schemas: [overlapping] })
		== Err([
			LayoutFieldOutOfBounds({ schema: "block", field: 1, offset: 4, len: 8, size: 10 }),
			LayoutFieldsOverlap({ schema: "block", first: 0, second: 1 }),
			LayoutDataWithoutValue({ block: "uniform0" }),
		])
}

expect RenderGraphValidate.validate(RenderGraphDesc.empty, { schemas: [] }) == Ok({ tex_phys: [] })

expect RenderGraphValidate.validation_message([]) == "render graph validation failed:"

expect
	RenderGraphValidate.validation_message([IdOutOfRange({ table: Uniform, id: 1 }), PassAfterMainRaster({ pass: "dispatch0" })])
		== "render graph validation failed:\n  - uniform id 1 is undeclared\n  - pass dispatch0 must precede the main raster pass"

## A render graph pipeline reads exactly one uniform buffer, declared by its
## planner, whose block is the one its shader reflects: the graph stages one
## uniform per command, so another block would never be written and its
## bindings would escape hazard analysis.
check_pipeline_uniform : ResourcePlanner, ResourcePlanner.PlannedPipeline -> List(RenderGraphValidate.GraphError)
check_pipeline_uniform = |planner, pipeline| {
	slot =
		match pipeline.uniform_slots {
			[only] => only
			slots => return [PipelineUniformCount({ pipeline: pipeline.name, count: slots.len() })]
		}

	uniform =
		match planner.uniforms().get(slot.to_u64()) {
			Ok(found) => found
			Err(_) => return [PipelineUniformUndeclared({ pipeline: pipeline.name, slot })]
		}

	found = "${uniform.layout.name}, ${uniform.layout.gpu_size.to_str()} bytes"
	expected =
		match pipeline.parameter_blocks {
			[block] if block.type_name == uniform.layout.name and block.size == uniform.layout.gpu_size.to_u64() =>
				return []

			[block] => "${block.type_name}, ${block.size.to_str()} bytes"
			blocks => "${blocks.len().to_str()} reflected ParameterBlock globals"
		}

	[PipelineUniformMismatch({ pipeline: pipeline.name, uniform: uniform.name, found, expected })]
}
