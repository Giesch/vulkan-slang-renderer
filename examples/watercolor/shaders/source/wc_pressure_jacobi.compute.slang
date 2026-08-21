#language slang 2026

module wc_pressure_jacobi;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Sampler2D<float>.Handle divergence;
    float2 gridSize;
}

// the ping-pong sides swap between the two dispatches of one frame, which no
// once-per-frame uniform write can express
[[vk::push_constant]] ConstantBuffer<JacobiDispatch> jacobiDispatch;

struct JacobiDispatch {
    Sampler2D<float>.Handle pressureIn;
    RWTexture2D<float>.Handle pressureOut;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize)) {
        return;
    }

    let gridSize = int2(params.gridSize);

    let pressureLeft = jacobiDispatch.pressureIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
    let pressureRight = jacobiDispatch.pressureIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let pressureDown = jacobiDispatch.pressureIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
    let pressureUp = jacobiDispatch.pressureIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];

    let divergence = params.divergence[pixel];

    jacobiDispatch.pressureOut[pixel] = (pressureLeft + pressureRight + pressureDown + pressureUp - divergence) * 0.25;
}
