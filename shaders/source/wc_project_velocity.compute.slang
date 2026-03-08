#language slang 2026

module wc_project_velocity;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    RWTexture2D<float> u;
    RWTexture2D<float> v;
    Texture2D<float> pressure;
    Texture2D<float> wetMask;
    float2 gridSize;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let gs = int2(params.gridSize);
    let wet = params.wetMask[pixel];

    // Subtract pressure gradient from velocity
    // u at (i+0.5, j): dp/dx = p[i+1,j] - p[i,j]
    let pR = params.pressure[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
    let pC = params.pressure[pixel];
    params.u[pixel] = (params.u[pixel] - (pR - pC)) * wet;

    // v at (i, j+0.5): dp/dy = p[i,j+1] - p[i,j]
    let pU = params.pressure[clamp(pixel + int2(0, 1), int2(0), gs - 1)];
    params.v[pixel] = (params.v[pixel] - (pU - pC)) * wet;
}
