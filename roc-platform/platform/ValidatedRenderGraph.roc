import RasterState
import RenderGraphDesc
import RenderGraphLower
import RenderGraphValidate
import ResourcePlanner

## A render graph that lowered and validated, with the host wire form of its
## resources and description and its per-frame packer.
##
## Only `GraphSet` turns a validated graph into a registered packer; a raw
## validated graph cannot submit frames.
ValidatedRenderGraph(frame) :: {
	host : HostGraph,
	pack : frame -> List(HostFrameValue),
	fingerprint : List(U8),
}.{

	## Lowers `nodes` against `planner` and validates the description and the
	## declarations. `pack` emits one value per node value site, in lowering
	## order. Rejects the graph with the aggregated validation message.
	new : ResourcePlanner, List(RenderGraphLower.Node), (frame -> List(HostFrameValue)) -> Try(ValidatedRenderGraph(frame), [InvalidRenderGraph(Str)])
	new = |planner, nodes, pack_sites| {
		lowered = RenderGraphLower.lower(planner, nodes)
		desc_errors =
			match RenderGraphValidate.validate(lowered.desc, lowered.schemas) {
				Ok(_) => []
				Err(errors) => errors
			}

		errors = [
			lowered.errors,
			desc_errors,
			RenderGraphValidate.validate_resources(planner, lowered.desc),
		].join()

		if !errors.is_empty() {
			return Err(InvalidRenderGraph(RenderGraphValidate.validation_message(errors)))
		}

		host = {
			buffers: planner.buffers().map(host_buffer),
			uniform_buffers: planner.uniforms().map(|uniform| { name: uniform.name, size: uniform.layout.gpu_size }),
			pipelines: planner.pipelines().map(host_pipeline),
			import_decls: planner.import_decls().map(|import_decl| { name: import_decl.name }),
			desc: lowered.desc,
			schemas: lowered.schemas,
			uniform_slots: lowered.uniform_slots,
			pipeline_indices: lowered.pipeline_indices,
		}
		sites = lowered.sites
		pack = |frame| resolve_sites(sites, pack_sites(frame))

		digest = Crypto.BLAKE3.hash(Str.inspect(host).to_utf8()).to_bytes()

		Ok({ host, pack, fingerprint: digest })
	}

	## The graph's packer, without the host assets it was validated with.
	packer : ValidatedRenderGraph(frame) -> (frame -> List(HostFrameValue))
	packer = |graph| graph.pack

	definition : ValidatedRenderGraph(frame) -> HostGraph
	definition = |graph| graph.host

	## A digest of the host graph, identifying it without its assets.
	fingerprint : ValidatedRenderGraph(frame) -> List(U8)
	fingerprint = |graph| graph.fingerprint

	## Everything the host creates for one graph, and the description it
	## executes. Description tables index the host tables: `desc.buffers[i]`
	## is `buffers[i]`, `desc.import_decls[i]` is `import_decls[i]`,
	## `desc.uniforms[i]` writes `uniform_buffers[uniform_slots[i]]`, and
	## `desc.pipelines[i]` is `pipelines[pipeline_indices[i]]`.
	##
	## Every type is nominal so the generated glue names each Rust struct.
	HostGraph := {
		buffers : List(HostBuffer),
		uniform_buffers : List(HostUniformBuffer),
		pipelines : List(HostPipeline),
		import_decls : List(HostImport),
		desc : RenderGraphDesc.GraphDesc,
		schemas : RenderGraphDesc.SchemaTable,
		uniform_slots : List(U32),
		pipeline_indices : List(U32),
	}.{
		is_eq : _
	}

	## `len` elements of `element_size` bytes, initialized from `contents`.
	HostBuffer := {
		name : Str,
		role : HostBufferRole,
		element_size : U32,
		len : U32,
		contents : HostBufferContents,
	}.{
		is_eq : _
	}

	## The renderer constructor a buffer uses; `IndirectArgs` is an
	## immutable buffer of `vkCmdDrawIndexedIndirect` records.
	HostBufferRole := [CpuUpdated, GpuUpdated, Immutable, Singleton, Index, IndirectArgs].{
		is_eq : _
	}

	## Packed leading elements, or an index buffer's indices and vertex bound.
	HostBufferContents := [Bytes(List(U8)), Indices(HostIndices)].{
		is_eq : _
	}

	HostIndices := { indices : List(U32), vertex_count : U32 }.{
		is_eq : _
	}

	## A uniform buffer of `size` bytes, written once per frame.
	HostUniformBuffer := { name : Str, size : U32 }.{
		is_eq : _
	}

	## `uniform_buffers` indexes `HostGraph.uniform_buffers`, one per
	## parameter block in descriptor-set-layout order.
	HostPipeline := {
		name : Str,
		kind : RenderGraphDesc.PipelineKind,
		stages : HostPipelineStages,
		reflection_json : Str,
		uniform_buffers : List(U32),
	}.{
		is_eq : _
	}

	HostPipelineStages := [Graphics(HostGraphicsStages), Compute(HostComputeStages)].{
		is_eq : _
	}

	HostGraphicsStages := {
		vertex_spv : List(U8),
		fragment_spv : List(U8),
		raster : RasterState,
	}.{
		is_eq : _
	}

	HostComputeStages := {
		compute_spv : List(U8),
		workgroup_size : HostWorkgroupSize,
	}.{
		is_eq : _
	}

	HostWorkgroupSize := { x : U32, y : U32, z : U32 }.{
		is_eq : _
	}

	## An external sampled texture the host supplies by name.
	HostImport := { name : Str }.{
		is_eq : _
	}

	## One frame's value for one `ValueDecl`, in declaration order, as the
	## Rust `PlainValue` takes it.
	HostFrameValue := [Absent, Count(U32), Bytes(List(U8)), Array(HostArray)].{
		is_eq : _
	}

	## `count` packed elements.
	HostArray := { count : U32, bytes : List(U8) }.{
		is_eq : _
	}
}

