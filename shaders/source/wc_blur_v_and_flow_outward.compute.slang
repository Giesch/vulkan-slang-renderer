#language slang 2026

/// Fused vertical Gaussian blur + flow outward pass.

module wc_blur_v_and_flow_outward;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> inputTex;   // blur_temp (horizontally blurred wet_mask)
    Texture2D<float> wetMask;
    RWTexture2D<float> pressure;
    float2 gridSize;
    float eta;
}

static const int RADIUS = 10;

// Precomputed weights for exp(-float(i * i) / (2.0 * SIGMA * SIGMA)) where SIGMA = 3.333
static const float WEIGHTS[11] = {
    1.0, 0.955997, 0.835214, 0.667013, 0.486752,
    0.324652, 0.197899, 0.110251, 0.056149, 0.026133, 0.011109
};

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gs = int2(params.gridSize);

    // Vertical Gaussian blur (reading from horizontally-blurred blur_temp)
    float weightSum = 0.0;
    float valueSum = 0.0;

    for (int i = -RADIUS; i <= RADIUS; i++) {
        let samplePos = clamp(pixel + int2(0, i), int2(0), gs - 1);
        let w = WEIGHTS[abs(i)];
        valueSum += params.inputTex[samplePos] * w;
        weightSum += w;
    }

    let blurred = valueSum / weightSum;

    // Flow outward only reads blurred_mask at the same texel (no neighbor access),
    // so we compute the vertical blur inline and use the result directly —
    // eliminating the blurred_mask intermediate texture, 1 dispatch, and 1 barrier.
    let wet = params.wetMask[pixel];
    params.pressure[pixel] += -params.eta * (1.0 - blurred) * wet;
}
