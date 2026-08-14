#language slang 2026

module toon_link;

import mltrs;
import tev;

ParameterBlock<ToonLinkParams> params;

[[vk::push_constant]] ConstantBuffer<ToonLinkDraw> draw;

struct ToonLinkDraw {
    mltrs::ImmutableAddr<Material> material;
}

struct Material {
    // the base image
    Sampler2D.Handle tex0;
    // either the ZBtoonEX ramp, the 'hitomi' pupil for the eyes, or an unused 1x1
    Sampler2D.Handle tex1;
    TevParams tev;
    // last so that the 16-aligned members pack ahead of it
    GXAlphaCompare alphaCompare;
}

// `GXLights` and `GXTevColorOverride` must appear only here, never inside
// `Material`. This block is std140 and Material is std430, behind a pointer.
// A type reflected under both layouts trips the "shared type has an incompatible
// layout" panic in codegen.
struct ToonLinkParams {
    mltrs::MVPMatrices mvp;
    GXLights lights;
    GXTevColorOverride env;
    DebugMode debugMode;
}

// The variant names are the debug window's radio button labels, via
// `pascal_to_display`. There is no parallel list of names on the Rust side.
enum DebugMode : uint {
    // The shaded pixel: the GX_TEVPREV register after the whole stage chain.
    FinalTev = 0,
    // The world-space normal as `n * 0.5 + 0.5`. Mid-gray is a zero component,
    // red is +X, green is +Y (up), blue is +Z.
    WorldNormals = 1,
    // The vertex attribute uv0, with no texgen applied: red is u, green is v.
    Uv0 = 2,
    // The TEV output alpha as grayscale: black is 0, white is 1.
    TevAlpha = 3,
    // The lit vertex channel before TEV reads it. Red is LIGHT0, green is the
    // eflight LIGHT1, blue is the flat ambient term.
    RasterColor0 = 4,
    // Texgen 1's coord: red is u1, green is v1. On the lit materials this is
    // the SRTG ramp lookup. On eyeL and eyeR it is the pupil UV.
    Texgen1Coord = 5,
    // The albedo texel at texcoord 0, with no swap tables and no stage chain.
    RawTex0 = 6,
    // Slot 1 at texcoord 1: the ramp, the pupil, or a 1x1 white dummy where
    // slot 1 is unreferenced.
    RawTex1 = 7,
    // FinalTev with the color channel and the texgens recomputed from the
    // interpolated normal, instead of interpolating the per-vertex results.
    ChannelPerPixel = 8,
    // FinalTev with the texgen matrices forced to identity. Only eyeL and eyeR
    // use a non-identity matrix, so every other material matches FinalTev.
    IdentityTexMtx = 9,
}

struct Vertex {
    float3 position;
    // model space; the converter bakes the bind pose, so no skinning here
    float3 normal;
    float2 uv0;
};

struct FragVertex {
    float4 position : SV_Position;
    float3 normal;
    float2 uv0;
    // GX evaluates the color channel and the texgens per vertex, in the XF unit.
    float4 color0;
    // Both texgen outputs, packed. The reflection allows an array field only
    // with a uniform binding, and this is a varying, so a third texgen needs a
    // `texcoord23` field. tev_ir.rs gates num_tex_gens at 2.
    float4 texcoord01;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    let worldNormal = params.mvp.rotateDirection(vertex.normal);
    let tev = draw.material.tev;

    let color0 = tev.evalRasterColor(worldNormal, params.lights);
    let forceIdentity = params.debugMode == DebugMode.IdentityTexMtx;
    let texcoord01 = tev.evalTexGens(vertex.uv0, color0, forceIdentity);

    return FragVertex(position, worldNormal, vertex.uv0, color0, texcoord01);
}

// The color target is _SRGB, so the hardware applies the linear → sRGB encode
// on store. The GX values must reach the stored bits unchanged, so this decode
// cancels that encode. RGB only, never alpha.
float3 srgbDecode(float3 c) {
    return select(c <= 0.04045, c / 12.92, pow((c + 0.055) / 1.055, 2.4));
}

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    let material = draw.material[0];

    Sampler2D tex0 = material.tex0;
    Sampler2D tex1 = material.tex1;

    float4 color0 = fragVertex.color0;
    float4 texcoord01 = fragVertex.texcoord01;
    if (params.debugMode == DebugMode.ChannelPerPixel) {
        color0 = material.tev.evalRasterColor(fragVertex.normal, params.lights);
        texcoord01 = material.tev.evalTexGens(fragVertex.uv0, color0, false);
    }

    let tevOut = material.tev.evalStages(color0, texcoord01, tex0, tex1, params.env);

    // GX order: the alpha test runs on the TEV output, not on a raw texel.
    if (!material.alphaCompare.test(tevOut.a)) {
        discard;
    }

    float3 color;
    switch (params.debugMode) {
        case DebugMode.WorldNormals:
            color = normalize(fragVertex.normal) * 0.5 + 0.5;
            break;
        case DebugMode.Uv0:
            color = float3(fragVertex.uv0, 0.0);
            break;
        case DebugMode.TevAlpha:
            color = float3(tevOut.a);
            break;
        case DebugMode.RasterColor0:
            color = color0.rgb;
            break;
        case DebugMode.Texgen1Coord:
            color = float3(texcoord01.zw, 0.0);
            break;
        case DebugMode.RawTex0:
            color = tex0.Sample(texcoord01.xy).rgb;
            break;
        case DebugMode.RawTex1:
            color = tex1.Sample(texcoord01.zw).rgb;
            break;
        default:
            color = tevOut.rgb;
            break;
    }

    // Four materials blend with Source_Alpha, so the real TEV alpha reaches the
    // target, not 1.0.
    return float4(srgbDecode(color), tevOut.a);
}
