import ShaderReflection exposing [
	BufferBinding,
	ComputeShader,
	ElementPacker,
	GraphicsIndirectShader,
	GraphicsShader,
	ImmutableBufferBinding,
	Provenance,
	ReadBufferBinding,
	ResourceFieldKind,
	SampledTexBinding,
	SchemaField,
	StorageTexBinding,
	UniformSlot,
	UniformSpec,
]
import RasterState
import RenderGraphDesc

## Explicit declarations of every resource a render graph uses, mirroring the
## Rust renderer's buffer, uniform, pipeline, and texture constructors.
##
## Setup in Roc is pure data, so each constructor appends a declaration and
## returns the grown planner with a typed slot or key naming it. A planner
## only grows: a slot minted from an earlier planner in the same chain stays
## valid in every later one.
##
## Every declaration records a `Provenance`: its name and a digest of
## everything it declares. Slots, keys, and the bindings they mint carry it,
## and `RenderGraph.new` rejects one whose planner entry differs, so two
## unrelated planner chains cannot alias each other's ids. Identical
## declarations are interchangeable, so they share a provenance.
ResourcePlanner :: {
	buffers : List(PlannedBuffer),
	uniforms : List(PlannedUniform),
	pipelines : List(PlannedPipeline),
	textures : List(PlannedTexture),
	import_decls : List(PlannedImport),
}.{

	## What the host creates for one buffer declaration. `len` is the
	## element capacity and `element_size` the bytes per element.
	PlannedBuffer : {
		name : Str,
		role : BufferRole,
		element_size : U32,
		len : U32,
		contents : BufferContents,
		provenance : Provenance,
	}

	## A buffer declaration before its provenance is recorded.
	BufferSpec : {
		name : Str,
		role : BufferRole,
		element_size : U32,
		len : U32,
		contents : BufferContents,
	}

	## The backend buffer family, as the host binds it. Indexed-indirect
	## argument buffers are immutable buffers of
	## `DrawIndexedIndirectCommand` records.
	BufferRole : [CpuUpdated, GpuUpdated, Immutable, Singleton, Index, IndirectArgs]

	## A buffer's setup-time contents: packed leading elements, or an index
	## buffer's indices and the vertex count they must stay below.
	BufferContents : [
		Bytes(List(U8)),
		Indices({ indices : List(U32), vertex_count : U32 }),
	]

	## A uniform buffer declaration: its name and its block's layout.
	PlannedUniform : { name : Str, layout : BlockLayout, provenance : Provenance }

	## A block schema without its packers, so declarations compare by value.
	BlockLayout : {
		name : Str,
		gpu_size : U32,
		data_size : U32,
		resource_fields : List(ResourceFieldKind),
		fields : List(SchemaField),
	}

	## What the host creates for one pipeline declaration. `uniform_slots`
	## lists the planner uniform buffers its parameter blocks read, and
	## `parameter_blocks` the blocks its shader reflects, both in
	## descriptor-set-layout order. A render graph pipeline has exactly one.
	PlannedPipeline : {
		name : Str,
		kind : RenderGraphDesc.PipelineKind,
		stages : PipelineStages,
		reflection_json : Str,
		uniform_slots : List(U32),
		parameter_blocks : List(ParameterBlock),
	}

	## A reflected `ParameterBlock<T>` global: its element type's name and
	## its constant buffer's byte size.
	ParameterBlock : { type_name : Str, size : U64 }

	PipelineStages : [
		Graphics(
			{ vertex_spv : List(U8), fragment_spv : List(U8), raster : RasterState },
		),
		Compute(
			{ compute_spv : List(U8), workgroup_size : { x : U32, y : U32, z : U32 } },
		),
	]

	## A logical graph texture declaration.
	PlannedTexture : { decl : RenderGraphDesc.TexDecl, provenance : Provenance }

	## An external sampled texture the host supplies, named for the host.
	PlannedImport : { name : Str, provenance : Provenance }

	empty : ResourcePlanner
	empty = {
		buffers: [],
		uniforms: [],
		pipelines: [],
		textures: [],
		import_decls: [],
	}

	buffers : ResourcePlanner -> List(PlannedBuffer)
	buffers = |planner| planner.buffers

	uniforms : ResourcePlanner -> List(PlannedUniform)
	uniforms = |planner| planner.uniforms

	pipelines : ResourcePlanner -> List(PlannedPipeline)
	pipelines = |planner| planner.pipelines

	textures : ResourcePlanner -> List(PlannedTexture)
	textures = |planner| planner.textures

	import_decls : ResourcePlanner -> List(PlannedImport)
	import_decls = |planner| planner.import_decls

	## One declared buffer: its id in the planner's buffer table and the
	## declaration a slot was minted for.
	BufferRef : { id : U32, decl : PlannedBuffer }

	## A buffer the GPU only reads, uploaded once at setup.
	SingletonSlot(elem) :: { buffer : BufferRef }.{
		buffer_ref : SingletonSlot(elem) -> BufferRef
		buffer_ref = |slot| slot.buffer

		## The whole buffer.
		addr : SingletonSlot(elem) -> ImmutableBufferBinding(elem)
		addr = |slot| ImmutableBufferBinding.new(element_target(slot.buffer, 0))

		## The buffer from element `element` on. An element past the end fails
		## graph validation.
		addr_at : SingletonSlot(elem), U32 -> ImmutableBufferBinding(elem)
		addr_at = |slot, element|
			ImmutableBufferBinding.new(element_target(slot.buffer, element))
	}

	## A buffer the GPU only reads, with one copy per frame in flight.
	ImmutableSlot(elem) :: { buffer : BufferRef }.{
		buffer_ref : ImmutableSlot(elem) -> BufferRef
		buffer_ref = |slot| slot.buffer

		## The whole buffer.
		addr : ImmutableSlot(elem) -> ImmutableBufferBinding(elem)
		addr = |slot| ImmutableBufferBinding.new(element_target(slot.buffer, 0))

		## The buffer from element `element` on. An element past the end fails
		## graph validation.
		addr_at : ImmutableSlot(elem), U32 -> ImmutableBufferBinding(elem)
		addr_at = |slot, element|
			ImmutableBufferBinding.new(element_target(slot.buffer, element))
	}

	## A storage buffer the CPU rewrites with `RenderGraph.upload`.
	CpuUpdatedSlot(elem) :: { buffer : BufferRef, packer : ElementPacker(elem) }.{
		buffer_ref : CpuUpdatedSlot(elem) -> BufferRef
		buffer_ref = |slot| slot.buffer

		packer : CpuUpdatedSlot(elem) -> ElementPacker(elem)
		packer = |slot| slot.packer

		## The current frame's copy, read-write.
		addr : CpuUpdatedSlot(elem) -> BufferBinding(elem)
		addr = |slot| BufferBinding.new(element_target(slot.buffer, 0))

		## The current frame's copy, read-only.
		read_addr : CpuUpdatedSlot(elem) -> ReadBufferBinding(elem)
		read_addr = |slot| ReadBufferBinding.new(element_target(slot.buffer, 0))
	}

	## A storage buffer only the GPU writes, keeping the previous frame's copy.
	GpuUpdatedSlot(elem) :: { buffer : BufferRef }.{
		buffer_ref : GpuUpdatedSlot(elem) -> BufferRef
		buffer_ref = |slot| slot.buffer

		## This frame's copy, read-write.
		current : GpuUpdatedSlot(elem) -> BufferBinding(elem)
		current = |slot| BufferBinding.new(element_target(slot.buffer, 0))

		## The previous frame's copy, read-only.
		previous : GpuUpdatedSlot(elem) -> ReadBufferBinding(elem)
		previous = |slot|
			ReadBufferBinding.new({ ..element_target(slot.buffer, 0), slot: Previous })
	}

	## A hardware index buffer of `U32` indices, bound by indexed draws.
	IndexBufferSlot :: { buffer : BufferRef }.{
		buffer_ref : IndexBufferSlot -> BufferRef
		buffer_ref = |slot| slot.buffer

		index_count : IndexBufferSlot -> U32
		index_count = |slot| slot.buffer.decl.len
	}

	## One `vkCmdDrawIndexedIndirect` record: 5 four-byte fields, 20 bytes.
	DrawIndexedIndirectCommand := {
		index_count : U32,
		instance_count : U32,
		first_index : U32,
		vertex_offset : I32,
		first_instance : U32,
	}.{
		is_eq : _

		gpu_size : U32
		gpu_size = 20

		to_bytes : DrawIndexedIndirectCommand -> List(U8)
		to_bytes = |command|
			[
				ShaderReflection.u32_bytes(command.index_count),
				ShaderReflection.u32_bytes(command.instance_count),
				ShaderReflection.u32_bytes(command.first_index),
				ShaderReflection.i32_bytes(command.vertex_offset),
				ShaderReflection.u32_bytes(command.first_instance),
			].join()

		packer : ElementPacker(DrawIndexedIndirectCommand)
		packer = { stride: gpu_size, to_bytes }
	}

	## A logical graph texture. Physical images and versioning are internal
	## to the graph.
	GraphTex :: { id : U32, decl : PlannedTexture }.{
		tex_id : GraphTex -> U32
		tex_id = |tex| tex.id

		decl : GraphTex -> RenderGraphDesc.TexDecl
		decl = |tex| tex.decl.decl

		## Sample the most recent version written before this node.
		read : GraphTex -> SampledTexBinding
		read = |tex| SampledTexBinding.new(Read(declared_tex(tex)))

		## Sample the version before the most recent write.
		read_previous : GraphTex -> SampledTexBinding
		read_previous = |tex| SampledTexBinding.new(ReadPrevious(declared_tex(tex)))

		## Produce the next version of the texture.
		write : GraphTex -> StorageTexBinding
		write = |tex| StorageTexBinding.new(Write(declared_tex(tex)))

		## Read-modify-write the current version in place.
		mutate : GraphTex -> StorageTexBinding
		mutate = |tex| StorageTexBinding.new(Mutate(declared_tex(tex)))
	}

	## An external sampled texture the host supplies by name.
	ImportedTex :: { id : U32, decl : PlannedImport }.{
		import_id : ImportedTex -> U32
		import_id = |tex| tex.id

		decl : ImportedTex -> PlannedImport
		decl = |tex| tex.decl

		read : ImportedTex -> SampledTexBinding
		read = |tex|
			SampledTexBinding.new(Imported({ id: tex.id, provenance: tex.decl.provenance }))
	}

	## One declared pipeline: its id in the planner's pipeline table and the
	## declaration a key was minted for.
	PipelineRef : { id : U32, decl : PlannedPipeline }

	## A pipeline for direct vertex-count and indexed draws. `push` is the
	## shader's `NoPush` or `PushSpec`, which selects the draw constructors
	## that accept the key.
	GraphicsPipelineKey(push) :: { pipeline : PipelineRef, push : push }.{
		pipeline_ref : GraphicsPipelineKey(push) -> PipelineRef
		pipeline_ref = |key| key.pipeline

		push_spec : GraphicsPipelineKey(push) -> push
		push_spec = |key| key.push
	}

	## A pipeline for indexed-indirect draws; its shader reads `SV_DrawIndex`.
	GraphicsIndirectPipelineKey(push) :: { pipeline : PipelineRef, push : push }.{
		pipeline_ref : GraphicsIndirectPipelineKey(push) -> PipelineRef
		pipeline_ref = |key| key.pipeline

		push_spec : GraphicsIndirectPipelineKey(push) -> push
		push_spec = |key| key.push
	}

	## A compute pipeline for dispatches.
	ComputePipelineKey(push) :: { pipeline : PipelineRef, push : push }.{
		pipeline_ref : ComputePipelineKey(push) -> PipelineRef
		pipeline_ref = |key| key.pipeline

		push_spec : ComputePipelineKey(push) -> push
		push_spec = |key| key.push
	}

	## A buffer the GPU reads, uploaded once from `initial_items`.
	singleton_buffer : ResourcePlanner, Str, ElementPacker(elem), List(elem) -> (ResourcePlanner, SingletonSlot(elem))
	singleton_buffer = |planner, name, elem_packer, initial_items| {
		decl = packed_buffer(name, Singleton, elem_packer, initial_items)
		(grown, buffer_ref) = add_buffer(planner, decl)

		(grown, { buffer: buffer_ref })
	}

	## A buffer the GPU reads, with one copy per frame in flight, each
	## starting as `initial_items`.
	immutable_buffer : ResourcePlanner, Str, ElementPacker(elem), List(elem) -> (ResourcePlanner, ImmutableSlot(elem))
	immutable_buffer = |planner, name, elem_packer, initial_items| {
		decl = packed_buffer(name, Immutable, elem_packer, initial_items)
		(grown, buffer_ref) = add_buffer(planner, decl)

		(grown, { buffer: buffer_ref })
	}

	## An argument buffer for `RenderGraph.draw_indexed_indirect`. Its
	## address also reads the records as an immutable buffer.
	indirect_buffer : ResourcePlanner, Str, List(DrawIndexedIndirectCommand) -> (ResourcePlanner, ImmutableSlot(DrawIndexedIndirectCommand))
	indirect_buffer = |planner, name, initial_commands| {
		commands_packer = DrawIndexedIndirectCommand.packer
		decl = packed_buffer(name, IndirectArgs, commands_packer, initial_commands)
		(grown, buffer_ref) = add_buffer(planner, decl)

		(grown, { buffer: buffer_ref })
	}

	## A storage buffer of `capacity` elements the CPU rewrites each frame
	## with `RenderGraph.upload`; every flight copy starts with
	## `initial_items`.
	cpu_updated_buffer : ResourcePlanner, Str, ElementPacker(elem), U32, List(elem) -> (ResourcePlanner, CpuUpdatedSlot(elem))
	cpu_updated_buffer = |planner, name, elem_packer, capacity, initial_items| {
		initial = packed_buffer(name, CpuUpdated, elem_packer, initial_items)
		decl = { ..initial, len: capacity }
		(grown, buffer_ref) = add_buffer(planner, decl)

		(grown, { buffer: buffer_ref, packer: elem_packer })
	}

	## A storage buffer of `capacity` elements only the GPU writes; every
	## flight copy starts with `initial_items`.
	gpu_updated_buffer : ResourcePlanner, Str, ElementPacker(elem), U32, List(elem) -> (ResourcePlanner, GpuUpdatedSlot(elem))
	gpu_updated_buffer = |planner, name, elem_packer, capacity, initial_items| {
		initial = packed_buffer(name, GpuUpdated, elem_packer, initial_items)
		decl = { ..initial, len: capacity }
		(grown, buffer_ref) = add_buffer(planner, decl)

		(grown, { buffer: buffer_ref })
	}

	## A hardware index buffer. Graph validation rejects empty indices, a
	## zero `vertex_count`, and any index at or past `vertex_count`.
	index_buffer : ResourcePlanner, Str, List(U32), U32 -> (ResourcePlanner, IndexBufferSlot)
	index_buffer = |planner, name, indices, vertex_count| {
		decl = {
			name,
			role: Index,
			element_size: 4,
			len: checked_len(name, indices),
			contents: Indices({ indices, vertex_count }),
		}
		(grown, buffer_ref) = add_buffer(planner, decl)

		(grown, { buffer: buffer_ref })
	}

	## A uniform buffer for one of a shader's parameter blocks, such as a
	## generated module's `params`.
	uniform_buffer : ResourcePlanner, Str, UniformSpec(data, bindings) -> (ResourcePlanner, UniformSlot(data, bindings))
	uniform_buffer = |planner, name, spec| {
		slot_index = planner.uniforms.len().to_u32_wrap()
		layout = block_layout(spec.schema)
		provenance = provenance_of(name, Str.inspect(("uniform", layout)), [])
		uniform = { name, layout, provenance }
		grown = { ..planner, uniforms: planner.uniforms.append(uniform) }

		(grown, UniformSlot.new(slot_index, spec.schema, provenance))
	}

	## A fixed-size logical storage texture.
	texture : ResourcePlanner, Str, U32, U32, RenderGraphDesc.GraphFormat -> (ResourcePlanner, GraphTex)
	texture = |planner, name, width, height, format| {
		tex_decl = { name, format, size: Fixed(width, height), usage: Storage }
		provenance = provenance_of(name, Str.inspect(("texture", tex_decl)), [])
		decl = { decl: tex_decl, provenance }
		tex = { id: planner.textures.len().to_u32_wrap(), decl }

		({ ..planner, textures: planner.textures.append(decl) }, tex)
	}

	## An external sampled texture the host supplies by `name`.
	imported_texture : ResourcePlanner, Str -> (ResourcePlanner, ImportedTex)
	imported_texture = |planner, import_name| {
		provenance = provenance_of(import_name, "import", [])
		decl = { name: import_name, provenance }
		tex = { id: planner.import_decls.len().to_u32_wrap(), decl }

		({ ..planner, import_decls: planner.import_decls.append(decl) }, tex)
	}

	## A graphics pipeline's name, shader, uniform slot per parameter block,
	## and raster state.
	RasterPipelineConfig(shader, uniforms) : {
		name : Str,
		shader : shader,
		uniforms : uniforms,
		raster : RasterState,
	}

	## A pipeline for direct vertex-count and indexed draws. `uniforms`
	## names one uniform slot per parameter block of the shader; a render
	## graph shader declares exactly one.
	graphics_pipeline : ResourcePlanner, RasterPipelineConfig(GraphicsShader(uniforms, push), uniforms) -> (ResourcePlanner, GraphicsPipelineKey(push))
	graphics_pipeline = |planner, config| {
		shader = config.shader
		decl = {
			name: config.name,
			kind: Graphics,
			stages: Graphics({
				vertex_spv: shader.vertex_spv,
				fragment_spv: shader.fragment_spv,
				raster: config.raster,
			}),
			reflection_json: shader.reflection_json,
			uniform_slots: (shader.uniform_slots)(config.uniforms),
			parameter_blocks: parameter_blocks(
				shader.reflection.global_parameters,
				shader.reflection.pipeline_layout,
			),
		}
		(grown, pipeline_ref) = add_pipeline(planner, decl)

		(grown, { pipeline: pipeline_ref, push: shader.push })
	}

	## A pipeline for indexed-indirect draws.
	graphics_indirect_pipeline : ResourcePlanner, RasterPipelineConfig(GraphicsIndirectShader(uniforms, push), uniforms) -> (ResourcePlanner, GraphicsIndirectPipelineKey(push))
	graphics_indirect_pipeline = |planner, config| {
		shader = config.shader
		decl = {
			name: config.name,
			kind: GraphicsIndirect,
			stages: Graphics({
				vertex_spv: shader.vertex_spv,
				fragment_spv: shader.fragment_spv,
				raster: config.raster,
			}),
			reflection_json: shader.reflection_json,
			uniform_slots: (shader.uniform_slots)(config.uniforms),
			parameter_blocks: parameter_blocks(
				shader.reflection.global_parameters,
				shader.reflection.pipeline_layout,
			),
		}
		(grown, pipeline_ref) = add_pipeline(planner, decl)

		(grown, { pipeline: pipeline_ref, push: shader.push })
	}

	## A compute pipeline.
	compute_pipeline : ResourcePlanner, { name : Str, shader : ComputeShader(uniforms, push), uniforms : uniforms } -> (ResourcePlanner, ComputePipelineKey(push))
	compute_pipeline = |planner, config| {
		shader = config.shader
		decl = {
			name: config.name,
			kind: Compute,
			stages: Compute(
				{ compute_spv: shader.compute_spv, workgroup_size: shader.workgroup_size },
			),
			reflection_json: shader.reflection_json,
			uniform_slots: (shader.uniform_slots)(config.uniforms),
			parameter_blocks: parameter_blocks(
				shader.reflection.global_parameters,
				shader.reflection.pipeline_layout,
			),
		}
		(grown, pipeline_ref) = add_pipeline(planner, decl)

		(grown, { pipeline: pipeline_ref, push: shader.push })
	}

	## A block schema's comparable layout.
	block_layout : ShaderReflection.BlockSchema(data, bindings) -> BlockLayout
	block_layout = |schema| {
		name: schema.name,
		gpu_size: schema.gpu_size,
		data_size: schema.data_size,
		resource_fields: schema.resource_fields,
		fields: schema.fields,
	}

	add_buffer : ResourcePlanner, BufferSpec -> (ResourcePlanner, BufferRef)
	add_buffer = |planner, spec| {
		contents =
			match spec.contents {
				Bytes(bytes) => bytes
				Indices({ indices, vertex_count }) =>
					indices.append(vertex_count).join_map(ShaderReflection.u32_bytes)
			}

		summary = Str.inspect(("buffer", spec.role, spec.element_size, spec.len))
		decl = {
			name: spec.name,
			role: spec.role,
			element_size: spec.element_size,
			len: spec.len,
			contents: spec.contents,
			provenance: provenance_of(spec.name, summary, contents),
		}
		buffer = { id: planner.buffers.len().to_u32_wrap(), decl }

		({ ..planner, buffers: planner.buffers.append(decl) }, buffer)
	}

	add_pipeline : ResourcePlanner, PlannedPipeline -> (ResourcePlanner, PipelineRef)
	add_pipeline = |planner, decl| {
		pipeline = { id: planner.pipelines.len().to_u32_wrap(), decl }

		({ ..planner, pipelines: planner.pipelines.append(decl) }, pipeline)
	}
}

