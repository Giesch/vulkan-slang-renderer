#language slang 2026

module wc_transfer_pigment;

import watercolor_common;

ParameterBlock<Params> params;

public struct PigmentProperties {
    public float density;         // rho — controls deposition rate
    public float stainingPower;   // omega — resistance to lifting
    public float granulation;     // paper texture interaction
    public float _pad0;
}

struct Params {
    RWTexture2D<float4> pigment;
    RWTexture2D<float4> deposit;
    Texture2D<float> paperHeight;
    Texture2D<float> wetMask;
    float2 gridSize;
    float transferRate;
    float _pad;
    PigmentProperties pigment0;
    PigmentProperties pigment1;
    PigmentProperties pigment2;
    PigmentProperties pigment3;
}

void transferChannel(inout float g, inout float d, float h, PigmentProperties props, float rate) {
    // h_eff: granulating pigments prefer valleys (low h)
    let hEff = lerp(h, 1.0 - h, props.granulation);

    // Adsorption: water → paper
    let deltaDown = rate * g * pow(max(1.0 - hEff * props.density, 0.0), 2.0);

    // Desorption: paper → water (harder for staining pigments)
    let deltaUp = rate * d * pow(max(1.0 + (hEff - 1.0) * props.density, 0.0), 2.0) / max(props.stainingPower, 0.1);

    // Clamp to available amounts
    let actualDown = min(deltaDown, g);
    let actualUp = min(deltaUp, d);

    g = g - actualDown + actualUp;
    d = d + actualDown - actualUp;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    if (!inBounds(dispatchThreadID.xy, params.gridSize))
        return;

    let wet = params.wetMask[pixel];
    if (wet < 0.001)
        return;

    let h = params.paperHeight[pixel];
    float4 g = params.pigment[pixel];
    float4 d = params.deposit[pixel];

    PigmentProperties props[4] = { params.pigment0, params.pigment1, params.pigment2, params.pigment3 };

    for (int i = 0; i < 4; i++) {
        float gi = g[i];
        float di = d[i];
        transferChannel(gi, di, h, props[i], params.transferRate);
        g[i] = gi;
        d[i] = di;
    }

    params.pigment[pixel] = max(g, float4(0.0));
    params.deposit[pixel] = max(d, float4(0.0));
}
