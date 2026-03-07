#language slang 2026

module wc_update_velocity;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> uIn;
    Texture2D<float> vIn;
    Texture2D<float> pressure;
    Texture2D<float> paperHeight;
    Texture2D<float> wetMask;
    RWTexture2D<float> uOut;
    RWTexture2D<float> vOut;
    float2 gridSize;
    float2 texelSize;
    float dt;
    float mu;
    float kappa;
    float slopeStrength;
}

// Interpolate v at a u-grid location (offset by half in x)
float interpolateV(Texture2D<float> vTex, int2 pixel, float2 gridSize) {
    // v lives at (i, j+0.5), u lives at (i+0.5, j)
    // Average 4 surrounding v values
    int2 gs = int2(gridSize);
    int2 p00 = clamp(pixel, int2(0), gs - 1);
    int2 p10 = clamp(pixel + int2(1, 0), int2(0), gs - 1);
    int2 p01 = clamp(pixel + int2(0, -1), int2(0), gs - 1);
    int2 p11 = clamp(pixel + int2(1, -1), int2(0), gs - 1);
    return 0.25 * (vTex[p00] + vTex[p10] + vTex[p01] + vTex[p11]);
}

// Interpolate u at a v-grid location (offset by half in y)
float interpolateU(Texture2D<float> uTex, int2 pixel, float2 gridSize) {
    int2 gs = int2(gridSize);
    int2 p00 = clamp(pixel, int2(0), gs - 1);
    int2 p10 = clamp(pixel + int2(0, 1), int2(0), gs - 1);
    int2 p01 = clamp(pixel + int2(-1, 0), int2(0), gs - 1);
    int2 p11 = clamp(pixel + int2(-1, 1), int2(0), gs - 1);
    return 0.25 * (uTex[p00] + uTex[p10] + uTex[p01] + uTex[p11]);
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);
    let gs = int2(params.gridSize);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    // --- Update u (horizontal velocity at i+0.5, j) ---
    {
        let uHere = params.uIn[pixel];

        // Semi-Lagrangian backtrace
        let pos = float2(pixel) + float2(0.5, 0.0);
        let vHere = interpolateV(params.vIn, pixel, params.gridSize);
        let backtrace = pos - params.dt * float2(uHere, vHere);
        let uAdvected = bilinearSampleR(params.uIn, backtrace, params.gridSize);

        // Laplacian for diffusion
        let uL = params.uIn[clamp(pixel + int2(-1, 0), int2(0), gs - 1)];
        let uR = params.uIn[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
        let uD = params.uIn[clamp(pixel + int2(0, -1), int2(0), gs - 1)];
        let uU = params.uIn[clamp(pixel + int2(0, 1), int2(0), gs - 1)];
        let uLap = uL + uR + uD + uU - 4.0 * uHere;

        // Pressure gradient (dp/dx)
        let pR = params.pressure[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
        let pL = params.pressure[pixel];
        let dpDx = pR - pL;

        // Paper slope (dh/dx)
        let hR = params.paperHeight[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
        let hL = params.paperHeight[pixel];
        let dhDx = (hR - hL) * params.slopeStrength;

        // Wet mask at this location
        let wet = params.wetMask[pixel];

        let uNew = (uAdvected + params.dt * (params.mu * uLap - dpDx + dhDx - params.kappa * uHere)) * wet;
        params.uOut[pixel] = uNew;
    }

    // --- Update v (vertical velocity at i, j+0.5) ---
    {
        let vHere = params.vIn[pixel];

        let pos = float2(pixel) + float2(0.0, 0.5);
        let uHere = interpolateU(params.uIn, pixel, params.gridSize);
        let backtrace = pos - params.dt * float2(uHere, vHere);
        let vAdvected = bilinearSampleR(params.vIn, backtrace, params.gridSize);

        let vL = params.vIn[clamp(pixel + int2(-1, 0), int2(0), gs - 1)];
        let vR = params.vIn[clamp(pixel + int2(1, 0), int2(0), gs - 1)];
        let vD = params.vIn[clamp(pixel + int2(0, -1), int2(0), gs - 1)];
        let vU = params.vIn[clamp(pixel + int2(0, 1), int2(0), gs - 1)];
        let vLap = vL + vR + vD + vU - 4.0 * vHere;

        let pU = params.pressure[clamp(pixel + int2(0, 1), int2(0), gs - 1)];
        let pD = params.pressure[pixel];
        let dpDy = pU - pD;

        let hU = params.paperHeight[clamp(pixel + int2(0, 1), int2(0), gs - 1)];
        let hD = params.paperHeight[pixel];
        let dhDy = (hU - hD) * params.slopeStrength;

        let wet = params.wetMask[pixel];

        let vNew = (vAdvected + params.dt * (params.mu * vLap - dpDy + dhDy - params.kappa * vHere)) * wet;
        params.vOut[pixel] = vNew;
    }
}
