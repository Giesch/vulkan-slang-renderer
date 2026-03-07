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

    let gs = int2(params.gridSize);

    let pL = params.pressureIn[clamp(pixel + int2(-1, 0), int2(0), gs - 1)];
    let pR = params.pressureIn[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
    let pD = params.pressureIn[clamp(pixel + int2(0, -1), int2(0), gs - 1)];
    let pU = params.pressureIn[clamp(pixel + int2(0, 1), int2(0), gs - 1)];

    let div = params.divergence[pixel];

    params.pressureOut[pixel] = (pL + pR + pD + pU - div) * 0.25;
}
