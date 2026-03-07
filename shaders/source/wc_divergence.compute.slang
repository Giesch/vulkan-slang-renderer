#language slang 2026

module wc_divergence;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> uIn;
    Texture2D<float> vIn;
    RWTexture2D<float> divergence;
    float2 gridSize;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gs = int2(params.gridSize);

    // u is at (i+0.5, j): du/dx = u[i,j] - u[i-1,j]
    let uRight = params.uIn[pixel];
    let uLeft = params.uIn[clamp(pixel + int2(-1, 0), int2(0), gs - 1)];

    // v is at (i, j+0.5): dv/dy = v[i,j] - v[i,j-1]
    let vTop = params.vIn[pixel];
    let vBottom = params.vIn[clamp(pixel + int2(0, -1), int2(0), gs - 1)];

    params.divergence[pixel] = (uRight - uLeft) + (vTop - vBottom);
}
