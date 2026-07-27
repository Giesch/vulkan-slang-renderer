#language slang 2026

module toon_link;

import mvp;
import tev;

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
    // See DEBUG_* below.
    //
    // NOTE: changing this struct's shape while `just dev` runs trips
    // assert_shader_interface_unchanged — body edits hot-reload, shape
    // edits need `just shaders` + a restart.
    uint  debugMode;
}

// Keep in sync with examples/toon_link.rs's legend and module doc comment.
static const uint DEBUG_TEV = 0;         // final TEV output
static const uint DEBUG_NORMALS = 1;     // world-space normals
static const uint DEBUG_UV0 = 2;         // uv0
static const uint DEBUG_ALPHA = 3;       // final TEV alpha as grayscale
static const uint DEBUG_COLOR0 = 4;      // the rasterized channel, pre-TEV
static const uint DEBUG_SRTG = 5;        // texgen 1's coord as red/green
static const uint DEBUG_TEX0 = 6;        // raw tex0 at texcoord 0
static const uint DEBUG_TEX1 = 7;        // raw tex1 at texcoord 1
static const uint DEBUG_CHAN_PER_PIXEL = 8;  // channel recomputed per fragment
static const uint DEBUG_IDENTITY_TEXMTX = 9; // texgen matrices forced to identity

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
    // the model matrix is rotation-free uniform scale in P6, so
    // rotateDirection is exact; renormalized per-fragment after interpolation
    let worldNormal = params.mvp.rotateDirection(vertex.normal);

    let color0 = params.tev.evalRasterColor(worldNormal);
    let forceIdentity = params.debugMode == DEBUG_IDENTITY_TEXMTX;
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
    if (params.debugMode == DEBUG_CHAN_PER_PIXEL) {
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
    case DEBUG_NORMALS:
        color = normalize(fragVertex.normal) * 0.5 + 0.5;
        break;
    case DEBUG_UV0:
        color = float3(fragVertex.uv0, 0.0);
        break;
    case DEBUG_ALPHA:
        // the TEV output alpha — what the alpha test and the blend both see
        color = float3(tevOut.a);
        break;
    case DEBUG_COLOR0:
        // the lighting channel before TEV touches it; must be smoothly shaded
        color = color0.rgb;
        break;
    case DEBUG_SRTG:
        // texgen 1's coord. On the lit materials that is the SRTG ramp lookup;
        // on eyeL/eyeR it is the pupil UV, which is the useful reading there.
        color = float3(texcoord01.zw, 0.0);
        break;
    case DEBUG_TEX0:
        color = params.tex0.Sample(texcoord01.xy).rgb;
        break;
    case DEBUG_TEX1:
        color = params.tex1.Sample(texcoord01.zw).rgb;
        break;
    default:
        // final TEV output (modes 0, 8 and 9)
        color = tevOut.rgb;
        break;
    }

    // The real TEV alpha, not 1.0: four materials blend with Source_Alpha, so a
    // hardcoded 1.0 would silently turn their alpha blend into an opaque write.
    return float4(srgbDecode(color), tevOut.a);
}
