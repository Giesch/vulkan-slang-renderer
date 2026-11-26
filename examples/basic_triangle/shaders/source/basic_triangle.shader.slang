#language slang 2026

module basic_triangle;

import mltrs;

struct Vertex {
    float3 position;
    float3 color;
}

struct TriangleParams {
    mltrs::MVPMatrices mvp;
    mltrs::ImmutableAddr<Vertex> vertices;
}

ParameterBlock<TriangleParams> params;

struct FragInput {
    float4 position : SV_POSITION;
    float3 color;
}

[shader("vertex")]
FragInput vertexMain(uint vertexId : SV_VertexID) {
    let vertex = params.vertices[vertexId];
    let position = params.mvp.project(vertex.position);

    return FragInput(position, vertex.color);
}

[shader("fragment")]
float4 fragmentMain(FragInput fragVertex) {
    return float4(fragVertex.color, 1.0);
}
