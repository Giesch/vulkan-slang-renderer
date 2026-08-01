#language slang 2026

module wc_capillary_flow;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> saturationIn;
    Texture2D<float> wetMaskIn;
    Texture2D<float> paperHeight;
    RWTexture2D<float> saturationOut;
    RWTexture2D<float> wetMaskOut;
    float2 gridSize;
    float diffuseRate;
    float capacity;
    float sigma;    // wet mask expansion threshold
    float dryThreshold;    // saturation below which cell becomes dry
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize)) {
        return;
    }

    let gridSize = int2(params.gridSize);

    float saturation = params.saturationIn[pixel];
    let wetMask = params.wetMaskIn[pixel];

    // (b) Diffuse: gather from wet neighbors only (uphill flow)
    let saturationLeft = params.saturationIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
    let saturationRight = params.saturationIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let saturationDown = params.saturationIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
    let saturationUp = params.saturationIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];

    let wetLeft = params.wetMaskIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
    let wetRight = params.wetMaskIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let wetDown = params.wetMaskIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
    let wetUp = params.wetMaskIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];

    let diffusion = params.diffuseRate * (
        max(saturationLeft - saturation, 0.0) * step(0.001, wetLeft) + max(saturationRight - saturation, 0.0) * step(0.001, wetRight) +
        max(saturationDown - saturation, 0.0) * step(0.001, wetDown) + max(saturationUp - saturation, 0.0) * step(0.001, wetUp)
    );
    saturation = min(saturation + diffusion, params.capacity);

    params.saturationOut[pixel] = saturation;

    // (c) Expand wet mask where saturation exceeds threshold
    float newWet = wetMask;
    if (saturation > params.sigma) {
        newWet = 1.0;
    } else if (saturation < params.dryThreshold) {
        newWet = 0.0;
    }
    params.wetMaskOut[pixel] = newWet;
}
