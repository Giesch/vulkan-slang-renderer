#language slang 2026

module sdf_2d;

ParameterBlock<SDF2DUniform> sdfUniform;

struct SDF2DUniform {
    float time;
}

[shader("vertex")]
FragVertex vertMain(uint vertexIndex : SV_VertexID) {
    let position = float4(0.0, 0.0, 0.0, 1.0);
    return FragVertex(position);
}

struct FragVertex {
    float4 position : SV_Position;
};

[shader("fragment")]
float4 fragMain() : SV_TARGET {
    let color = float4(1.0, 0.0, 1.0, 1.0);
    return color;
}
