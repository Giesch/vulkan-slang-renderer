#language slang 2026

module suzanne;

import mvp;

ParameterBlock<SuzanneParams> params;

struct SuzanneParams {
    MVPMatrices mvp;
    float time;
    Sampler2D texture0;
    Sampler2D texture1;
    Sampler2D texture2;
}

struct Vertex {
    float3 position;
    float3 color;
    float2 texCoord;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
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
    // cycle between the three textures over time;
    // this is uniform control flow, so implicit-LOD Sample is safe
    let index = int(params.time / 2.0) % 3;

    var textureColor: float4;
    if (index == 0) {
        textureColor = params.texture0.Sample(fragVertex.texCoord);
    } else if (index == 1) {
        textureColor = params.texture1.Sample(fragVertex.texCoord);
    } else {
        textureColor = params.texture2.Sample(fragVertex.texCoord);
    }

    return float4(fragVertex.color * textureColor.rgb, 1.0);
}
