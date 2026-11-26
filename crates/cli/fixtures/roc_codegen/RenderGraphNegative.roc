import pf.ResourcePlanner
import pf.RenderGraph
import pf.RasterState
import BasicTriangle
import Particle
import RocBlocks
import VertexPullingFloat3

## The positive control for the real-Roc gate's negative typecheck cases:
## each case swaps `checked` for a misuse of the render graph API that must
## fail to typecheck.
RenderGraphNegative := {}

(planner1, vertices) = ResourcePlanner.singleton_buffer(ResourcePlanner.empty, "vertices", BasicTriangle.Vertex.packer, [])

(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)

(planner3, params) = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)

(planner4, pipe) = ResourcePlanner.graphics_pipeline(planner3, { name: "triangle", shader: BasicTriangle.shader, uniforms: { params: params }, raster: RasterState.default })

(planner5, particles) = ResourcePlanner.singleton_buffer(planner4, "particles", Particle.Particle.packer, [])

(planner6, tint) = ResourcePlanner.uniform_buffer(planner5, "tint", RocBlocks.tint)

(planner7, block_params) = ResourcePlanner.uniform_buffer(planner6, "block_params", RocBlocks.params)

(planner8, blocks_pipe) = ResourcePlanner.graphics_pipeline(planner7, { name: "blocks", shader: RocBlocks.shader, uniforms: { tint: tint, params: block_params }, raster: RasterState.default })

(planner9, pull_params) = ResourcePlanner.uniform_buffer(planner8, "pull", VertexPullingFloat3.params)

(planner10, pairs) = ResourcePlanner.immutable_buffer(planner9, "pairs", VertexPullingFloat3.PairVertex.packer, [])

(planner11, tails) = ResourcePlanner.immutable_buffer(planner10, "tails", VertexPullingFloat3.TailVertex.packer, [])

(planner12, wides) = ResourcePlanner.immutable_buffer(planner11, "wides", VertexPullingFloat3.WideVertex.packer, [])

(planner13, commands) = ResourcePlanner.indirect_buffer(planner12, "commands", [])

(_planner, pull_pipe) = ResourcePlanner.graphics_indirect_pipeline(planner13, { name: "pull", shader: VertexPullingFloat3.shader, uniforms: { params: pull_params }, raster: RasterState.default })

pull_bindings = { pairs: pairs.addr(), tails: tails.addr(), wides: wides.addr() }

pulled = RenderGraph.draw_indexed_indirect(pull_pipe, indices, pull_params, commands, 0, 1, pull_bindings)

triangle_bindings = { vertices: vertices.addr() }

checked = RenderGraph.draw_indexed(pipe, indices, params, triangle_bindings)
