#language slang 2026

/// Flow outward pass: reduce pressure and drain saturation at wet-area edges,
/// detected via the Gaussian-blurred wet mask (fluid removal per Curtis et al.).

module wc_flow_outward;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> blurredMask;   // fully-blurred wet mask
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

    let blurred = params.blurredMask[pixel];
    let wetMask = params.wetMask[pixel];
    let flowTerm = params.eta * (1.0 - blurred) * wetMask;
    params.pressure[pixel] -= flowTerm;

    params.saturation[pixel] = max(params.saturation[pixel] - flowTerm, 0.0);
}
