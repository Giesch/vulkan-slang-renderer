#language slang 2026

module wc_gaussian_blur;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> inputTex;
    RWTexture2D<float> outputTex;
    float2 gridSize;
    float2 direction; // (1,0) for horizontal, (0,1) for vertical
}

static const int RADIUS = 10;
static const float SIGMA = 3.333; // radius / 3

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

    for (int i = -RADIUS; i <= RADIUS; i++) {
        let samplePos = clamp(pixel + dir * i, int2(0), gs - 1);
        let w = exp(-float(i * i) / (2.0 * SIGMA * SIGMA));
        valueSum += params.inputTex[samplePos] * w;
        weightSum += w;
    }

    params.outputTex[pixel] = valueSum / weightSum;
}
