#language slang 2026

module wc_project_velocity;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    RWTexture2D<float> u;
    RWTexture2D<float> v;
    Texture2D<float> pressure;
    Texture2D<float> wetMask;
    Texture2D<float> blurredMask;
    float2 gridSize;
    float outwardStrength;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize)) {
        return;
    }

    let gridSize = int2(params.gridSize);
    let wetMask = params.wetMask[pixel];

    // Full-strength pressure projection: subtract pressure gradient from velocity
    // u at (i+0.5, j): dp/dx = p[i+1,j] - p[i,j]
    let pressureRight = params.pressure[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let pressureCenter = params.pressure[pixel];
    params.u[pixel] = (params.u[pixel] - (pressureRight - pressureCenter)) * wetMask;

    // v at (i, j+0.5): dp/dy = p[i,j+1] - p[i,j]
    let pressureUp = params.pressure[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
    params.v[pixel] = (params.v[pixel] - (pressureUp - pressureCenter)) * wetMask;

    // Inject outward velocity at wet/dry edges after projection.
    // blurredMask gradient points interior→edge (high→low);
    // negate to get outward direction (interior→edge).
    // This divergent flow is corrected next frame, but pigment advection already happened.
    let blurredHere = params.blurredMask[pixel];
    let blurredRight = params.blurredMask[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    params.u[pixel] += params.outwardStrength * (blurredHere - blurredRight) * smoothstep(0.0, 0.1, wetMask);

    let blurredUp = params.blurredMask[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
    params.v[pixel] += params.outwardStrength * (blurredHere - blurredUp) * smoothstep(0.0, 0.1, wetMask);
}
