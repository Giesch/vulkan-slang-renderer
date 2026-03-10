#language slang 2026

module wc_capillary_flow;

import watercolor_common;

ParameterBlock<Params> params;

struct Params {
    Texture2D<float> saturationIn;
    Texture2D<float> wetMaskIn;
    Texture2D<float> paperHeight;
    RWTexture2D<float> saturationOut;
    RWTexture2D<float> wetMaskOut;
    float2 gridSize;
    float diffuseRate;
    float capacity;
    float sigma;    // wet mask expansion threshold
    float dryThreshold;    // saturation below which cell becomes dry
    uint frameIndex;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize)) {
        return;
    }

    let gridSize = int2(params.gridSize);

    float saturation = params.saturationIn[pixel];
    let wetMask = params.wetMaskIn[pixel];

    // (b) Diffuse: gather from wet neighbors only (uphill flow)
    let saturationLeft = params.saturationIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
    let saturationRight = params.saturationIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let saturationDown = params.saturationIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
    let saturationUp = params.saturationIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];

    let wetLeft = params.wetMaskIn[clamp(pixel + int2(-1, 0), int2(0), gridSize - 1)];
    let wetRight = params.wetMaskIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
    let wetDown = params.wetMaskIn[clamp(pixel + int2(0, -1), int2(0), gridSize - 1)];
    let wetUp = params.wetMaskIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];

    let diffusion = params.diffuseRate * (
        max(saturationLeft - saturation, 0.0) * step(0.001, wetLeft) + max(saturationRight - saturation, 0.0) * step(0.001, wetRight) +
        max(saturationDown - saturation, 0.0) * step(0.001, wetDown) + max(saturationUp - saturation, 0.0) * step(0.001, wetUp)
    );
    saturation = min(saturation + diffusion, params.capacity);

    params.saturationOut[pixel] = saturation;

    // (c) Expand wet mask: MSAA + noise approach for smooth edges
    // MSAA base: 4-tap supersampled threshold
    float subpixelWet = 0.0;
    float2 offsets[4] = { float2(-0.25, -0.25), float2(0.25, -0.25),
                          float2(-0.25, 0.25),  float2(0.25, 0.25) };
    for (int s = 0; s < 4; s++) {
        float subSat = bilinearSampleR(params.saturationIn, float2(pixel) + 0.5 + offsets[s], params.gridSize);
        if (subSat > params.sigma) {
            subpixelWet += 1.0;
        } else if (subSat < params.dryThreshold) {
            // contributes 0.0
        } else {
            subpixelWet += wetMask;
        }
    }
    float msaaMask = subpixelWet * 0.25;

    // Paper noise overlay: perturb the smooth MSAA result with paper texture
    let paperNoise = params.paperHeight[pixel];
    let noiseOffset = (paperNoise - 0.5) * 0.15;  // subtle ±7.5% perturbation
    float noisyMask = clamp(msaaMask + noiseOffset, 0.0, 1.0);

    // Temporal blend for stability
    params.wetMaskOut[pixel] = lerp(wetMask, noisyMask, 0.3);
}
