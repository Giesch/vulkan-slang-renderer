import pf.ShaderReflection exposing [Float4x4]
import pf.ResourcePlanner
import pf.RenderGraph
import pf.GraphSet
import pf.RasterState
import BasicTriangle
import Particle
import Particles
import RocBlocks
import RocNoBlocks
import VertexPullingFloat3

## Builds render graphs from generated shader modules through the platform's
## public API, as an app would, and checks the host graph and frame values.
RenderGraphConsumer := {}

red = { x: 1.0, y: 0.0, z: 0.0 }

blue = { x: 0.0, y: 0.0, z: 1.0 }

green = { x: 0.0, y: 1.0, z: 0.0 }

left = { x: -1.0, y: -1.0, z: 0.0 }

top = { x: 0.0, y: 1.0, z: 0.0 }

right = { x: 1.0, y: -1.0, z: 0.0 }

# The basic-triangle shape: vertex pulling with a hardware index buffer.
(planner1, vertices) = ResourcePlanner.singleton_buffer(
	ResourcePlanner.empty,
	"vertices",
	BasicTriangle.Vertex.packer,
	[
		{ position: left, color: red },
		{ position: top, color: blue },
		{ position: right, color: green },
	],
)

(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)

(planner3, params) = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)

(planner4, pipe) = ResourcePlanner.graphics_pipeline(
	planner3,
	{
		name: "basic_triangle",
		shader: BasicTriangle.shader,
		uniforms: { params: params },
		raster: RasterState.default,
	},
)

node = RenderGraph.draw_indexed(pipe, indices, params, { vertices: vertices.addr() })

Ok(graph) = RenderGraph.new(planner4, node).map_ok(GraphSet.single)

matrices = {
	model: Float4x4.identity,
	view: Float4x4.identity,
	proj: Float4x4.identity,
}

expect graph.draw({ mvp: matrices }).to_host() == { graph_id: 0, values: [Bytes(matrices.to_bytes())] }

expect
	match GraphSet.definitions(graph) {
		[host] =>
			match (host.buffers, host.pipelines) {
				([vertex_buffer, index_buffer], [pipeline]) =>
					vertex_buffer.role == Singleton
						and vertex_buffer.element_size == 32
						and vertex_buffer.len == 3
						and vertex_buffer.contents == Bytes([left, top, right].map_with_index(|position, i| (position, i)).join_map(|(position, i)| BasicTriangle.Vertex.to_bytes({ position, color: [red, blue, green].get(i) ?? red })))
						and index_buffer.contents == Indices({ indices: [0, 1, 2], vertex_count: 3 })
						and pipeline.kind == Graphics
						and pipeline.stages == Graphics({ vertex_spv: BasicTriangle.shader.vertex_spv, fragment_spv: BasicTriangle.shader.fragment_spv, raster: RasterState.default })
						and pipeline.reflection_json == BasicTriangle.shader.reflection_json
						and host.uniform_buffers == [{ name: "params", size: 208 }]

				_ => Bool.False
			}

		_ => Bool.False
	}

