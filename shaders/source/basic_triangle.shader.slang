#language slang 2026

module basic_triangle;

import mvp;

ParameterBlock<MVPMatrices> matrices;

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
    let position = matrices.project(vertex.position);
    return FragInput(position, vertex.color);
}

[shader("fragment")]
float4 fragmentMain(FragInput fragVertex) {
    return float4(fragVertex.color, 1.0);
}
