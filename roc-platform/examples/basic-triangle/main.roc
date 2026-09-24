app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.RenderGraph exposing [IndexedPipeline]
import pf.Graphs
import pf.ShaderReflection exposing [Float4x4]

import Generated/BasicTriangle
import Generated/Mltrs

game = Game.new({ init!, draw, graphs: graph })

Ok(graph) = RenderGraph.draw_indexed(pipeline)
	|> Graphs.single

init! : {} => Game.Init
init! = |_|
	{ window_title: "Basic Triangle from Roc!" }

draw : Game.Frame -> Graphs.Submission(_)
draw = |frame| {
	camera = mvp_with_aspect_ratio(frame.aspect_ratio)

	graph.draw(camera)
}

pipeline : IndexedPipeline(_, _)
pipeline = RenderGraph.indexed_pipeline({
	name: "basic_triangle",
	shader: BasicTriangle.shader,
	vertices,
	indices: [0, 1, 2],
})

vertices : List(BasicTriangle.Vertex)
vertices = {
	red = { x: 1.0, y: 0.0, z: 0.0 }
	blue = { x: 0.0, y: 1.0, z: 0.0 }
	green = { x: 0.0, y: 0.0, z: 1.0 }

	left = { x: -1.0, y: -1.0, z: 0.0 }
	top = { x: 1.0, y: -1.0, z: 0.0 }
	right = { x: 0.0, y: 1.0, z: 0.0 }

	[
		{ position: left, color: red },
		{ position: top, color: blue },
		{ position: right, color: green },
	]
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
