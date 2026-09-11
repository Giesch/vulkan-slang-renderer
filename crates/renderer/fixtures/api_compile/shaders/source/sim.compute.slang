#language slang 2026

module sim_compute;

import mltrs;
import particle;

ParameterBlock<SimParams> simParams;

struct SimParams {
    mltrs::ReadAddr<Particle> particlesIn;
    mltrs::Addr<Particle> particlesOut;
    float deltaTime;
}

[numthreads(256, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    uint index = dispatchThreadID.x;

    Particle p = simParams.particlesIn[index];
    p.position += p.velocity * simParams.deltaTime;

    simParams.particlesOut[index] = p;
}
