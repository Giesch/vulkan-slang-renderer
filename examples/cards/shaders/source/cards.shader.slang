#language slang 2026

module cards;

import mltrs;

ParameterBlock<CardViewerParams> params;

struct CardViewerParams {
    mltrs::MVPMatrices mvp;
    Sampler2D.Handle texture;
}

struct Vertex {
    float3 position;
    float3 color;
    float2 texCoord;
};

[shader("vertex")]
FragVertex vertMain(uint vertexIndex : SV_VertexID, Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    return FragVertex(position, vertex.color, vertex.texCoord);
}

struct FragVertex {
    float4 position : SV_Position;
    float3 color;
    float2 texCoord;
};

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    return params.texture.Sample(fragVertex.texCoord);
}
