app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph exposing [IndexedPipeline]
import pf.GraphSet
import pf.ShaderReflection exposing [Float4x4, Float3]

import Generated/BasicTriangle
import Generated/Mltrs

game = Game.new({ init!, draw, graphs: graph })

Ok(graph) = RenderGraph.draw_indexed(pipeline) |> GraphSet.single

init! : {} => Game.Init
init! = |_|
	{ window_title: "Basic Triangle from Roc!" }

draw : Game.Frame -> GraphSet.Submission(_)
draw = |frame| {
	camera = mvp_with_aspect_ratio(frame.aspect_ratio)

	graph.draw({
		matrices: camera,
		time: frame.elapsed,
	})
}

pipeline : IndexedPipeline(_, _)
pipeline = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
	vertices,
	indices: plain_indices,
})

plain_indices : List(U32)
plain_indices = {
	len = vertices.len().to_u32_wrap()
	(0..<len).iter().collect()
}

expect plain_indices == [0, 1, 2, 3, 4, 5]

RGB := { r : U8, g : U8, b : U8 }.{
	to_float3 : RGB -> Float3
	to_float3 = |{ r, g, b }| {
		x: r.to_f32() / 255.0,
		y: g.to_f32() / 255.0,
		z: b.to_f32() / 255.0,
	}
}

vertices : List(BasicTriangle.Vertex)
vertices = {
	_orange = RGB.{ r: 252, g: 144, b: 3 }
	dark_orange = RGB.{ r: 252, g: 94, b: 3 }
	red = RGB.{ r: 252, g: 53, b: 3 }
	yellow = RGB.{ r: 252, g: 198, b: 3 }

	top_tri = {
		left : Float3
		left = { x: -1.0, y: -0.0, z: 0.0 }
		right = { x: 1.0, y: -0.0, z: 0.0 }
		top = { x: 0.0, y: 2.0, z: 0.0 }

		[
			{ position: left, color: dark_orange },
			{ position: right, color: dark_orange },
			{ position: top, color: red },
		]
	}

	left_tri = {
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

	right_tri = {
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

	with_color_as_float3 = |v| { position: v.position, color: v.color.to_float3() }
	triangle_as_vertices = |tri| tri.map(with_color_as_float3)

	[
		top_tri,
		left_tri,
		right_tri,
	].join_map(triangle_as_vertices)
}

# Column-major like glam: `row_i` holds column `i`,
# which the row-major shader multiplies as `position * M`.
mvp_with_aspect_ratio : F32 -> Mltrs.MvpMatrices
mvp_with_aspect_ratio = |aspect| {
	frustrum = { fov_y_degrees: 45.0, aspect, near: 0.1, far: 10.0 }
	projection = perspective(frustrum)

	{ model: Float4x4.identity, view: view_from_z6, proj: projection }
}

## The view from `(0, 0, 6)` towards the origin with `+Y` up.
view_from_z6 : Float4x4
view_from_z6 = {
	row_0: { x: 1.0, y: 0.0, z: 0.0, w: 0.0 },
	row_1: { x: 0.0, y: 1.0, z: 0.0, w: 0.0 },
	row_2: { x: 0.0, y: 0.0, z: 1.0, w: 0.0 },
	row_3: { x: 0.0, y: 0.0, z: -6.0, w: 1.0 },
}

Frustrum : { fov_y_degrees : F32, aspect : F32, near : F32, far : F32 }

## A right-handed perspective projection with a 0..1 depth range.
perspective : Frustrum -> Float4x4
perspective = |{ fov_y_degrees, aspect, near, far }| {
	focal_length = 1.0 / F32.tan(fov_y_degrees * F32.pi / 360.0)

	{
		row_0: { x: focal_length / aspect, y: 0.0, z: 0.0, w: 0.0 },
		row_1: { x: 0.0, y: focal_length, z: 0.0, w: 0.0 },
		row_2: { x: 0.0, y: 0.0, z: far / (near - far), w: -1.0 },
		row_3: { x: 0.0, y: 0.0, z: near * far / (near - far), w: 0.0 },
	}
}