host_buffer : ResourcePlanner.PlannedBuffer -> ValidatedRenderGraph.HostBuffer
host_buffer = |buffer| {
	role =
		match buffer.role {
			CpuUpdated => CpuUpdated
			GpuUpdated => GpuUpdated
			Immutable => Immutable
			Singleton => Singleton
			Index => Index
			IndirectArgs => IndirectArgs
		}

	contents =
		match buffer.contents {
			Bytes(bytes) => Bytes(bytes)
			Indices({ indices, vertex_count }) => Indices({ indices, vertex_count })
		}

	{ name: buffer.name, role, element_size: buffer.element_size, len: buffer.len, contents }
}

host_pipeline : ResourcePlanner.PlannedPipeline -> ValidatedRenderGraph.HostPipeline
host_pipeline = |pipeline| {
	stages =
		match pipeline.stages {
			Graphics({ vertex_spv, fragment_spv, raster }) => Graphics({ vertex_spv, fragment_spv, raster })
			Compute({ compute_spv, workgroup_size }) =>
				Compute({ compute_spv, workgroup_size: { x: workgroup_size.x, y: workgroup_size.y, z: workgroup_size.z } })
		}

	{
		name: pipeline.name,
		kind: pipeline.kind,
		stages,
		reflection_json: pipeline.reflection_json,
		uniform_buffers: pipeline.uniform_slots,
	}
}

## The frame's description values from its packed sites: a value site passes
## through, and a shared uniform's later site must repeat the first node's
## bytes, since the uniform is written once per frame.
resolve_sites : List(RenderGraphLower.SitePlan), List(ValidatedRenderGraph.HostFrameValue) -> List(ValidatedRenderGraph.HostFrameValue)
resolve_sites = |sites, raw|
	if (sites.len() != raw.len())
		crash "render graph: packed ${raw.len().to_str()} frame values for ${sites.len().to_str()} value sites"
	else
		sites.map_with_index(|site, index| (site, index)).fold(
			[],
			|values, (site, index)|
				match site {
					Value(_) => values.append(raw.get(index) ?? Absent)
					SameAs(first) => {
						if raw.get(index) != raw.get(first) {
							crash "render graph: two nodes sharing one uniform buffer supplied different data in the same frame; the buffer is written once per frame"
						}

						values
					}
				},
		)
