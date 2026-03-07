#language slang 2026

module wc_move_pigment;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float4> pigmentIn;
    Texture2D<float> uIn;
    Texture2D<float> vIn;
    Texture2D<float> wetMask;
    RWTexture2D<float4> pigmentOut;
    float2 gridSize;
    float dt;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gs = int2(params.gridSize);
    let wet = params.wetMask[pixel];

    if (wet < 0.001) {
        params.pigmentOut[pixel] = params.pigmentIn[pixel];
        return;
    }

    // Cell-center velocity by averaging staggered components
    let uL = params.uIn[clamp(pixel + int2(-1, 0), int2(0), gs - 1)];
    let uR = params.uIn[pixel];
    let vD = params.vIn[clamp(pixel + int2(0, -1), int2(0), gs - 1)];
    let vU = params.vIn[pixel];
    let vel = float2((uL + uR) * 0.5, (vD + vU) * 0.5);

    // Semi-Lagrangian backtrace
    let pos = float2(pixel) + 0.5;
    let backtrace = pos - params.dt * vel;

    let advected = bilinearSampleRGBA(params.pigmentIn, backtrace, params.gridSize);
    params.pigmentOut[pixel] = max(advected, float4(0.0));
}
