# Not an example: tuple composition preserves declaration and value order for
# every arity. `roc test` runs its expectations; `roc check` must accept it.
app [game] { pf: platform "../../platform/main.roc" }

import pf.Game
import pf.GraphSet
import pf.RasterState
import pf.RenderGraph
import pf.ResourcePlanner
import pf.ShaderReflection exposing [Float4x4]

import Generated/BasicTriangle

game : Game
game = Game.new({ init!, graphs, draw })

init! : {} => Game.Init
init! = |_| Game.Init.titled("tuple render graphs")

draw = |_frame| graphs.draw((filled(1.0), [filled(2.0), filled(3.0)]))

origin = { x: 0.0, y: 0.0, z: 0.0 }

vertex = { position: origin, color: origin }

(planner1, vertices) = ResourcePlanner.singleton_buffer(
	ResourcePlanner.empty,
	"vertices",
	BasicTriangle.Vertex.packer,
	[vertex, vertex, vertex],
)

(planner2, indices) = ResourcePlanner.index_buffer(planner1, "indices", [0, 1, 2], 3)

## Declares a uniform buffer and a pipeline that reads it, and draws the
## triangle with them, so each draw takes its own frame value.
Params : BasicTriangle.TriangleParamsData

draw_triangle : ResourcePlanner, Str -> (ResourcePlanner, RenderGraph(Params))
draw_triangle = |planner, name| {
	(with_params, params) =
		ResourcePlanner.uniform_buffer(planner, name, BasicTriangle.params)
	(with_pipeline, pipeline) = ResourcePlanner.graphics_pipeline(
		with_params,
		{
			name,
			shader: BasicTriangle.shader,
			uniforms: { params: params },
			raster: RasterState.default,
		},
	)
	bindings = { vertices: vertices.addr() }

	(with_pipeline, RenderGraph.draw_indexed(pipeline, indices, params, bindings))
}

(planner_1, node_1) = draw_triangle(planner2, "node_1")

(planner_2, node_2) = draw_triangle(planner_1, "node_2")

(planner_3, node_3) = draw_triangle(planner_2, "node_3")

(planner_4, node_4) = draw_triangle(planner_3, "node_4")

(planner_5, node_5) = draw_triangle(planner_4, "node_5")

(planner_6, node_6) = draw_triangle(planner_5, "node_6")

(planner_7, node_7) = draw_triangle(planner_6, "node_7")

(planner_8, node_8) = draw_triangle(planner_7, "node_8")

(planner_9, node_9) = draw_triangle(planner_8, "node_9")

(planner_10, node_10) = draw_triangle(planner_9, "node_10")

(planner_11, node_11) = draw_triangle(planner_10, "node_11")

(planner_12, node_12) = draw_triangle(planner_11, "node_12")

planner = planner_12

## Mixed frame types: one draw's data, and a list of draws' data.
mixed = RenderGraph.from_tuple_2((node_1, RenderGraph.list([node_2, node_3])))

Ok(graphs) = RenderGraph.new(planner, mixed).map_ok(GraphSet.single)

filled : F32 -> BasicTriangle.TriangleParamsData
filled = |value| {
	matrix : Float4x4
	matrix = {
		row_0: { x: value, y: value, z: value, w: value },
		row_1: { x: value, y: value, z: value, w: value },
		row_2: { x: value, y: value, z: value, w: value },
		row_3: { x: value, y: value, z: value, w: value },
	}

	{ mvp: { model: matrix, view: matrix, proj: matrix } }
}

## The frame values of a set built from `graph`, drawn with `frame`.
values_of = |graph, frame|
	match RenderGraph.new(planner, graph).map_ok(GraphSet.single) {
		Ok(set) => set.draw(frame).to_host().values
		Err(_) => []
	}

## `count` draws' values, filled with 1.0 through `count`.
expected : U64 -> List(_)
expected = |count|
	(1..=count).iter().map(|i| Bytes(filled(i.to_f32()).to_bytes())).collect()

expect
	values_of(
		RenderGraph.from_tuple_2((node_1, node_2)),
		(filled(1.0), filled(2.0)),
	)
		== expected(2)

expect
	values_of(
		RenderGraph.from_tuple_3((node_1, node_2, node_3)),
		(filled(1.0), filled(2.0), filled(3.0)),
	)
		== expected(3)

expect
	values_of(
		RenderGraph.from_tuple_4((node_1, node_2, node_3, node_4)),
		(filled(1.0), filled(2.0), filled(3.0), filled(4.0)),
	)
		== expected(4)

expect
	values_of(
		RenderGraph.from_tuple_5((node_1, node_2, node_3, node_4, node_5)),
		(filled(1.0), filled(2.0), filled(3.0), filled(4.0), filled(5.0)),
	)
		== expected(5)

expect
	values_of(
		RenderGraph.from_tuple_6((node_1, node_2, node_3, node_4, node_5, node_6)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
		),
	)
		== expected(6)

expect
	values_of(
		RenderGraph.from_tuple_7((
			node_1,
			node_2,
			node_3,
			node_4,
			node_5,
			node_6,
			node_7,
		)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
			filled(7.0),
		),
	)
		== expected(7)

expect
	values_of(
		RenderGraph.from_tuple_8((
			node_1,
			node_2,
			node_3,
			node_4,
			node_5,
			node_6,
			node_7,
			node_8,
		)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
			filled(7.0),
			filled(8.0),
		),
	)
		== expected(8)

expect
	values_of(
		RenderGraph.from_tuple_9((
			node_1,
			node_2,
			node_3,
			node_4,
			node_5,
			node_6,
			node_7,
			node_8,
			node_9,
		)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
			filled(7.0),
			filled(8.0),
			filled(9.0),
		),
	)
		== expected(9)

expect
	values_of(
		RenderGraph.from_tuple_10((
			node_1,
			node_2,
			node_3,
			node_4,
			node_5,
			node_6,
			node_7,
			node_8,
			node_9,
			node_10,
		)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
			filled(7.0),
			filled(8.0),
			filled(9.0),
			filled(10.0),
		),
	)
		== expected(10)

expect
	values_of(
		RenderGraph.from_tuple_11((
			node_1,
			node_2,
			node_3,
			node_4,
			node_5,
			node_6,
			node_7,
			node_8,
			node_9,
			node_10,
			node_11,
		)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
			filled(7.0),
			filled(8.0),
			filled(9.0),
			filled(10.0),
			filled(11.0),
		),
	)
		== expected(11)

expect
	values_of(
		RenderGraph.from_tuple_12((
			node_1,
			node_2,
			node_3,
			node_4,
			node_5,
			node_6,
			node_7,
			node_8,
			node_9,
			node_10,
			node_11,
			node_12,
		)),
		(
			filled(1.0),
			filled(2.0),
			filled(3.0),
			filled(4.0),
			filled(5.0),
			filled(6.0),
			filled(7.0),
			filled(8.0),
			filled(9.0),
			filled(10.0),
			filled(11.0),
			filled(12.0),
		),
	)
		== expected(12)

## Nested tuples keep depth-first order.
expect
	values_of(
		RenderGraph.from_tuple_2((
			node_1,
			RenderGraph.from_tuple_2((node_2, node_3)),
		)),
		(filled(1.0), (filled(2.0), filled(3.0))),
	)
		== expected(3)

## A list position supplies its draws' values in order.
expect
	graphs.draw((filled(1.0), [filled(2.0), filled(3.0)])).to_host().values
		== expected(3)
