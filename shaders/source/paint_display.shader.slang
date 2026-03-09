#language slang 2026

module paint_display;

import fullscreen_triangle;

enum DebugView : uint {
    Pigments = 0,
    WetAreaMask = 1,
}

ParameterBlock<DisplayParams> displayParams;

struct PigmentKM {
    float3 absorption;
    float3 scattering;
}

struct DisplayParams {
    Sampler2D deposit;
    Sampler2D paperHeight;
    Sampler2D wetMask;
    float2 texelSize;
    uint debugView;
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

struct KMResult {
    float3 reflectance;
    float3 transmittance;
}

// Kubelka-Munk single layer reflectance and transmittance per RGB channel
KMResult kubelkaMunk(float3 absorption, float3 scattering, float thickness) {
    if (thickness < 0.001) {
        return KMResult(float3(0.0), float3(1.0));
    }

    float3 a = 1.0 + absorption / max(scattering, float3(0.001));
    float3 b = sqrt(max(a * a - 1.0, float3(0.0)));
    float3 bScatteringThickness = b * scattering * thickness;

    // Clamp to avoid overflow in sinh/cosh
    bScatteringThickness = min(bScatteringThickness, float3(20.0));

    float3 sinhBSx = sinh(bScatteringThickness);
    float3 coshBSx = cosh(bScatteringThickness);

    float3 denom = a * sinhBSx + b * coshBSx;
    float3 reflectance = sinhBSx / max(denom, float3(0.0001));
    float3 transmittance = b / max(denom, float3(0.0001));

    return KMResult(reflectance, transmittance);
}

// Composite a layer over a substrate reflectance
float3 compositeLayer(float3 layerReflectance, float3 layerTransmittance, float3 substrateReflectance) {
    return layerReflectance + layerTransmittance * layerTransmittance * substrateReflectance / max(float3(1.0) - layerReflectance * substrateReflectance, float3(0.0001));
}

static const float BUMP_STRENGTH = 10.0;
static const float3 LIGHT_DIR = normalize(float3(1, 1, 2));
static const float AMBIENT = 0.6;
static const float3 PAPER_COLOR = float3(0.95);

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let uv = input.texCoord;

    // Debug: show wet mask as grayscale
    if (displayParams.debugView == uint(DebugView.WetAreaMask)) {
        let wet = displayParams.wetMask.Sample(uv).r;
        return float4(wet, wet, wet, 1.0);
    }

    let texelSize = displayParams.texelSize;

    // Sample height map neighbors for bump mapping
    let heightLeft = displayParams.paperHeight.Sample(uv + float2(-texelSize.x, 0.0)).r;
    let heightRight = displayParams.paperHeight.Sample(uv + float2( texelSize.x, 0.0)).r;
    let heightDown = displayParams.paperHeight.Sample(uv + float2(0.0, -texelSize.y)).r;
    let heightUp = displayParams.paperHeight.Sample(uv + float2(0.0,  texelSize.y)).r;

    // Bump-mapped normal
    let heightGradientX = (heightRight - heightLeft) * BUMP_STRENGTH;
    let heightGradientY = (heightUp - heightDown) * BUMP_STRENGTH;
    let normal = normalize(float3(-heightGradientX, -heightGradientY, 1.0));

    // Lambertian lighting
    let diffuse = max(dot(normal, LIGHT_DIR), 0.0);
    let lighting = AMBIENT + (1.0 - AMBIENT) * diffuse;

    // Sample deposited pigment (4 channels)
    let deposit = displayParams.deposit.Sample(uv);

    // Kubelka-Munk compositing per Curtis et al. Section 5.2:
    // When a layer contains multiple pigments, K and S are weighted
    // in proportion to each pigment's relative thickness.
    PigmentKM pigments[4] = {
        displayParams.pigment0, displayParams.pigment1,
        displayParams.pigment2, displayParams.pigment3
    };

    float totalThickness = deposit.x + deposit.y + deposit.z + deposit.w;
    float3 totalReflectance = PAPER_COLOR;

    if (totalThickness > 0.001) {
        float3 mixedAbsorption = float3(0.0);
        float3 mixedScattering = float3(0.0);
        for (int i = 0; i < 4; i++) {
            mixedAbsorption += deposit[i] * pigments[i].absorption;
            mixedScattering += deposit[i] * pigments[i].scattering;
        }
        mixedAbsorption /= totalThickness;
        mixedScattering /= totalThickness;

        let kubelkaMunkResult = kubelkaMunk(mixedAbsorption, mixedScattering, totalThickness);
        totalReflectance = compositeLayer(kubelkaMunkResult.reflectance, kubelkaMunkResult.transmittance, totalReflectance);
    }

    // Apply lighting
    let color = totalReflectance * lighting;

    return float4(color, 1.0);
}
