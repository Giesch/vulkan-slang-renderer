#language slang 2026

module toon_link;

import mvp;
import tev;

// Keep in sync with examples/toon_link.rs's module doc comment. The variant
// names are what the debug window's radio buttons are labelled with, via
// `pascal_to_display` — there is no parallel list of names on the Rust side.
enum DebugMode : uint {
    FinalTev = 0,         // final TEV output
    WorldNormals = 1,     // world-space normals
    Uv0 = 2,
    TevAlpha = 3,         // final TEV alpha as grayscale
    RasterColor0 = 4,     // the rasterized channel, pre-TEV
    Texgen1Coord = 5,     // texgen 1's coord as red/green
    RawTex0 = 6,          // raw tex0 at texcoord 0
    RawTex1 = 7,          // raw tex1 at texcoord 1
    ChannelPerPixel = 8,  // channel recomputed per fragment
    IdentityTexMtx = 9,   // texgen matrices forced to identity
    // Selecting this one also switches the example to RenderMode::MaskWhite,
    // which draws only the four `*damA` mask batches. On its own it would paint
    // the whole model white, so the two must move together — see
    // `ToonLink::mode` in examples/toon_link.rs.
    MaskWhite = 10,       // solid white wherever TEV alpha > 0
}

ParameterBlock<ToonLinkParams> params;

struct ToonLinkParams {
    // texmap slot 0 — the albedo. Slot 1 is the ZBtoonEX ramp (or the hitomi
    // pupil on eyeL/eyeR). Both are sampled by the TEV interpreter, selected
    // per stage by the material's TEV order.
    Sampler2D tex0;
    Sampler2D tex1;

    MVPMatrices mvp;

    // The whole per-material GX configuration, built by src/tev_pack.rs.
    TevParams tev;

    // raw GX codes, straight from MAT3 via the manifest.
    // GXCompare: NEVER 0, LESS 1, EQUAL 2, LEQUAL 3,
    //            GREATER 4, NEQUAL 5, GEQUAL 6, ALWAYS 7
    uint4 alphaCompare;      // comp0, ref0, comp1, ref1
    uint  alphaCompareOp;    // GXAlphaOp: AND 0, OR 1, XOR 2, XNOR 3
    // NOTE: changing this struct's shape while `just dev` runs trips
    // assert_shader_interface_unchanged — body edits hot-reload, shape
    // edits need `just shaders` + a restart.
    DebugMode debugMode;
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
    // GX evaluates the color channel and the texgens in the XF unit, per vertex.
    float4 color0;
    // Both texgen outputs packed as (uv0, uv1). Not an array: the reflection
    // only allows array fields with a uniform binding
    // (src/shaders/reflection/parameters.rs), and this is a varying. A third and
    // fourth texgen would need a `texcoord23` field, which is why tev_ir.rs
    // gates num_tex_gens at 2.
    float4 texcoord01;
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    // the model matrix is a Y rotation with uniform scale, which rotateDirection
    // handles exactly (only a *non-uniform* scale would need the
    // inverse-transpose); renormalized per-fragment after interpolation
    let worldNormal = params.mvp.rotateDirection(vertex.normal);

    let color0 = params.tev.evalRasterColor(worldNormal);
    let forceIdentity = params.debugMode == DebugMode.IdentityTexMtx;
    let texcoord01 = params.tev.evalTexGens(vertex.uv0, color0, forceIdentity);

    return FragVertex(position, worldNormal, vertex.uv0, color0, texcoord01);
}

// One GXCompare function on the 8-bit alpha the hardware compares.
bool gxCompare(uint comp, uint alpha, uint reference) {
    switch (comp) {
    case 0: return false;              // NEVER
    case 1: return alpha < reference;  // LESS
    case 2: return alpha == reference; // EQUAL
    case 3: return alpha <= reference; // LEQUAL
    case 4: return alpha > reference;  // GREATER
    case 5: return alpha != reference; // NEQUAL
    case 6: return alpha >= reference; // GEQUAL
    default: return true;              // ALWAYS (7)
    }
}

// Combine the two compare results per GXAlphaOp.
bool gxAlphaOp(uint op, bool r0, bool r1) {
    switch (op) {
    case 0: return r0 && r1;           // AND
    case 1: return r0 || r1;           // OR
    case 2: return r0 != r1;           // XOR
    default: return r0 == r1;          // XNOR (3)
    }
}

