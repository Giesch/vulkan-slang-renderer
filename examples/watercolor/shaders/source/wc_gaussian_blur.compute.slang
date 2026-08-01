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

static const int BLUR_RADIUS = 10;

// Precomputed weights for exp(-float(i * i) / (2.0 * SIGMA * SIGMA)) where SIGMA = 3.333
static const float BLUR_WEIGHTS[11] = {
    1.0, 0.955997, 0.835214, 0.667013, 0.486752,
    0.324652, 0.197899, 0.110251, 0.056149, 0.026133, 0.011109
};

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize)) {
        return;
    }

    let gridSize = int2(params.gridSize);
    let direction = int2(params.direction);

    float weightSum = 0.0;
    float valueSum = 0.0;

    for (int i = -BLUR_RADIUS; i <= BLUR_RADIUS; i++) {
        let samplePos = clamp(pixel + direction * i, int2(0), gridSize - 1);
        let weight = BLUR_WEIGHTS[abs(i)];
        valueSum += params.inputTex[samplePos] * weight;
        weightSum += weight;
    }

    params.outputTex[pixel] = valueSum / weightSum;
}
