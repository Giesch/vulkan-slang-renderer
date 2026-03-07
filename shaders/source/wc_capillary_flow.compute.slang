#language slang 2026

module wc_capillary_flow;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> saturationIn;
    Texture2D<float> wetMaskIn;
    Texture2D<float> pressure;
    Texture2D<float> paperHeight;
    RWTexture2D<float> saturationOut;
    RWTexture2D<float> wetMaskOut;
    float2 gridSize;
    float absorbRate;
    float diffuseRate;
    float capacity;
    float sigma;    // wet mask expansion threshold
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gs = int2(params.gridSize);

    float s = params.saturationIn[pixel];
    let p = params.pressure[pixel];
    let wet = params.wetMaskIn[pixel];

    // (a) Absorb water from pressure
    if (wet > 0.0) {
        s += min(params.absorbRate * max(p, 0.0), params.capacity - s);
    }

    // (b) Diffuse: gather from neighbors (only uphill flow — from higher saturation)
    let sL = params.saturationIn[clamp(pixel + int2(-1, 0), int2(0), gs - 1)];
    let sR = params.saturationIn[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
    let sD = params.saturationIn[clamp(pixel + int2(0, -1), int2(0), gs - 1)];
    let sU = params.saturationIn[clamp(pixel + int2(0, 1), int2(0), gs - 1)];

    let diffusion = params.diffuseRate * (
        max(sL - s, 0.0) + max(sR - s, 0.0) +
        max(sD - s, 0.0) + max(sU - s, 0.0)
    );
    s = min(s + diffusion, params.capacity);

    params.saturationOut[pixel] = s;

    // (c) Expand wet mask where saturation exceeds threshold
    float newWet = wet;
    if (s > params.sigma) {
        newWet = 1.0;
    }
    params.wetMaskOut[pixel] = newWet;
}
