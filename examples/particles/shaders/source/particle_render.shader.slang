#language slang 2026

module particle_render;

import mltrs;
import particle;

ParameterBlock<RenderParams> renderParams;

struct RenderParams {
    uint particleCount;
    mltrs::ReadAddr<Particle> particles;
}

struct FragInput {
    float4 color : TEXCOORD0;
    float4 position : SV_Position;
}

// 6 vertices per particle (2 triangles forming a quad)
// counter-clockwise in Y-up clip space
static const float2 quadOffsets[6] = {
    {-1.0, -1.0}, { 1.0, -1.0}, {-1.0,  1.0},
    { 1.0, -1.0}, { 1.0,  1.0}, {-1.0,  1.0},
};

static const float PARTICLE_SIZE = 0.008;

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    uint particleIndex = id / 6;
    uint vertexIndex = id % 6;

    Particle p = renderParams.particles[particleIndex];
    float2 offset = quadOffsets[vertexIndex] * PARTICLE_SIZE;

    FragInput output;
    output.position = float4(p.position + offset, 0.5, 1.0);
    output.color = p.color;

    return output;
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    return input.color;
}
