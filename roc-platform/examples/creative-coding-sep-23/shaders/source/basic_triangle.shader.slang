#language slang 2026

module basic_triangle;

import mltrs;

ParameterBlock<Params> params;

struct Params {
    mltrs::MVPMatrices matrices;
    float time;
}

struct Vertex {
    float3 position;
    float3 color;
};

struct FragInput {
    float4 position : SV_POSITION;
    float3 color;
};

[shader("vertex")]
FragInput vertexMain(Vertex vertex) {
    let position = params.matrices.project(vertex.position);
    return FragInput(position, vertex.color);
}

[shader("fragment")]
float4 fragmentMain(FragInput vert) {
    let frequency = 1.0;
    let wave = sin(params.time * frequency) * 0.5 + 0.5;

    let debug = float3(1.0, 0.0, 1.0);

    return float4(vert.color + (wave * debug), 1.0);

    // return float4(debug, 1.0);
    // return float4(fragVertex.color, 1.0);
}
