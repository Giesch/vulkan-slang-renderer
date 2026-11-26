#language slang 2026

module creative_coding;

import mltrs;

struct Vertex {
    float3 position;
    float3 color;
}

struct Params {
    mltrs::MVPMatrices matrices;
    mltrs::ImmutableAddr<Vertex> vertices;
    float time;
}

ParameterBlock<Params> params;

struct FragInput {
    float4 position : SV_POSITION;
    float3 color;
}

[shader("vertex")]
FragInput vertexMain(uint vertexId : SV_VertexID) {
    let vertex = params.vertices[vertexId];
    let position = params.matrices.project(vertex.position);

    return FragInput(position, vertex.color);
}

[shader("fragment")]
float4 fragmentMain(FragInput fragVertex) {
    let frequency = 1.0;
    let wave = sin(params.time * frequency) * 0.5 + 0.5;
    let magenta = float3(1.0, 0.0, 1.0);

    return float4(fragVertex.color + wave * magenta, 1.0);
}
