#language slang 2026

module toon_link_modern;
import mltrs;

ParameterBlock<ModernParams> params;

[[vk::push_constant]] ConstantBuffer<ModernMultiDraw> multiDraw;

struct ModernMultiDraw {
    mltrs::ImmutableAddr<ModernIndividualDraw> individualDraws;
}

struct ModernIndividualDraw {
    mltrs::ImmutableAddr<ModernMaterial> material;
}

struct ModernMaterial {
    Sampler2D.Handle albedo;
    Sampler2D.Handle pupil;
    float2 pupilOffset;
    uint hasPupil;
    float lightingMix;
}

struct ModernParams {
    mltrs::MVPMatrices mvp;
    float4 mainDirection;
    float4 secondaryDirection;
    float4 shadowColor;
    float4 litColor;
    float4 secondaryColor;
    // center, softness, LUT ambient, secondary intensity
    float4 controls;
    Sampler2D.Handle rampTexture;
    ModernRamp ramp;
    ModernDiagnostic diagnostic;
}

enum ModernRamp : uint {
    Analytic = 0,
    Texture = 1
}

enum ModernDiagnostic : uint {
    Final = 0,
    WorldNormals = 1,
    Uv0 = 2,
    NDotL = 3,
    BandOnly = 4,
    AlbedoOnly = 5
}

struct ModernVertex {
    float3 position;
    float3 normal;
    float2 uv0;
}

struct ModernFragment {
    float4 position : SV_Position;
    float3 normal;
    float2 uv0;
    nointerpolation uint drawIndex;
}

[shader("vertex")]
ModernFragment vertMain(ModernVertex vertex, uint drawIndex : SV_DrawIndex) {
    return ModernFragment(
        params.mvp.project(vertex.position),
        params.mvp.rotateDirection(vertex.normal),
        vertex.uv0,
        drawIndex
    );
}

float modernBand(float cosine) {
    let center = params.controls.x;
    let softness = params.controls.y;
    // smoothstep has undefined division when both edges coincide.

    return softness <= 0.0
        ? (cosine >= center ? 1.0 : 0.0)
        : smoothstep(center - softness, center + softness, cosine);
}

[shader("fragment")]
float4 fragMain(ModernFragment fragment) : SV_TARGET {
    let material = multiDraw.individualDraws[fragment.drawIndex].material;
    Sampler2D albedoTexture = material.albedo;
    var albedo = albedoTexture.Sample(fragment.uv0);
    // Coverage is raw alpha, including in every diagnostic and invisible mask.
    let invisible = albedo.a <= 0.0;
    if (invisible) {
        discard;
    }

    let hasPupil = material.hasPupil != 0;
    if (hasPupil) {
        Sampler2D pupilTexture = material.pupil;
        albedo.rgb *= pupilTexture.Sample(fragment.uv0 + material.pupilOffset).rgb;
    }

    let normal = normalize(fragment.normal);
    let cosine = saturate(dot(normal, params.mainDirection.xyz));
    let secondaryCosine = params.controls.w > 0.0
        ? saturate(dot(normal, params.secondaryDirection.xyz))
        : 0.0;
    var band = modernBand(cosine);
    var secondaryBand = modernBand(secondaryCosine);
    let useTextureRamp = params.ramp == ModernRamp.Texture;
    if (useTextureRamp) {
        // A real two-dimensional data lookup. Disabled/zero secondary uses
        // a fixed coordinate independent of its retained direction and tint.
        let rampCoord = saturate(float2(cosine, secondaryCosine) + params.controls.z);
        let weights = params.rampTexture.Sample(rampCoord);
        band = weights.r;
        secondaryBand = weights.g;
    }

    let directLighting = lerp(params.shadowColor.rgb, params.litColor.rgb, band);
    let litAlbedo = albedo.rgb * directLighting
        + params.secondaryColor.rgb * params.controls.w * secondaryBand;
    var color = lerp(litAlbedo, albedo.rgb, material.lightingMix);
    switch (params.diagnostic) {
        case ModernDiagnostic.WorldNormals: color = normal * 0.5 + 0.5; break;
        case ModernDiagnostic.Uv0: color = float3(fragment.uv0, 0.0); break;
        case ModernDiagnostic.NDotL: color = float3(cosine); break;
        case ModernDiagnostic.BandOnly: color = float3(band); break;
        case ModernDiagnostic.AlbedoOnly: color = albedo.rgb; break;
        default: break;
    }

    // Color samplers decode sRGB; CPU colors are already linear. The target
    // encodes once. Source alpha alone composites feature edges.

    return float4(color, albedo.a);
}
