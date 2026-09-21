import ShaderReflection
import RenderGraphLower
import RenderGraphValidate
import RenderGraph

## Internal validated definition. Only Graphs turns definitions into registered
## packers; raw definitions cannot submit frames.
ValidatedRenderGraph(frame) :: {
	wire : Wire,
	pack : frame -> List(List(U8)),
}.{

	## Lower and validate the nodes. Rejects the blueprint with the aggregated
	## validation message.
	new : RenderGraph(frame) -> Try(ValidatedRenderGraph(frame), [InvalidRenderGraph(Str)])
	new = |bp| {
		decls = RenderGraph.decls(bp)
		checked = decls.map_with_index(|node, index| check_node(node, index.to_u32_wrap()))
		draws = decls.map_with_index(
			|node, index| {
				pipeline: index.to_u32_wrap(),
				call: match node.call {
					WholeIndexed => WholeIndexed
					VertexCount(count) => VertexCount(count)
				},
				uniform: {
					slot: index.to_u32_wrap(),
					gpu_size: node.uniform.size,
					data_size: node.uniform.size,
					bindings: [],
				},
			},
		)
		lowered = RenderGraphLower.lower(draws)
		errors = [
			checked.join_map(|item| item.errors),
			lowered.errors,
			RenderGraphValidate.validate(lowered.desc, lowered.schemas),
		].join()
		if !errors.is_empty() {
			Err(InvalidRenderGraph(RenderGraphValidate.validation_message(errors)))
		} else {
			Ok(
				ValidatedRenderGraph.(
					{
						wire: {
							pipelines: checked.map(|item| item.pipeline),
							uniforms: decls.map(|node| HostUniform.{ name: node.uniform.name, size: node.uniform.size }),
							draws: draws.map(
								|draw| HostDraw.{
									pipeline: draw.pipeline,
									uniform: draw.uniform.slot,
									call: match draw.call {
										VertexCount(count) => VertexCount(count)
										_ => WholeIndexed
									},
								},
							),
						},
						pack: RenderGraph.packer(bp),
					},
				),
			)
		}
	}

	## The blueprint's packer preserves validated node order.
	packer : ValidatedRenderGraph(frame) -> (frame -> List(List(U8)))
	packer = |graph| graph.pack

	definition : ValidatedRenderGraph(frame) -> HostGraph
	definition = |graph| HostGraph.(graph.wire)

	## The constant buffer sizes a shader declares, in descriptor-set-layout order.
	constant_buffer_sizes : ShaderReflection.GraphicsReflection -> List(U64)
	constant_buffer_sizes = |reflection|
		reflection.pipeline_layout.descriptor_set_layouts.join_map(
			|set| set.binding_ranges.join_map(
				|range| match range.descriptor_type {
					ConstantBuffer => [range.size]
					_ => []
				},
			),
		)

	## Check one node's uniform against its shader and its mesh, and build
	## the host pipeline. The node's uniform buffer shares its index.
	check_node : RenderGraph.Decl, U32 -> { pipeline : HostPipeline, errors : List(RenderGraphValidate.GraphError) }
	check_node = |node, index| {
		expected = constant_buffer_sizes(node.shader.reflection)
		count_errors = if expected.len() != 1 {
			[PipelineUniformCount({ pipeline: node.name, expected: expected.len(), found: 1 })]
		} else {
			[]
		}

		binding_errors = match expected.get(node.uniform.index.to_u64()) {
			Err(_) =>
				[
					UniformIndexOutOfRange({
						pipeline: node.name,
						uniform: node.uniform.name,
						index: node.uniform.index,
						count: expected.len(),
					}),
				]
			Ok(size) =>
				if node.uniform.size.to_u64() == size {
					[]
				} else {
					[PipelineUniformSize({ pipeline: node.name, uniform: node.uniform.name, size: node.uniform.size, expected: size })]
				}
			}

		mesh_errors = match node.mesh {
			VertexCount => []
			Indexed(mesh) => {
				bytes = mesh.vertex_bytes.len()
				if bytes == 0 or mesh.indices.is_empty() {
					[EmptyMesh({ pipeline: node.name })]
				} else if mesh.vertex_stride == 0 or bytes % mesh.vertex_stride.to_u64() != 0 {
					[MeshStride({ pipeline: node.name, bytes, stride: mesh.vertex_stride })]
				} else {
					[]
				}
			}
		}

		pipeline = HostPipeline.{
			name: node.name,
			vertex_spv: node.shader.vertex_spv,
			fragment_spv: node.shader.fragment_spv,
			reflection_json: node.shader.reflection_json,
			mesh: match node.mesh {
				VertexCount => VertexCount
				Indexed(mesh) => Indexed(HostIndexedMesh.{ vertex_bytes: mesh.vertex_bytes, indices: mesh.indices })
			},
			uniforms: [index],
		}

		{ pipeline, errors: [count_errors, binding_errors, mesh_errors].join() }
	}

	## The host-facing bundle. Every type is nominal so the generated glue
	## names its Rust struct.
	Wire : {
		pipelines : List(HostPipeline),
		uniforms : List(HostUniform),
		draws : List(HostDraw),
	}

	HostIndexedMesh := {
		vertex_bytes : List(U8),
		indices : List(U32),
	}

	HostMesh := [Indexed(HostIndexedMesh), VertexCount]

	HostPipeline := {
		name : Str,
		vertex_spv : List(U8),
		fragment_spv : List(U8),
		reflection_json : Str,
		mesh : HostMesh,

		## indices into `Draw.uniforms`, in descriptor-set-layout order
		uniforms : List(U32),
	}

	HostUniform := {
		name : Str,
		size : U32,
	}

	HostDrawCall := [WholeIndexed, VertexCount(U32)]

	HostDraw := {
		pipeline : U32,
		uniform : U32,
		call : HostDrawCall,
	}

	HostGraph := Wire
}