// sRGB → linear on one channel. The color target is _SRGB, so the hardware
// applies linear → sRGB *encode* on store; we hold raw GX values that must
// reach the stored bits unchanged, so we apply the *decode* first and the two
// transfers cancel. Applied to RGB only, never alpha.
//
// Verified numerically in P7 (2026-07-27): a flat tunic texel round-trips to the
// source PNG's byte value with 0 LSB error.
float srgbDecodeChannel(float c) {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

float3 srgbDecode(float3 c) {
    return float3(
        srgbDecodeChannel(c.r),
        srgbDecodeChannel(c.g),
        srgbDecodeChannel(c.b));
}

[shader("fragment")]
float4 fragMain(FragVertex fragVertex) : SV_TARGET {
    // Debug mode 8 re-runs the color channel per fragment, so mode 0 vs mode 8
    // is a direct A/B of GX's per-vertex lighting against the smoother
    // per-fragment version (phase_08 decision 4). It has to happen before the
    // stages, since the channel feeds RASC/RASA and the SRTG texcoord.
    float4 color0 = fragVertex.color0;
    float4 texcoord01 = fragVertex.texcoord01;
    if (params.debugMode == DebugMode.ChannelPerPixel) {
        color0 = params.tev.evalRasterColor(fragVertex.normal);
        texcoord01 = params.tev.evalTexGens(fragVertex.uv0, color0, false);
    }

    let tevOut =
        params.tev.evalStages(color0, texcoord01, params.tex0, params.tex1);

    // GX order: the alpha test runs on the TEV output, not on a raw texel. It
    // runs in every debug mode, so the cutout stays visible while inspecting
    // normals or UVs.
    let alpha8 = uint(round(saturate(tevOut.a) * 255.0));
    let keep0 = gxCompare(params.alphaCompare.x, alpha8, params.alphaCompare.y);
    let keep1 = gxCompare(params.alphaCompare.z, alpha8, params.alphaCompare.w);
    if (!gxAlphaOp(params.alphaCompareOp, keep0, keep1)) {
        discard;
    }

    float3 color;
    switch (params.debugMode) {
    case DebugMode.WorldNormals:
        // world-space normals as color (the P6 diagnostic)
        color = normalize(fragVertex.normal) * 0.5 + 0.5;
        break;
    case DebugMode.Uv0:
        // uv0: red = u, green = v
        color = float3(fragVertex.uv0, 0.0);
        break;
    case DebugMode.TevAlpha:
        // the TEV output alpha — what the alpha test and the blend both see
        color = float3(tevOut.a);
        break;
    case DebugMode.RasterColor0:
        // the lighting channel before TEV touches it; must be smoothly shaded
        color = color0.rgb;
        break;
    case DebugMode.Texgen1Coord:
        // texgen 1's coord. On the lit materials that is the SRTG ramp lookup;
        // on eyeL/eyeR it is the pupil UV, which is the useful reading there.
        color = float3(texcoord01.zw, 0.0);
        break;
    case DebugMode.RawTex0:
        color = params.tex0.Sample(texcoord01.xy).rgb;
        break;
    case DebugMode.RawTex1:
        color = params.tex1.Sample(texcoord01.zw).rgb;
        break;
    case DebugMode.MaskWhite:
        // The mask view. The `*damA` materials compare Always, so their
        // silhouette lives entirely in alpha and the GX alpha test above never
        // discards; this discard is what makes the shape visible. It is the
        // same predicate the composite pass runs (`Greater 0` on the same texel
        // of the same texture through the same texcoord), so what is white here
        // is exactly what composites there.
        if (tevOut.a <= 0.0) {
            discard;
        }
        color = float3(1.0);
        break;
    default:
        // final TEV output (FinalTev, ChannelPerPixel and IdentityTexMtx)
        color = tevOut.rgb;
        break;
    }

    // The real TEV alpha, not 1.0: four materials blend with Source_Alpha, so a
    // hardcoded 1.0 would silently turn their alpha blend into an opaque write.
    return float4(srgbDecode(color), tevOut.a);
}
