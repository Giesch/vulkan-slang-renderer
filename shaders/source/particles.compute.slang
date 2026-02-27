#language slang 2026

module particles_compute;

import particle;

ParameterBlock<SimParams> simParams;

struct SimParams {
    StructuredBuffer<Particle> particlesIn;
    RWStructuredBuffer<Particle> particlesOut;
    float deltaTime;
}

[numthreads(256, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    uint index = dispatchThreadID.x;

    Particle p = simParams.particlesIn[index];

    p.position += p.velocity * simParams.deltaTime;

    // Wrap at boundaries [-1, 1]
    if (p.position.x > 1.0) p.position.x -= 2.0;
    if (p.position.x < -1.0) p.position.x += 2.0;
    if (p.position.y > 1.0) p.position.y -= 2.0;
    if (p.position.y < -1.0) p.position.y += 2.0;

    simParams.particlesOut[index] = p;
}
