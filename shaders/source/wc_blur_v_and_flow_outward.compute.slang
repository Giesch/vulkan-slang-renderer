#language slang 2026

/// Fused vertical Gaussian blur + flow outward pass.

module wc_blur_v_and_flow_outward;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> inputTex;   // blur_temp (horizontally blurred wet_mask)
    Texture2D<float> wetMask;
    RWTexture2D<float> pressure;
    RWTexture2D<float> saturation;
    float2 gridSize;
    float eta;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize)) {
        return;
    }

    let gridSize = int2(params.gridSize);

    // Vertical Gaussian blur (reading from horizontally-blurred blur_temp)
    float weightSum = 0.0;
    float valueSum = 0.0;

    for (int i = -BLUR_RADIUS; i <= BLUR_RADIUS; i++) {
        let samplePos = clamp(pixel + int2(0, i), int2(0), gridSize - 1);
        let weight = BLUR_WEIGHTS[abs(i)];
        valueSum += params.inputTex[samplePos] * weight;
        weightSum += weight;
    }

    let blurred = valueSum / weightSum;

    // Flow outward only reads blurred_mask at the same texel (no neighbor access),
    // so we compute the vertical blur inline and use the result directly —
    // eliminating the blurred_mask intermediate texture, 1 dispatch, and 1 barrier.
    let wetMask = params.wetMask[pixel];
    let flowTerm = params.eta * (1.0 - blurred) * wetMask;
    params.pressure[pixel] -= flowTerm;

    // Drain saturation at wet edges (fluid removal per Curtis et al.)
    params.saturation[pixel] = max(params.saturation[pixel] - flowTerm, 0.0);
}
