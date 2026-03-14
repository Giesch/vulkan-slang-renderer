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
    Sampler2D<float4> pigmentIn_0_3;
    Sampler2D<float4> pigmentIn_4_7;
    Sampler2D<float4> pigmentIn_8_11;

    Texture2D<float> uIn;
    Texture2D<float> vIn;
    Texture2D<float> wetMask;
    Texture2D<float> paperHeight;

    RWTexture2D<float4> pigmentOut_0_3;
    RWTexture2D<float4> pigmentOut_4_7;
    RWTexture2D<float4> pigmentOut_8_11;

    Texture2D<float4> depositIn_0_3;
    Texture2D<float4> depositIn_4_7;
    Texture2D<float4> depositIn_8_11;

    RWTexture2D<float4> depositOut_0_3;
    RWTexture2D<float4> depositOut_4_7;
    RWTexture2D<float4> depositOut_8_11;

    float2 gridSize;
    float dt;
    float transferRate;

    PigmentProperties pigment0;
    PigmentProperties pigment1;
    PigmentProperties pigment2;
    PigmentProperties pigment3;
    PigmentProperties pigment4;
    PigmentProperties pigment5;
    PigmentProperties pigment6;
    PigmentProperties pigment7;
    PigmentProperties pigment8;
    PigmentProperties pigment9;
    PigmentProperties pigment10;
    PigmentProperties pigment11;
}

void transferChannel(inout float pigmentInWater, inout float pigmentDeposit, float paperHeight, PigmentProperties props, float rate) {
    // h_eff: granulating pigments prefer valleys (low h)
    let effectiveHeight = lerp(paperHeight, 1.0 - paperHeight, props.granulation);

    // Adsorption: water → paper
    let adsorb = max(1.0 - effectiveHeight * props.density, 0.0);
    let deltaDown = rate * pigmentInWater * adsorb * adsorb;

    // Desorption: paper → water (harder for staining pigments)
    let desorb = max(1.0 + (effectiveHeight - 1.0) * props.density, 0.0);
    let deltaUp = rate * pigmentDeposit * desorb * desorb / max(props.stainingPower, 0.1);

    // Clamp to available amounts
    let actualDown = min(deltaDown, pigmentInWater);
    let actualUp = min(deltaUp, pigmentDeposit);

    pigmentInWater = pigmentInWater - actualDown + actualUp;
    pigmentDeposit = pigmentDeposit + actualDown - actualUp;
}

void advectAndTransferGroup(
    Sampler2D<float4> pigmentIn,
    RWTexture2D<float4> pigmentOut,
    Texture2D<float4> depositIn,
    RWTexture2D<float4> depositOut,
    int2 pixel, float wetMask, float2 velocity, float2 position,
    float paperHeight, PigmentProperties props[4], float transferRate
) {
    let invGridSize = 1.0 / params.gridSize;

    // --- Advection ---
    float4 advected;
    if (wetMask < 0.001) {
        advected = pigmentIn.SampleLevel((float2(pixel) + 0.5) * invGridSize, 0.0);
    } else {
        // Semi-Lagrangian backtrace
        let backtrace = position - params.dt * velocity;
        advected = max(pigmentIn.SampleLevel(backtrace * invGridSize, 0.0), float4(0.0));
    }

    // --- Transfer pigment ↔ deposit ---
    if (wetMask < 0.001) {
        pigmentOut[pixel] = advected;
        depositOut[pixel] = depositIn[pixel];
        return;
    }

    float4 pigmentInWater = advected;
    float4 pigmentDeposit = depositIn[pixel];

    for (int i = 0; i < 4; i++) {
        float pigmentChannel = pigmentInWater[i];
        float depositChannel = pigmentDeposit[i];
        transferChannel(pigmentChannel, depositChannel, paperHeight, props[i], transferRate);
        pigmentInWater[i] = pigmentChannel;
        pigmentDeposit[i] = depositChannel;
    }

    pigmentOut[pixel] = max(pigmentInWater, float4(0.0));
    depositOut[pixel] = max(pigmentDeposit, float4(0.0));
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

    // Compute cell-center velocity and paper height (only needed for wet pixels)
    float2 velocity = float2(0.0);
    float paperHeight = 0.0;
    if (wetMask >= 0.001) {
        let uBottom = params.uIn[pixel];
        let uTop = params.uIn[clamp(pixel + int2(0, 1), int2(0), gridSize - 1)];
        let vLeft = params.vIn[pixel];
        let vRight = params.vIn[clamp(pixel + int2(1, 0), int2(0), gridSize - 1)];
        velocity = float2((uBottom + uTop) * 0.5, (vLeft + vRight) * 0.5);
        paperHeight = params.paperHeight[pixel];
    }

    let position = float2(pixel) + 0.5;

    // Group 0-3
    {
        PigmentProperties props[4] = { params.pigment0, params.pigment1, params.pigment2, params.pigment3 };
        advectAndTransferGroup(
            params.pigmentIn_0_3, params.pigmentOut_0_3,
            params.depositIn_0_3, params.depositOut_0_3,
            pixel, wetMask, velocity, position, paperHeight, props, params.transferRate
        );
    }

    // Group 4-7
    {
        PigmentProperties props[4] = { params.pigment4, params.pigment5, params.pigment6, params.pigment7 };
        advectAndTransferGroup(
            params.pigmentIn_4_7, params.pigmentOut_4_7,
            params.depositIn_4_7, params.depositOut_4_7,
            pixel, wetMask, velocity, position, paperHeight, props, params.transferRate
        );
    }

    // Group 8-11
    {
        PigmentProperties props[4] = { params.pigment8, params.pigment9, params.pigment10, params.pigment11 };
        advectAndTransferGroup(
            params.pigmentIn_8_11, params.pigmentOut_8_11,
            params.depositIn_8_11, params.depositOut_8_11,
            pixel, wetMask, velocity, position, paperHeight, props, params.transferRate
        );
    }
}
