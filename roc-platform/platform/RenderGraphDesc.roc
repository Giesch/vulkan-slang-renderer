## Plain-data description consumed by validation and lowering.
##
## Mirrors `crates/render-graph/src/runtime/desc.rs` type for type. Ids index
## the tables of `GraphDesc` and `SchemaTable`. Two fields carry different
## names because Roc reserves the words: `import_decls` is `imports` and
## `raster_targets` is `targets`.
RenderGraphDesc := {}.{
	Maybe(a) : [None, Some(a)]

	TexId : U32
	BufferId : U32
	ImportId : U32
	ValueId : U32
	UniformId : U32
	PipelineId : U32
	SchemaId : U32
	FieldKey : U16

	GraphFormat : [R32Float, Rgba32Float]

	GraphDesc : {
		textures : List(TexDecl),
		buffers : List(BufferDecl),
		import_decls : List(ImportDecl),
		values : List(ValueDecl),
		uniforms : List(UniformDecl),
		pipelines : List(PipelineDecl),
		uploads : List(UploadDesc),
		passes : List(PassDesc),
	}

	empty : GraphDesc
	empty = {
		textures: [],
		buffers: [],
		import_decls: [],
		values: [],
		uniforms: [],
		pipelines: [],
		uploads: [],
		passes: [],
	}

	TexDecl : {
		name : Str,
		format : GraphFormat,
		size : SizeClass,
		usage : TexUsage,
	}

	SizeClass : [Fixed(U32, U32), Window, WindowDiv(U32)]

	TexUsage : [Storage, Color, Depth]

	BufferDecl : {
		name : Str,
		kind : BufferKind,
		capacity : Maybe(U32),
		elem_size : Maybe(U32),
	}

	BufferKind : [GpuOnlyFlight, Singleton, Immutable, Storage]

	ImportDecl : { name : Str }

	ValueDecl : {
		name : Str,
		kind : ValueKind,
		optional : Bool,
	}

	ValueKind : [
		Count,
		Groups,
		Bytes({ schema : SchemaId }),
		Array({ elem : SchemaId, max_len : U32 }),
	]

	UniformDecl : {
		name : Str,
		schema : SchemaId,
		source : UniformSourceDesc,
	}

	UniformSourceDesc : {
		data : Maybe(ValueId),
		bindings : List((FieldKey, ResourceRef)),
	}

	PipelineDecl : {
		name : Str,
		kind : PipelineKind,
		params : SchemaId,
		push : Maybe(SchemaId),
	}

	PipelineKind : [Compute, Graphics]

	UploadDesc : {
		name : Str,
		buffer : BufferId,
		value : ValueId,
	}

	PassDesc : [
		Leaf(LeafPass),
		When({ name : Str, value : ValueId, body : List(LeafPass) }),
		Repeat({ name : Str, count : ValueId, body : List(LeafPass) }),
	]

	LeafPass : [Compute(DispatchDesc), Raster(RasterDesc)]

	## Every leaf pass in order, flattening scopes.
	leaves : GraphDesc -> List(LeafPass)
	leaves = |desc|
		List.join_map(
			desc.passes,
			|pass|
				match pass {
					Leaf(leaf) => [leaf]
					When(when) => when.body
					Repeat(repeat) => repeat.body
				},
		)

	DispatchDesc : {
		name : Str,
		pipeline : PipelineId,
		uniform : UniformId,
		groups : GroupSource,
		push : Maybe(PushDesc),
	}

	GroupSource : [Fixed(U32, U32, U32), Value(ValueId)]

	PushDesc : {
		data : Maybe(ValueId),
		schema : SchemaId,
		bindings : List((FieldKey, ResourceRef)),
	}

	RasterDesc : {
		name : Str,
		raster_targets : RasterTargets,
		draws : List(DrawDesc),
	}

	RasterTargets : [Main, Offscreen({ color : List(TexId), depth : Maybe(TexId) })]

	DrawDesc : {
		name : Str,
		pipeline : PipelineId,
		uniform : UniformId,
		call : DrawCall,
		push : Maybe(PushDesc),
	}

	DrawCall : [
		VertexCount(U32),
		WholeIndexed,
		IndexRange({ first_index : U32, index_count : U32 }),
		IndexedIndirect({ args : BufferRef, draw_count : U32 }),
	]

	ResourceRef : [
		Tex(TexId, TexAccess),
		Buf(BufferRef, BufAccess),
		External(ImportId),
	]

	TexAccess : [Read, ReadPrevious, Write, Mutate]

	BufAccess : [Read, Mutate]

	BufferRef : {
		buffer : BufferId,
		slot : SlotSel,
		offset : U32,
		range : Maybe(U32),
	}

	SlotSel : [Current, Previous]

	SchemaTable : { schemas : List(SchemaDesc) }

	SchemaDesc : {
		name : Str,
		size : U32,
		resource_fields : List(ResourceFieldKind),
		layout : Maybe(SchemaLayout),
	}

	ResourceFieldKind : [SampledTex, StorageTex, BufAddr]

	SchemaLayout : { fields : List(SchemaField) }

	SchemaField : {
		key : FieldKey,
		offset : U32,
		len : U32,
		kind : SchemaFieldKind,
	}

	SchemaFieldKind : [Data({ src_offset : U32 }), SampledTex, StorageTex, BufAddr]
}
