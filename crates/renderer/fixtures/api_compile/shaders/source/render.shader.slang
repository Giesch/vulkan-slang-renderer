#language slang 2026

module render;

import mltrs;
import particle;

ParameterBlock<RenderParams> renderParams;

struct RenderParams {
    uint particleCount;
    mltrs::ReadAddr<Particle> particles;
}

struct RenderFragInput {
    float4 color : TEXCOORD0;
    float4 position : SV_Position;
}

[shader("vertex")]
RenderFragInput vertMain(uint id : SV_VertexID) {
    Particle p = renderParams.particles[min(id, renderParams.particleCount - 1)];

    RenderFragInput output;
    output.position = float4(p.position, 0.5, 1.0);
    output.color = float4(p.velocity, 0.0, 1.0);

    return output;
}

[shader("fragment")]
float4 fragMain(RenderFragInput input) : SV_Target {
    return input.color;
}
