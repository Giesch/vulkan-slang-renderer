#language slang 2026

module wc_advect_and_transfer_pigment;

import watercolor_common;

ParameterBlock<Params> params;

public struct PigmentProperties {
    public float density;         // rho — controls deposition rate
    public float stainingPower;   // omega — resistance to lifting
    public float granulation;     // paper texture interaction
}

struct Params {
    Texture2D<float4> pigmentIn;
    Texture2D<float> uIn;
    Texture2D<float> vIn;
    Texture2D<float> wetMask;
    Texture2D<float> paperHeight;
    Texture2D<float> saturation;
    RWTexture2D<float4> pigmentOut;
    RWTexture2D<float4> deposit;
    float2 gridSize;
    float dt;
    float transferRate;
    PigmentProperties pigment0;
    PigmentProperties pigment1;
    PigmentProperties pigment2;
    PigmentProperties pigment3;
}

void transferChannel(inout float pigmentInWater, inout float pigmentDeposit, float paperHeight, PigmentProperties props, float rate) {
    // h_eff: granulating pigments prefer valleys (low h)
    let effectiveHeight = lerp(paperHeight, 1.0 - paperHeight, props.granulation);

    // Adsorption: water → paper
    let deltaDown = rate * pigmentInWater * pow(max(1.0 - effectiveHeight * props.density, 0.0), 2.0);

    // Desorption: paper → water (harder for staining pigments)
    let deltaUp = rate * pigmentDeposit * pow(max(1.0 + (effectiveHeight - 1.0) * props.density, 0.0), 2.0) / max(props.stainingPower, 0.1);

    // Clamp to available amounts
    let actualDown = min(deltaDown, pigmentInWater);
    let actualUp = min(deltaUp, pigmentDeposit);

    pigmentInWater = pigmentInWater - actualDown + actualUp;
    pigmentDeposit = pigmentDeposit + actualDown - actualUp;
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

    // --- Advection ---
    float4 advected;
    if (wetMask < 0.001) {
        advected = params.pigmentIn[pixel];
    } else {
        // Cell-center velocity at (i+0.5, j+0.5) by interpolating staggered components
        // u at (i+0.5, j+0.5): average u[i,j] and u[i,j+1]
        let uBottom = params.uIn[pixel];                                        // u at (i+0.5, j)
        let uTop = params.uIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];   // u at (i+0.5, j+1)
        // v at (i+0.5, j+0.5): average v[i,j] and v[i+1,j]
        let vLeft = params.vIn[pixel];                                        // v at (i, j+0.5)
        let vRight = params.vIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];   // v at (i+1, j+0.5)
        let velocity = float2((uBottom + uTop) * 0.5, (vLeft + vRight) * 0.5);

        // Semi-Lagrangian backtrace
        let position = float2(pixel) + 0.5;
        let backtrace = position - params.dt * velocity;

        advected = max(bilinearSampleRGBA(params.pigmentIn, backtrace, params.gridSize), float4(0.0));
    }

    // --- Transfer pigment ↔ deposit ---
    // When a cell dries, all remaining pigment in water deposits onto the paper.
    // This is critical for edge darkening: flow concentrates pigment at edges,
    // then drying locks it in place.
    if (wetMask < 0.001) {
        params.deposit[pixel] = params.deposit[pixel] + advected;
        params.pigmentOut[pixel] = float4(0.0);
        return;
    }

    let paperHeight = params.paperHeight[pixel];
    float4 pigmentInWater = advected;
    float4 pigmentDeposit = params.deposit[pixel];

    // Scale transfer rate by inverse saturation: as water evaporates,
    // pigment concentration increases, accelerating deposition.
    let saturation = params.saturation[pixel];
    let concentrationFactor = 1.0 / max(saturation, 0.1);
    let effectiveRate = params.transferRate * concentrationFactor;

    PigmentProperties props[4] = { params.pigment0, params.pigment1, params.pigment2, params.pigment3 };

    for (int i = 0; i < 4; i++) {
        float pigmentChannel = pigmentInWater[i];
        float depositChannel = pigmentDeposit[i];
        transferChannel(pigmentChannel, depositChannel, paperHeight, props[i], effectiveRate);
        pigmentInWater[i] = pigmentChannel;
        pigmentDeposit[i] = depositChannel;
    }

    params.pigmentOut[pixel] = max(pigmentInWater, float4(0.0));
    params.deposit[pixel] = max(pigmentDeposit, float4(0.0));
}
