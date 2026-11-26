app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner
import pf.ShaderReflection exposing [Float3, Float4x4]

import Generated/CreativeCoding
import Generated/Mltrs
import Generated/ShaderAtlas

game = Game.new({ init!, graphs: graph, draw })

## Shader edits reload live; edits to this file need a restart.
init! : {} => Game.Init
init! = |_| {
	window_title: "Creative Coding from Roc!",
	shader_hot_reload: Bool.True,
	shaders_source_dir: ShaderAtlas.shaders_source_dir,
}

draw : Game.Frame -> GraphSet.Submission(_)
draw = |frame| {
	matrices = mvp_with_aspect_ratio(frame.aspect_ratio)

	graph.draw({ matrices, time: frame.elapsed })
}

(planner1, vertex_buffer) = ResourcePlanner.singleton_buffer(
	ResourcePlanner.empty,
	"vertices",
	CreativeCoding.Vertex.packer,
	vertices,
)

(planner2, indices) =
	ResourcePlanner.index_buffer(planner1, "indices", plain_indices, vertex_count)

(planner3, params) =
	ResourcePlanner.uniform_buffer(planner2, "params", CreativeCoding.params)

(planner4, pipeline) = ResourcePlanner.graphics_pipeline(
	planner3,
	{
		name: "creative_coding",
		shader: CreativeCoding.shader,
		uniforms: { params: params },
		raster: RasterState.default,
	},
)

bindings = { vertices: vertex_buffer.addr() }

node = RenderGraph.draw_indexed(pipeline, indices, params, bindings)

Ok(graph) = RenderGraph.new(planner4, node).map_ok(GraphSet.single)

vertex_count : U32
vertex_count = vertices.len().to_u32_wrap()

## Each vertex once, in order.
plain_indices : List(U32)
plain_indices = (0..<vertex_count).iter().collect()

expect plain_indices == [0, 1, 2, 3, 4, 5, 6, 7, 8]

Rgb := { r : U8, g : U8, b : U8 }.{
	to_float3 : Rgb -> Float3
	to_float3 = |{ r, g, b }| {
		x: r.to_f32() / 255.0,
		y: g.to_f32() / 255.0,
		z: b.to_f32() / 255.0,
	}
}

vertices : List(CreativeCoding.Vertex)
vertices = {
	dark_orange = Rgb.{ r: 252, g: 94, b: 3 }
	red = Rgb.{ r: 252, g: 53, b: 3 }
	yellow = Rgb.{ r: 252, g: 198, b: 3 }

	top_triangle = {
		left : Float3
		left = { x: -1.0, y: 0.0, z: 0.0 }
		right = { x: 1.0, y: 0.0, z: 0.0 }
		top = { x: 0.0, y: 2.0, z: 0.0 }

		[
			{ position: left, color: dark_orange },
			{ position: right, color: dark_orange },
			{ position: top, color: red },
		]
	}

	left_triangle = {
		left : Float3
		left = { x: -2.0, y: -2.0, z: 0.0 }
		right = { x: 0.0, y: -2.0, z: 0.0 }
		top = { x: -1.0, y: 0.0, z: 0.0 }

		[
			{ position: left, color: dark_orange },
			{ position: right, color: yellow },
			{ position: top, color: dark_orange },
		]
	}

	right_triangle = {
		left : Float3
		left = { x: 0.0, y: -2.0, z: 0.0 }
		right = { x: 2.0, y: -2.0, z: 0.0 }
		top = { x: 1.0, y: 0.0, z: 0.0 }

		[
			{ position: left, color: yellow },
			{ position: right, color: yellow },
			{ position: top, color: dark_orange },
		]
	}

	with_float3_color = |vertex| {
		position: vertex.position,
		color: vertex.color.to_float3(),
	}
	triangles = [top_triangle, left_triangle, right_triangle]

	triangles.join_map(|triangle| triangle.map(with_float3_color))
}

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
