#language slang 2026

module wc_flow_outward;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> blurredMask;
    Texture2D<float> wetMask;
    RWTexture2D<float> pressure;
    float2 gridSize;
    float eta;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let wet = params.wetMask[pixel];
    let blurred = params.blurredMask[pixel];

    // Add negative pressure at edges (where blurred < 1 but wet == 1)
    // This drives flow outward toward the edges
    params.pressure[pixel] += -params.eta * (1.0 - blurred) * wet;
}
