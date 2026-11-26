## Plain-data description consumed by render graph validation and lowering.
##
## Mirrors `crates/render-graph/src/runtime/desc.rs` type for type. Ids index
## the tables of `GraphDesc` and `SchemaTable`. Two fields carry different
## names because Roc reserves the words: `import_decls` is `imports` and
## `raster_targets` is `targets`.
##
## Every type, including each tag payload record, is nominal and
## non-polymorphic, so the host glue names each one instead of hashing its
## structure. Optional fields use one concrete `Maybe*` union per payload type
## rather than a generic `Maybe(a)`.
RenderGraphDesc := {}.{
	TexId : U32

	BufferId : U32

	ImportId : U32

	ValueId : U32

	UniformId : U32

	PipelineId : U32

	SchemaId : U32

	## A resource field's position in its schema's `resource_fields`; binding
	## `i` of a uniform or push source carries key `i`.
	FieldKey : U16

	MaybeU32 := [None, Some(U32)].{
		is_eq : _
	}

	GraphFormat := [R32Float, Rgba32Float].{
		is_eq : _
	}

	GraphDesc := {
		textures : List(TexDecl),
		buffers : List(BufferDecl),
		import_decls : List(ImportDecl),
		values : List(ValueDecl),
		uniforms : List(UniformDecl),
		pipelines : List(PipelineDecl),
		uploads : List(UploadDesc),
		passes : List(PassDesc),

		## A picking readback after the main raster pass. The Roc graph API
		## has no picking node yet, so lowering leaves this `None`.
		picking : MaybePicking,
	}.{
		is_eq : _
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
		picking: None,
	}

	## A picking readback whose cursor position arrives as an 8-byte
	## `[f32; 2]` value each frame; an absent optional value skips it.
	PickingDesc := { cursor : ValueId }.{
		is_eq : _
	}

	MaybePicking := [None, Some(PickingDesc)].{
		is_eq : _
	}

	TexDecl := {
		name : Str,
		format : GraphFormat,
		size : SizeClass,
		usage : TexUsage,
	}.{
		is_eq : _
	}

	SizeClass := [Fixed(U32, U32), Window, WindowDiv(U32)].{
		is_eq : _
	}

	TexUsage := [Storage, Color, Depth].{
		is_eq : _
	}

	BufferDecl := {
		name : Str,
		kind : BufferKind,
		capacity : MaybeU32,
		elem_size : MaybeU32,
	}.{
		is_eq : _
	}

	## `Index` is a hardware index buffer whose `capacity` is its index count.
	## Indexed-indirect argument buffers are `Immutable`.
	BufferKind := [GpuUpdatedFlight, Singleton, Immutable, Storage, Index].{
		is_eq : _
	}

	ImportDecl := { name : Str }.{
		is_eq : _
	}

	ValueDecl := {
		name : Str,
		kind : ValueKind,

		## The frame may supply `Absent`.
		optional : Bool,
		scope : ValueScope,
	}.{
		is_eq : _
	}

	## The control-flow scope a value belongs to. `When` values are present
	## exactly when their gate is; the gate is itself `When({ gate })`. An
	## upload reading a `Repeat` value is skipped on a zero-trip frame.
	ValueScope := [Top, When(WhenScope), Repeat(RepeatScope)].{
		is_eq : _
	}

	WhenScope := { gate : ValueId }.{
		is_eq : _
	}

	RepeatScope := { count : ValueId }.{
		is_eq : _
	}

	ValueKind := [Count, Groups, Bytes(BytesValue), Array(ArrayValue)].{
		is_eq : _
	}

	## Bytes of exactly `schema`'s size.
	BytesValue := { schema : SchemaId }.{
		is_eq : _
	}

	## Up to `max_len` packed elements of `elem`'s size.
	ArrayValue := { elem : SchemaId, max_len : U32 }.{
		is_eq : _
	}

	UniformDecl := {
		name : Str,
		schema : SchemaId,
		source : UniformSourceDesc,
	}.{
		is_eq : _
	}

	UniformSourceDesc := {
		data : MaybeU32,
		bindings : List(ResourceBinding),
	}.{
		is_eq : _
	}

	## One resolved binding of a uniform or push source.
	ResourceBinding := { key : FieldKey, resource : ResourceRef }.{
		is_eq : _
	}

	PipelineDecl := {
		name : Str,
		kind : PipelineKind,
		params : SchemaId,
		push : MaybeU32,
	}.{
		is_eq : _
	}

	## `Graphics` takes direct vertex-count and indexed draws;
	## `GraphicsIndirect` takes indexed-indirect draws and its shader declares
	## `SV_DrawIndex`.
	PipelineKind := [Compute, Graphics, GraphicsIndirect].{
		is_eq : _
	}

	UploadDesc := {
		name : Str,
		buffer : BufferId,
		value : ValueId,
	}.{
		is_eq : _
	}

	PassDesc := [Leaf(LeafPass), When(WhenPass), Repeat(RepeatPass)].{
		is_eq : _
	}

	## A body that runs when the optional value `value` is present.
	WhenPass := { name : Str, value : ValueId, body : List(LeafPass) }.{
		is_eq : _
	}

	## A body that runs `count` times, where `count` is a count value.
	RepeatPass := { name : Str, count : ValueId, body : List(LeafPass) }.{
		is_eq : _
	}

	LeafPass := [Compute(DispatchDesc), Raster(RasterDesc)].{
		is_eq : _
	}

	## Every leaf pass in order, flattening scopes.
	leaves : GraphDesc -> List(LeafPass)
	leaves = |desc|
		desc.passes.join_map(
			|pass|
				match pass {
					Leaf(leaf) => [leaf]
					When({ body, .. }) => body
					Repeat({ body, .. }) => body
				},
		)

	## One dispatch or draw, keeping command boundaries inside raster passes.
	Command : {
		name : Str,
		pipeline : PipelineId,
		uniform : UniformId,
		push : MaybePush,
	}

	## A leaf's commands: its dispatch, or each draw of its raster pass.
	commands : LeafPass -> List(Command)
	commands = |leaf|
		match leaf {
			Compute(dispatch) =>
				[
					{
						name: dispatch.name,
						pipeline: dispatch.pipeline,
						uniform: dispatch.uniform,
						push: dispatch.push,
					},
				]

			Raster(raster) =>
				raster.draws.map(
					|draw| {
						name: draw.name,
						pipeline: draw.pipeline,
						uniform: draw.uniform,
						push: draw.push,
					},
				)
		}

	## A command's uniform bindings followed by its push bindings.
	command_bindings : GraphDesc, Command -> List(ResourceBinding)
	command_bindings = |desc, command| {
		uniform_bindings =
			match desc.uniforms.get(command.uniform.to_u64()) {
				Ok(uniform) => uniform.source.bindings
				Err(_) => []
			}

		push_bindings =
			match command.push {
				Some(push) => push.bindings
				None => []
			}

		uniform_bindings.concat(push_bindings)
	}

	DispatchDesc := {
		name : Str,
		pipeline : PipelineId,
		uniform : UniformId,
		groups : GroupSource,
		push : MaybePush,
	}.{
		is_eq : _
	}

	GroupSource := [Fixed(GroupCount), Value(ValueId)].{
		is_eq : _
	}

	GroupCount := { x : U32, y : U32, z : U32 }.{
		is_eq : _
	}

	PushDesc := {
		data : MaybeU32,
		schema : SchemaId,
		bindings : List(ResourceBinding),
	}.{
		is_eq : _
	}

	MaybePush := [None, Some(PushDesc)].{
		is_eq : _
	}

	RasterDesc := {
		name : Str,
		raster_targets : RasterTargets,
		draws : List(DrawDesc),
	}.{
		is_eq : _
	}

	RasterTargets := [Main, Offscreen(OffscreenTargets)].{
		is_eq : _
	}

	OffscreenTargets := { color : List(TexId), depth : MaybeU32 }.{
		is_eq : _
	}

	DrawDesc := {
		name : Str,
		pipeline : PipelineId,
		uniform : UniformId,
		call : DrawCall,
		push : MaybePush,
	}.{
		is_eq : _
	}

	DrawCall := [
		VertexCount(U32),
		WholeIndexed(WholeIndexedCall),
		IndexRange(IndexRangeCall),
		IndexedIndirect(IndexedIndirectCall),
	].{
		is_eq : _
	}

	WholeIndexedCall := { indices : BufferId }.{
		is_eq : _
	}

	IndexRangeCall := { indices : BufferId, first_index : U32, index_count : U32 }.{
		is_eq : _
	}

	IndexedIndirectCall := { indices : BufferId, args : BufferRef, draw_count : U32 }.{
		is_eq : _
	}

	## The index buffer an indexed draw binds; `None` for a vertex-count draw.
	index_buffer : DrawCall -> MaybeU32
	index_buffer = |call|
		match call {
			VertexCount(_) => None
			WholeIndexed({ indices }) => Some(indices)
			IndexRange({ indices, .. }) => Some(indices)
			IndexedIndirect({ indices, .. }) => Some(indices)
		}

	ResourceRef := [
		Tex(TexId, TexAccess),
		Buf(BufferRef, BufAccess),
		External(ImportId),
	].{
		is_eq : _
	}

	TexAccess := [Read, ReadPrevious, Write, Mutate].{
		is_eq : _
	}

	BufAccess := [Read, Mutate].{
		is_eq : _
	}

	BufferRef := {
		buffer : BufferId,
		slot : SlotSel,
		offset : U32,
		range : MaybeU32,
	}.{
		is_eq : _
	}

	SlotSel := [Current, Previous].{
		is_eq : _
	}

	SchemaTable := { schemas : List(SchemaDesc) }.{
		is_eq : _
	}

	SchemaDesc := {
		name : Str,
		size : U32,
		resource_fields : List(ResourceFieldKind),
		layout : MaybeLayout,
	}.{
		is_eq : _
	}

	ResourceFieldKind := [SampledTex, StorageTex, BufAddr].{
		is_eq : _
	}

	SchemaLayout := { fields : List(SchemaField) }.{
		is_eq : _
	}

	MaybeLayout := [None, Some(SchemaLayout)].{
		is_eq : _
	}

	SchemaField := {
		key : FieldKey,
		offset : U32,
		len : U32,
		kind : SchemaFieldKind,
	}.{
		is_eq : _
	}

	SchemaFieldKind := [Data(DataField), SampledTex, StorageTex, BufAddr].{
		is_eq : _
	}

	## A data field's offset into its block's packed data value.
	DataField := { src_offset : U32 }.{
		is_eq : _
	}
}