## A buffer of `items` packed with `elem_packer`, holding exactly them.
packed_buffer : Str, ResourcePlanner.BufferRole, ElementPacker(elem), List(elem) -> ResourcePlanner.BufferSpec
packed_buffer = |name, role, elem_packer, items| {
	name,
	role,
	element_size: elem_packer.stride,
	len: checked_len(name, items),
	contents: Bytes(items.join_map(elem_packer.to_bytes)),
}

## The length of a buffer's initial items, which a `U32` must hold.
checked_len : Str, List(a) -> U32
checked_len = |name, items| {
	len = items.len()
	if len > U32.highest.to_u64() {
		crash "resource planner: buffer ${name} has ${len.to_str()} elements; a U32 must hold its length"
	}

	len.to_u32_wrap()
}

## A declaration's provenance: its name and a digest of its `summary` and
## `contents`.
provenance_of : Str, Str, List(U8) -> Provenance
provenance_of = |name, summary, contents| {
	declared = Str.inspect((name, summary)).to_utf8().concat(contents)

	{ name, digest: Crypto.BLAKE3.hash(declared).to_bytes() }
}

declared_tex : ResourcePlanner.GraphTex -> ShaderReflection.DeclaredResource
declared_tex = |tex| { id: tex.tex_id(), provenance: tex.decl.provenance }

