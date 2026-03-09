#language slang 2026

module wc_pressure_jacobi;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> pressureIn;
    Texture2D<float> divergence;
    RWTexture2D<float> pressureOut;
    float2 gridSize;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gridSize = int2(params.gridSize);

    let pressureLeft = params.pressureIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
    let pressureRight = params.pressureIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let pressureDown = params.pressureIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
    let pressureUp = params.pressureIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];

    let divergence = params.divergence[pixel];

    params.pressureOut[pixel] = (pressureLeft + pressureRight + pressureDown + pressureUp - divergence) * 0.25;
}
