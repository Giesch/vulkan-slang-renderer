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
    let gridSize = int2(params.gridSize);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    // --- Update u (horizontal velocity at i+0.5, j) ---
    {
        let uHere = params.uIn[pixel];

        // Semi-Lagrangian backtrace
        let position = float2(pixel) + float2(0.5, 0.0);
        let vHere = interpolateV(params.vIn, pixel, params.gridSize);
        let backtrace = position - params.dt * float2(uHere, vHere);
        let uAdvected = bilinearSampleR(params.uIn, backtrace, params.gridSize);

        // Laplacian for diffusion
        let uLeft = params.uIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
        let uRight = params.uIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
        let uDown = params.uIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
        let uUp = params.uIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
        let uLaplacian = uLeft + uRight + uDown + uUp - 4.0 * uHere;

        // Pressure gradient (dp/dx)
        let pressureRight = params.pressure[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
        let pressureLeft = params.pressure[pixel];
        let pressureGradientX = pressureRight - pressureLeft;

        // Paper slope (dh/dx)
        let heightRight = params.paperHeight[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
        let heightLeft = params.paperHeight[pixel];
        let heightGradientX = (heightRight - heightLeft) * params.slopeStrength;

        // Wet mask at this location
        let wetMask = params.wetMask[pixel];

        let updatedU = (uAdvected + params.dt * (params.mu * uLaplacian - pressureGradientX + heightGradientX - params.kappa * uHere)) * wetMask;
        params.uOut[pixel] = updatedU;
    }

    // --- Update v (vertical velocity at i, j+0.5) ---
    {
        let vHere = params.vIn[pixel];

        let position = float2(pixel) + float2(0.0, 0.5);
        let uHere = interpolateU(params.uIn, pixel, params.gridSize);
        let backtrace = position - params.dt * float2(uHere, vHere);
        let vAdvected = bilinearSampleR(params.vIn, backtrace, params.gridSize);

        let vLeft = params.vIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
        let vRight = params.vIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
        let vDown = params.vIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
        let vUp = params.vIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
        let vLaplacian = vLeft + vRight + vDown + vUp - 4.0 * vHere;

        let pressureUp = params.pressure[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
        let pressureDown = params.pressure[pixel];
        let pressureGradientY = pressureUp - pressureDown;

        let heightUp = params.paperHeight[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
        let heightDown = params.paperHeight[pixel];
        let heightGradientY = (heightUp - heightDown) * params.slopeStrength;

        let wetMask = params.wetMask[pixel];

        let updatedV = (vAdvected + params.dt * (params.mu * vLaplacian - pressureGradientY + heightGradientY - params.kappa * vHere)) * wetMask;
        params.vOut[pixel] = updatedV;
    }
}