# A compute dispatch over a GPU-updated buffer's previous and current copies.
particle : Particle.Particle
particle = {
	position: { x: 1.0, y: 2.0 },
	velocity: { x: 3.0, y: 4.0 },
	color: { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
}

(compute_planner1, particles) = ResourcePlanner.gpu_updated_buffer(ResourcePlanner.empty, "particles", Particle.Particle.packer, 64, [particle])

(compute_planner2, sim) = ResourcePlanner.uniform_buffer(compute_planner1, "sim", Particles.sim_params)

(compute_planner, sim_pipe) = ResourcePlanner.compute_pipeline(compute_planner2, { name: "particles", shader: Particles.shader, uniforms: { sim_params: sim } })

simulate = RenderGraph.dispatch(
	sim_pipe,
	sim,
	{ x: 1, y: 1, z: 1 },
	{ particles_in: particles.previous(), particles_out: particles.current() },
)

Ok(simulation) = RenderGraph.new(compute_planner, RenderGraph.repeat(simulate)).map_ok(GraphSet.single)

expect
	simulation.draw({ count: 2, body: { delta_time: 0.5 } }).to_host().values
		== [Count(2), Bytes(ShaderReflection.f32_bytes(0.5))]

# Indexed-indirect draws of three std430 float3 vertex streams.
pair = { position: left, color: red }

tail = { position: top, scale: 0.5 }

wide = { position: right, color: { x: 0.0, y: 0.0, z: 1.0, w: 1.0 } }

(indirect_planner1, pairs) = ResourcePlanner.immutable_buffer(ResourcePlanner.empty, "pairs", VertexPullingFloat3.PairVertex.packer, [pair])

(indirect_planner2, tails) = ResourcePlanner.immutable_buffer(indirect_planner1, "tails", VertexPullingFloat3.TailVertex.packer, [tail])

(indirect_planner3, wides) = ResourcePlanner.immutable_buffer(indirect_planner2, "wides", VertexPullingFloat3.WideVertex.packer, [wide])

(indirect_planner4, pull_indices) = ResourcePlanner.index_buffer(indirect_planner3, "indices", [0, 0, 0], 1)

(indirect_planner5, commands) = ResourcePlanner.indirect_buffer(
	indirect_planner4,
	"commands",
	[{ index_count: 3, instance_count: 1, first_index: 0, vertex_offset: 0, first_instance: 0 }],
)

(indirect_planner6, pull_params) = ResourcePlanner.uniform_buffer(indirect_planner5, "pull", VertexPullingFloat3.params)

(indirect_planner, pull_pipe) = ResourcePlanner.graphics_indirect_pipeline(
	indirect_planner6,
	{
		name: "pull",
		shader: VertexPullingFloat3.shader,
		uniforms: { params: pull_params },
		raster: RasterState.no_depth,
	},
)

pull = RenderGraph.draw_indexed_indirect(
	pull_pipe,
	pull_indices,
	pull_params,
	commands,
	0,
	1,
	{ pairs: pairs.addr(), tails: tails.addr(), wides: wides.addr() },
)

Ok(pulled) = RenderGraph.new(indirect_planner, pull).map_ok(GraphSet.single)

expect pulled.draw({}).to_host() == { graph_id: 0, values: [] }

expect
	match GraphSet.definitions(pulled) {
		[host] =>
			host.pipelines.map(|pipeline| pipeline.kind) == [GraphicsIndirect]
				and host.buffers.map(|buffer| buffer.role) == [Immutable, Immutable, Immutable, Index, IndirectArgs]

		_ => Bool.False
	}

# A push-block pipeline takes the `*_with_push` constructors. This shader's
# push block writes a storage texture, which a draw may not, and it declares
# two parameter blocks where a render graph shader declares exactly one.
(blocks_planner0, source) = ResourcePlanner.texture(ResourcePlanner.empty, "source", 4, 4, Rgba32Float)

(blocks_planner1, target) = ResourcePlanner.texture(blocks_planner0, "target", 4, 4, Rgba32Float)

(blocks_planner2, tint) = ResourcePlanner.uniform_buffer(blocks_planner1, "tint", RocBlocks.tint)

(blocks_planner3, block_params) = ResourcePlanner.uniform_buffer(blocks_planner2, "params", RocBlocks.params)

(blocks_planner4, block_wides) = ResourcePlanner.cpu_updated_buffer(blocks_planner3, "wides", RocBlocks.WideVertex.packer, 4, [])

(blocks_planner5, block_pairs) = ResourcePlanner.immutable_buffer(blocks_planner4, "pairs", RocBlocks.PairVertex.packer, [{ position: left, color: red }])

(blocks_planner6, block_tails) = ResourcePlanner.immutable_buffer(blocks_planner5, "tails", RocBlocks.TailVertex.packer, [{ position: top, scale: 0.5 }])

(blocks_planner, blocks_pipe) = ResourcePlanner.graphics_pipeline(
	blocks_planner6,
	{
		name: "blocks",
		shader: RocBlocks.shader,
		uniforms: { tint: tint, params: block_params },
		raster: RasterState.default,
	},
)

blocks = RenderGraph.draw_vertex_count_with_push(
	blocks_pipe,
	block_params,
	3,
	{ pairs: block_pairs.addr(), tex: source.read(), tails: block_tails.addr().as_read() },
	{
		data: { tint: top, strength: 0.5 },
		bindings: { wides: block_wides.addr(), target: target.mutate() },
	},
)

expect
	match RenderGraph.new(blocks_planner, blocks) {
		Ok(_) => Bool.False
		Err(InvalidRenderGraph(message)) =>
			message
				== [
					"render graph validation failed:",
					"  - draw draw0 can only read graph textures (texture target)",
					"  - pipeline blocks's shader declares 2 ParameterBlock globals; a render graph shader needs exactly one ParameterBlock<T>",
				]
					|> Str.join_with("\n")
	}

# A shader without a parameter block cannot be a render graph pipeline.
(no_blocks_planner, no_blocks_pipe) = ResourcePlanner.graphics_pipeline(
	planner4,
	{ name: "no_blocks", shader: RocNoBlocks.shader, uniforms: {}, raster: RasterState.default },
)

expect
	match RenderGraph.new(no_blocks_planner, RenderGraph.draw_vertex_count(no_blocks_pipe, params, 3, { vertices: vertices.addr() })) {
		Ok(_) => Bool.False
		Err(InvalidRenderGraph(message)) =>
			message.contains("pipeline no_blocks's shader declares 0 ParameterBlock globals")
	}
