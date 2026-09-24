import ShaderReflection

## Draw declarations and their typed per-frame packer. Tuple constructors
## compose both in the same order; callers supply values, never node indices.
## A single node consumes its uniform directly. Nested tuples compose graphs
## larger than twelve nodes without changing their per-frame value structure.
RenderGraph(frame) :: { decls : List(Decl), pack : frame -> List(List(U8)) }.{

	## A graphics shader as generated modules expose it under `shader`.
	## `vertex` is a `ShaderReflection.VertexInput` for a shader that reads a
	## vertex struct and `ShaderReflection.NoVertexInput` otherwise, so a
	## pipeline constructor accepts only the shaders it can draw.
	Shader(vertex, uniform) : {
		name : Str,
		vertex_spv : List(U8),
		fragment_spv : List(U8),
		reflection_json : Str,
		reflection : ShaderReflection.GraphicsReflection,
		vertex : vertex,
		uniform : Uniform(uniform),
	}

	## The constant buffer a pipeline writes each frame, as the generated
	## shader module exposes it.
	Uniform(t) : ShaderReflection.UniformBinding(t)

	Mesh : {
		vertex_bytes : List(U8),
		vertex_stride : U32,
		indices : List(U32),
	}

	## A pipeline drawn from its own vertices and indices. `name` is for
	## messages only.
	IndexedPipeline(v, t) : {
		name : Str,
		shader : Shader(ShaderReflection.VertexInput(v), t),
		mesh : Mesh,
	}

	## The shader packs `vertices` with its own vertex layout.
	indexed_pipeline : {
		name : Str,
		shader : Shader(ShaderReflection.VertexInput(vertex), uniform),
		vertices : List(vertex),
		indices : List(U32),
	} -> IndexedPipeline(vertex, uniform)
	indexed_pipeline = |decl| {
		name: decl.name,
		shader: decl.shader,
		mesh: {
			vertex_bytes: decl.shader.vertex.pack(decl.vertices),
			vertex_stride: decl.shader.vertex.stride(),
			indices: decl.indices,
		},
	}

	## A pipeline drawn with a vertex count and no vertex input.
	VertexCountPipeline(t) : {
		name : Str,
		shader : Shader(ShaderReflection.NoVertexInput, t),
	}

	vertex_count_pipeline : {
		name : Str,
		shader : Shader(ShaderReflection.NoVertexInput, t),
	} -> VertexCountPipeline(t)
	vertex_count_pipeline = |decl| {
		name: decl.name,
		shader: decl.shader,
	}

	## A shader's pipeline inputs, with the vertex marker erased.
	ShaderDecl : {
		name : Str,
		vertex_spv : List(U8),
		fragment_spv : List(U8),
		reflection_json : Str,
		reflection : ShaderReflection.GraphicsReflection,
	}

	UniformDecl : { name : Str, index : U32, size : U32 }

	Call : [WholeIndexed, VertexCount(U32)]

	## One draw node: a pipeline of its own and the call that draws it.
	Decl : {
		name : Str,
		shader : ShaderDecl,
		mesh : [Indexed(Mesh), VertexCount],
		uniform : UniformDecl,
		call : Call,
	}

	## Compose 2 blueprints and consume their frame values in tuple order.
	from_tuple_2 : (RenderGraph(a), RenderGraph(b)) -> RenderGraph((a, b))
	from_tuple_2 = |(b0, b1)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls].join(),
				pack: |(v0, v1)| [pack0(v0), pack1(v1)].join(),
			},
		)
	}

	## Compose 3 blueprints and consume their frame values in tuple order.
	from_tuple_3 : (RenderGraph(a), RenderGraph(b), RenderGraph(c)) -> RenderGraph((a, b, c))
	from_tuple_3 = |(b0, b1, b2)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls].join(),
				pack: |(v0, v1, v2)| [pack0(v0), pack1(v1), pack2(v2)].join(),
			},
		)
	}

	## Compose 4 blueprints and consume their frame values in tuple order.
	from_tuple_4 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d)) -> RenderGraph((a, b, c, d))
	from_tuple_4 = |(b0, b1, b2, b3)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls].join(),
				pack: |(v0, v1, v2, v3)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3)].join(),
			},
		)
	}

	## Compose 5 blueprints and consume their frame values in tuple order.
	from_tuple_5 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e)) -> RenderGraph((a, b, c, d, e))
	from_tuple_5 = |(b0, b1, b2, b3, b4)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls].join(),
				pack: |(v0, v1, v2, v3, v4)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4)].join(),
			},
		)
	}

	## Compose 6 blueprints and consume their frame values in tuple order.
	from_tuple_6 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f)) -> RenderGraph((a, b, c, d, e, f))
	from_tuple_6 = |(b0, b1, b2, b3, b4, b5)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5)].join(),
			},
		)
	}

	## Compose 7 blueprints and consume their frame values in tuple order.
	from_tuple_7 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f), RenderGraph(g)) -> RenderGraph((a, b, c, d, e, f, g))
	from_tuple_7 = |(b0, b1, b2, b3, b4, b5, b6)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		pack6 = b6.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls, b6.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5, v6)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5), pack6(v6)].join(),
			},
		)
	}

	## Compose 8 blueprints and consume their frame values in tuple order.
	from_tuple_8 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f), RenderGraph(g), RenderGraph(h)) -> RenderGraph((a, b, c, d, e, f, g, h))
	from_tuple_8 = |(b0, b1, b2, b3, b4, b5, b6, b7)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		pack6 = b6.packer()
		pack7 = b7.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls, b6.decls, b7.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5, v6, v7)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5), pack6(v6), pack7(v7)].join(),
			},
		)
	}

	## Compose 9 blueprints and consume their frame values in tuple order.
	from_tuple_9 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f), RenderGraph(g), RenderGraph(h), RenderGraph(i)) -> RenderGraph((a, b, c, d, e, f, g, h, i))
	from_tuple_9 = |(b0, b1, b2, b3, b4, b5, b6, b7, b8)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		pack6 = b6.packer()
		pack7 = b7.packer()
		pack8 = b8.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls, b6.decls, b7.decls, b8.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5, v6, v7, v8)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5), pack6(v6), pack7(v7), pack8(v8)].join(),
			},
		)
	}

	## Compose 10 blueprints and consume their frame values in tuple order.
	from_tuple_10 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f), RenderGraph(g), RenderGraph(h), RenderGraph(i), RenderGraph(j)) -> RenderGraph((a, b, c, d, e, f, g, h, i, j))
	from_tuple_10 = |(b0, b1, b2, b3, b4, b5, b6, b7, b8, b9)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		pack6 = b6.packer()
		pack7 = b7.packer()
		pack8 = b8.packer()
		pack9 = b9.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls, b6.decls, b7.decls, b8.decls, b9.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5, v6, v7, v8, v9)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5), pack6(v6), pack7(v7), pack8(v8), pack9(v9)].join(),
			},
		)
	}

	## Compose 11 blueprints and consume their frame values in tuple order.
	from_tuple_11 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f), RenderGraph(g), RenderGraph(h), RenderGraph(i), RenderGraph(j), RenderGraph(k)) -> RenderGraph((a, b, c, d, e, f, g, h, i, j, k))
	from_tuple_11 = |(b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		pack6 = b6.packer()
		pack7 = b7.packer()
		pack8 = b8.packer()
		pack9 = b9.packer()
		pack10 = b10.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls, b6.decls, b7.decls, b8.decls, b9.decls, b10.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5, v6, v7, v8, v9, v10)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5), pack6(v6), pack7(v7), pack8(v8), pack9(v9), pack10(v10)].join(),
			},
		)
	}

	## Compose 12 blueprints and consume their frame values in tuple order.
	from_tuple_12 : (RenderGraph(a), RenderGraph(b), RenderGraph(c), RenderGraph(d), RenderGraph(e), RenderGraph(f), RenderGraph(g), RenderGraph(h), RenderGraph(i), RenderGraph(j), RenderGraph(k), RenderGraph(l)) -> RenderGraph((a, b, c, d, e, f, g, h, i, j, k, l))
	from_tuple_12 = |(b0, b1, b2, b3, b4, b5, b6, b7, b8, b9, b10, b11)| {
		pack0 = b0.packer()
		pack1 = b1.packer()
		pack2 = b2.packer()
		pack3 = b3.packer()
		pack4 = b4.packer()
		pack5 = b5.packer()
		pack6 = b6.packer()
		pack7 = b7.packer()
		pack8 = b8.packer()
		pack9 = b9.packer()
		pack10 = b10.packer()
		pack11 = b11.packer()
		RenderGraph.(
			{
				decls: [b0.decls, b1.decls, b2.decls, b3.decls, b4.decls, b5.decls, b6.decls, b7.decls, b8.decls, b9.decls, b10.decls, b11.decls].join(),
				pack: |(v0, v1, v2, v3, v4, v5, v6, v7, v8, v9, v10, v11)| [pack0(v0), pack1(v1), pack2(v2), pack3(v3), pack4(v4), pack5(v5), pack6(v6), pack7(v7), pack8(v8), pack9(v9), pack10(v10), pack11(v11)].join(),
			},
		)
	}

	empty : RenderGraph({})
	empty = RenderGraph.({ decls: [], pack: |_| [] })

	## Draw the pipeline's whole index buffer.
	draw_indexed : IndexedPipeline(v, t) -> RenderGraph(t)
	draw_indexed = |pipeline|
		make_graph({
			name: pipeline.name,
			shader: shader_decl(pipeline.shader),
			mesh: Indexed(pipeline.mesh),
			uniform: pipeline.shader.uniform,
			call: WholeIndexed,
		})

	## Draw `count` vertices with no vertex input.
	draw_vertex_count : VertexCountPipeline(t), U32 -> RenderGraph(t)
	draw_vertex_count = |pipeline, count|
		make_graph({
			name: pipeline.name,
			shader: shader_decl(pipeline.shader),
			mesh: VertexCount,
			uniform: pipeline.shader.uniform,
			call: VertexCount(count),
		})

	shader_decl : Shader(vertex, t) -> ShaderDecl
	shader_decl = |shader| {
		name: shader.name,
		vertex_spv: shader.vertex_spv,
		fragment_spv: shader.fragment_spv,
		reflection_json: shader.reflection_json,
		reflection: shader.reflection,
	}

	decls : RenderGraph(frame) -> List(Decl)
	decls = |rg| rg.decls

	## Extract only the packer so registered graphs do not retain host assets.
	packer : RenderGraph(frame) -> (frame -> List(List(U8)))
	packer = |rg| rg.pack
}

RenderGraphDesc(u) : {
	name : Str,
	shader : ShaderDecl,
	mesh : [Indexed(Mesh), VertexCount],
	uniform : Uniform(u),
	call : Call,
}

make_graph : RenderGraphDesc(t) -> RenderGraph(t)
make_graph = |{ name, shader, mesh, uniform: u, call }| {
	uniform = { name: u.name, index: u.index, size: u.size }

	pack = |value| {
		bytes = (u.to_bytes)(value)
		if bytes.len() != u.size.to_u64() {
			crash "render graph: uniform ${u.name} packed ${bytes.len().to_str()} bytes for a ${u.size.to_str()}-byte uniform"
		}

		[bytes]
	}

	RenderGraph.({ decls: [{ name, shader, mesh, uniform, call }], pack })
}
