#language slang 2026

module multi_mesh;

import mvp;

ParameterBlock<MultiMeshParams> params;

struct MultiMeshParams {
    MVPMatrices mvp;
    float4 tint;
    // the raster/texture test objects modulate by this; every pre-existing
    // shape binds a solid white image, so their output is unchanged
    Sampler2D texture;
}

struct Vertex {
    float3 position;
    // model-space; rotated into world space below so lighting stays correct
    // while each shape's model matrix animates
    float3 normal;
    // passed through untouched, including values outside [0, 1] — clamping or
    // wrapping here would make the sampler's wrap mode untestable
    float2 uv0;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    let worldNormal = params.mvp.rotateDirection(vertex.normal);
    return FragVertex(position, worldNormal, vertex.uv0);
}

struct FragVertex {
    float4 position : SV_Position;
    float3 normal;
    float2 uv0;
};

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    let lightDir = normalize(float3(0.4, 1.0, 0.6));
    let normal = normalize(fragVertex.normal);
    let lambert = 0.35 + 0.65 * max(dot(normal, lightDir), 0.0);
    let texel = params.texture.Sample(fragVertex.uv0);
    let color = params.tint.rgb * texel.rgb * lambert;

    // the tint's alpha reaches the blend stage, so a BlendMode::Alpha pipeline
    // is translucent and a BlendMode::Opaque one ignores it
    return float4(color, params.tint.a);
}
