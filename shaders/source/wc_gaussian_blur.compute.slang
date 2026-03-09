#language slang 2026

/// Separable Gaussian blur pass (horizontal or vertical, selected by `direction` param).
/// See: https://developer.nvidia.com/gpugems/gpugems3/part-vi-gpu-computing/chapter-40-incremental-computation-gaussian

module wc_gaussian_blur;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> inputTex;
    RWTexture2D<float> outputTex;
    float2 gridSize;
    float2 direction; // (1,0) for horizontal, (0,1) for vertical
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gs = int2(params.gridSize);
    let dir = int2(params.direction);

    float weightSum = 0.0;
    float valueSum = 0.0;

    for (int i = -BLUR_RADIUS; i <= BLUR_RADIUS; i++) {
        let samplePos = clamp(pixel + dir * i, int2(0), gs - 1);
        let w = BLUR_WEIGHTS[abs(i)];
        valueSum += params.inputTex[samplePos] * w;
        weightSum += w;
    }

    params.outputTex[pixel] = valueSum / weightSum;
}
