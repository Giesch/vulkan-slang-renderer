app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner
import pf.ShaderReflection exposing [Float4x4]

import Generated/BasicTriangle
import Generated/Mltrs
import Generated/ShaderAtlas

game = Game.new({ init!, graphs: graph, draw })

init! : {} => Game.Init
init! = |_| {
	window_title: "Basic Triangle from Roc!",
	shader_hot_reload: Bool.False,
	shaders_source_dir: ShaderAtlas.shaders_source_dir,
}

draw : Game.Frame -> GraphSet.Submission(_)
draw = |frame| graph.draw({ mvp: mvp_with_aspect_ratio(frame.aspect_ratio) })

red = { x: 1.0, y: 0.0, z: 0.0 }

green = { x: 0.0, y: 1.0, z: 0.0 }

blue = { x: 0.0, y: 0.0, z: 1.0 }

left = { x: -1.0, y: -1.0, z: 0.0 }

right = { x: 1.0, y: -1.0, z: 0.0 }

top = { x: 0.0, y: 1.0, z: 0.0 }

(planner1, vertices) = ResourcePlanner.singleton_buffer(
	ResourcePlanner.empty,
	"vertices",
	BasicTriangle.Vertex.packer,
	[
		{ position: left, color: red },
		{ position: right, color: green },
		{ position: top, color: blue },
	],
)

(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)

(planner3, params) = ResourcePlanner.uniform_buffer(planner2, "params", BasicTriangle.params)

(planner4, pipeline) = ResourcePlanner.graphics_pipeline(
	planner3,
	{
		name: "basic_triangle",
		shader: BasicTriangle.shader,
		uniforms: { params: params },
		raster: RasterState.default,
	},
)

node = RenderGraph.draw_indexed(pipeline, indices, params, { vertices: vertices.addr() })

Ok(graph) = RenderGraph.new(planner4, node).map_ok(GraphSet.single)

# Column-major like glam: `row_i` holds column `i`,
# which the row-major shader multiplies as `position * M`.
mvp_with_aspect_ratio : F32 -> Mltrs.MvpMatrices
mvp_with_aspect_ratio = |aspect| {
	frustum = { fov_y_degrees: 45.0, aspect, near: 0.1, far: 10.0 }

	{ model: Float4x4.identity, view: view_from_z6, proj: perspective(frustum) }
}

## The view from `(0, 0, 6)` towards the origin with `+Y` up.
view_from_z6 : Float4x4
view_from_z6 = {
	row_0: { x: 1.0, y: 0.0, z: 0.0, w: 0.0 },
	row_1: { x: 0.0, y: 1.0, z: 0.0, w: 0.0 },
	row_2: { x: 0.0, y: 0.0, z: 1.0, w: 0.0 },
	row_3: { x: 0.0, y: 0.0, z: -6.0, w: 1.0 },
}

Frustum : { fov_y_degrees : F32, aspect : F32, near : F32, far : F32 }

## A right-handed perspective projection with a 0..1 depth range.
perspective : Frustum -> Float4x4
perspective = |{ fov_y_degrees, aspect, near, far }| {
	focal_length = 1.0 / F32.tan(fov_y_degrees * F32.pi / 360.0)

	{
		row_0: { x: focal_length / aspect, y: 0.0, z: 0.0, w: 0.0 },
		row_1: { x: 0.0, y: focal_length, z: 0.0, w: 0.0 },
		row_2: { x: 0.0, y: 0.0, z: far / (near - far), w: -1.0 },
		row_3: { x: 0.0, y: 0.0, z: near * far / (near - far), w: 0.0 },
	}
}
