#language slang 2026

module roc_shared;

import mltrs;
import particle;

ParameterBlock<RocSharedParams> rocSharedParams;

struct RocSharedParams {
    mltrs::ReadAddr<Particle> particles;
}

struct RocSharedFragInput {
    float4 color : TEXCOORD0;
    float4 position : SV_Position;
}

[shader("vertex")]
RocSharedFragInput vertexMain(uint id : SV_VertexID) {
    Particle p = rocSharedParams.particles[id];
    RocSharedFragInput output;
    output.position = float4(p.position, 0.5, 1.0);
    output.color = p.color;
    return output;
}

[shader("fragment")]
float4 fragmentMain(RocSharedFragInput input) : SV_Target {
    return input.color;
}
