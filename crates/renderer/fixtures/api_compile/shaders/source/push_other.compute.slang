#language slang 2026

module push_other_compute;

import mltrs;
import particle;

ParameterBlock<OtherParams> otherParams;

struct OtherParams {
    mltrs::Addr<Particle> particles;
    float scale;
}

// a second push block, so the fixture can type-check that a push-constant
// pipeline only accepts its own push type, not any push type
[[vk::push_constant]] ConstantBuffer<OtherPush> otherPush;

struct OtherPush {
    mltrs::Addr<Particle> particles;
    float factor;
}

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    uint index = dispatchThreadID.x;

    Particle p = otherParams.particles[index];
    p.velocity = p.velocity * (otherParams.scale * otherPush.factor);

    otherParams.particles[index] = p;
}
