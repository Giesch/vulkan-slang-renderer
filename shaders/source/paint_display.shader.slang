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
    Sampler2D deposit_0_3;
    Sampler2D deposit_4_7;
    Sampler2D deposit_8_11;
    Sampler2D paperHeight;
    Sampler2D wetMask;
    float2 texelSize;
    uint debugView;
    float canvasAspect;
    float windowAspect;
    PigmentKM pigment0;
    PigmentKM pigment1;
    PigmentKM pigment2;
    PigmentKM pigment3;
    PigmentKM pigment4;
    PigmentKM pigment5;
    PigmentKM pigment6;
    PigmentKM pigment7;
    PigmentKM pigment8;
    PigmentKM pigment9;
    PigmentKM pigment10;
    PigmentKM pigment11;
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
    let rawUV = input.texCoord;

    // Crop: remap UVs so canvas fills the window, cropping overflow
    float2 uv;
    let canvasAspect = displayParams.canvasAspect;
    let windowAspect = displayParams.windowAspect;
    if (windowAspect > canvasAspect) {
        // Window wider than canvas: crop top/bottom
        let scale = canvasAspect / windowAspect;
        uv.x = rawUV.x;
        uv.y = (rawUV.y - 0.5) * scale + 0.5;
    } else {
        // Window taller than canvas: crop left/right
        let scale = windowAspect / canvasAspect;
        uv.x = (rawUV.x - 0.5) * scale + 0.5;
        uv.y = rawUV.y;
    }

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

    // Sample deposited pigment (3 groups of 4 channels = 12 pigments)
    let deposit_0_3 = displayParams.deposit_0_3.Sample(uv);
    let deposit_4_7 = displayParams.deposit_4_7.Sample(uv);
    let deposit_8_11 = displayParams.deposit_8_11.Sample(uv);

    // Kubelka-Munk compositing per Curtis et al. Section 5.2:
    // When a layer contains multiple pigments, K and S are weighted
    // in proportion to each pigment's relative thickness.
    PigmentKM pigments[12] = {
        displayParams.pigment0, displayParams.pigment1,
        displayParams.pigment2, displayParams.pigment3,
        displayParams.pigment4, displayParams.pigment5,
        displayParams.pigment6, displayParams.pigment7,
        displayParams.pigment8, displayParams.pigment9,
        displayParams.pigment10, displayParams.pigment11
    };

    // Gather all 12 deposit thicknesses into an array
    float deposits[12];
    deposits[0] = deposit_0_3.x; deposits[1] = deposit_0_3.y;
    deposits[2] = deposit_0_3.z; deposits[3] = deposit_0_3.w;
    deposits[4] = deposit_4_7.x; deposits[5] = deposit_4_7.y;
    deposits[6] = deposit_4_7.z; deposits[7] = deposit_4_7.w;
    deposits[8] = deposit_8_11.x; deposits[9] = deposit_8_11.y;
    deposits[10] = deposit_8_11.z; deposits[11] = deposit_8_11.w;

    float totalThickness = 0.0;
    for (int i = 0; i < 12; i++)
        totalThickness += deposits[i];

    float3 totalReflectance = PAPER_COLOR;

    if (totalThickness > 0.001) {
        float3 mixedAbsorption = float3(0.0);
        float3 mixedScattering = float3(0.0);
        for (int i = 0; i < 12; i++) {
            mixedAbsorption += deposits[i] * pigments[i].absorption;
            mixedScattering += deposits[i] * pigments[i].scattering;
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