## Each reflected `ParameterBlock<T>` global with its constant buffer's size.
## Both lists are in descriptor-set-layout order.
parameter_blocks : List(ShaderReflection.GlobalParameter), ShaderReflection.PipelineLayout -> List(ResourcePlanner.ParameterBlock)
parameter_blocks = |globals, layout| {
	type_names = globals.join_map(
		|global|
			match global {
				ParameterBlock({ element_type, .. }) => [element_type.type_name]
				PushConstant(_) => []
			},
	)
	sizes = layout.descriptor_set_layouts.join_map(
		|set|
			set.binding_ranges.join_map(
				|range|
					match range.descriptor_type {
						ConstantBuffer => [range.size]
						_ => []
					},
			),
	)

	type_names.map_with_index(|type_name, i| { type_name, size: sizes.get(i) ?? 0 })
}

## The current slot of `buffer` from element `element` on. An offset past
## `U32` saturates, which graph validation reports as out of bounds.
element_target : ResourcePlanner.BufferRef, U32 -> ShaderReflection.BufferTarget
element_target = |buffer, element| {
	offset = element.to_u64() * buffer.decl.element_size.to_u64()
	max_offset = U32.highest.to_u64()
	clamped = if (offset > max_offset) max_offset else offset

	declared = { id: buffer.id, provenance: buffer.decl.provenance }

	{ buffer: declared, slot: Current, offset: clamped.to_u32_wrap() }
}
