#language slang 2026

module paint_display;

import fullscreen_triangle;

ParameterBlock<DisplayParams> displayParams;

struct PigmentKM {
    float3 K; // absorption
    float _pad0;
    float3 S; // scattering
    float _pad1;
}

struct DisplayParams {
    Sampler2D deposit;
    Sampler2D paperHeight;
    float2 texelSize;
    float2 _pad;
    PigmentKM pigment0;
    PigmentKM pigment1;
    PigmentKM pigment2;
    PigmentKM pigment3;
}

struct FragInput {
    float4 position : SV_Position;
    float2 texCoord : TEXCOORD0;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.texCoord);
}

// Kubelka-Munk single layer reflectance over substrate
// Returns (reflectance, transmittance) per RGB channel
float3 kmReflectance(float3 K, float3 S, float thickness) {
    if (thickness < 0.001)
        return float3(0.0);

    float3 a = 1.0 + K / max(S, float3(0.001));
    float3 b = sqrt(max(a * a - 1.0, float3(0.0)));
    float3 bSx = b * S * thickness;

    // Clamp to avoid overflow in sinh/cosh
    bSx = min(bSx, float3(20.0));

    float3 shBSx = sinh(bSx);
    float3 chBSx = cosh(bSx);

    float3 denom = a * shBSx + b * chBSx;
    float3 R = shBSx / max(denom, float3(0.0001));

    return R;
}

float3 kmTransmittance(float3 K, float3 S, float thickness) {
    if (thickness < 0.001)
        return float3(1.0);

    float3 a = 1.0 + K / max(S, float3(0.001));
    float3 b = sqrt(max(a * a - 1.0, float3(0.0)));
    float3 bSx = b * S * thickness;
    bSx = min(bSx, float3(20.0));

    float3 shBSx = sinh(bSx);
    float3 chBSx = cosh(bSx);

    float3 denom = a * shBSx + b * chBSx;
    float3 T = b / max(denom, float3(0.0001));

    return T;
}

// Composite a layer over a substrate reflectance
float3 compositeLayer(float3 R_layer, float3 T_layer, float3 R_sub) {
    return R_layer + T_layer * T_layer * R_sub / max(float3(1.0) - R_layer * R_sub, float3(0.0001));
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let uv = input.texCoord;
    let ts = displayParams.texelSize;

    // Sample height map neighbors for bump mapping
    let hL = displayParams.paperHeight.Sample(uv + float2(-ts.x, 0.0)).r;
    let hR = displayParams.paperHeight.Sample(uv + float2( ts.x, 0.0)).r;
    let hD = displayParams.paperHeight.Sample(uv + float2(0.0, -ts.y)).r;
    let hU = displayParams.paperHeight.Sample(uv + float2(0.0,  ts.y)).r;

    // Bump-mapped normal
    let bumpStrength = 10.0;
    let dhdx = (hR - hL) * bumpStrength;
    let dhdy = (hU - hD) * bumpStrength;
    let normal = normalize(float3(-dhdx, -dhdy, 1.0));

    // Lambertian lighting
    let lightDir = normalize(float3(-1.0, -1.0, 2.0));
    let ambient = 0.6;
    let diffuse = max(dot(normal, lightDir), 0.0);
    let lighting = ambient + (1.0 - ambient) * diffuse;

    // Paper substrate reflectance (warm white)
    let paperBase = float3(0.95);

    // Sample deposited pigment (4 channels)
    let dep = displayParams.deposit.Sample(uv);

    // Kubelka-Munk compositing: layer each pigment over paper
    PigmentKM pigments[4] = {
        displayParams.pigment0, displayParams.pigment1,
        displayParams.pigment2, displayParams.pigment3
    };

    float3 R_total = paperBase;

    for (int i = 0; i < 4; i++) {
        let thickness = dep[i];
        if (thickness < 0.001)
            continue;

        let R_layer = kmReflectance(pigments[i].K, pigments[i].S, thickness);
        let T_layer = kmTransmittance(pigments[i].K, pigments[i].S, thickness);
        R_total = compositeLayer(R_layer, T_layer, R_total);
    }

    // Apply lighting
    let color = R_total * lighting;

    return float4(color, 1.0);
}
