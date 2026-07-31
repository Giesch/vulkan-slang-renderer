#language slang 2026

module toon_link;

import mvp;
import tev;

// Keep in sync with examples/toon_link.rs's module doc comment. The variant
// names are what the debug window's radio buttons are labelled with, via
// `pascal_to_display` — there is no parallel list of names on the Rust side.
//
// Three things hold in *every* mode, so the notes below only cover the rgb:
// the GX alpha test still runs on the TEV output before the switch, the output
// alpha is always the real `tevOut.a` (so blending still happens), and the rgb
// goes through `srgbDecode` — which cancels the _SRGB target's encode, so the
// bytes in the framebuffer are the raw values described here.
enum DebugMode : uint {
    // final TEV output.
    // `evalStages`'s GX_TEVPREV register: the shaded pixel. On a toon material
    // that is the band lerp between reg[1] (shadow) and konst[0] (lit) selected
    // by the ramp's red, modulated by the albedo, plus konst[1] (the eflight
    // highlight) weighted by the ramp's green. Shares the switch's `default:`
    // arm with ChannelPerPixel and IdentityTexMtx.
    FinalTev = 0,
    // world-space normals.
    // `normalize(normal) * 0.5 + 0.5`, so mid-gray is a zero component: red =
    // +X, green = +Y (up), blue = +Z. The normal is the model matrix's rotation
    // applied per vertex, interpolated, then renormalized here — the P6
    // diagnostic for a bad rotation or a bad interpolation.
    WorldNormals = 1,
    // the mesh's raw uv0.
    // red = u, green = v, blue = 0, straight off the vertex attribute with no
    // texgen applied. Every material's texgen 0 is MTX2x4/TEX0/IDENTITY, so
    // this is also what texcoord 0 samples the albedo with.
    Uv0 = 2,
    // final TEV alpha as grayscale.
    // `tevOut.a` replicated to rgb: black 0, white 1. The same number the alpha
    // test just compared, that the blend sees, and that the mask pass writes to
    // destination alpha. Use this rather than MaskWhite to read alpha's
    // magnitude, not just its coverage.
    TevAlpha = 3,
    // the rasterized channel, pre-TEV.
    // The "control colors": `evalRasterColor`'s COLOR0 rgb, which is what the
    // SRTG texgen turns into the ZBtoonEX lookup. With material color white and
    // ambient 50/255, red = saturate(0.196 + N·L for LIGHT0), green = the same
    // for the eflight LIGHT1 (0.196 flat when eflight is off) and blue = the
    // flat 0.196 ambient. Must look smoothly shaded — it is per-vertex lit and
    // interpolated, so any banding here is a bug, not the toon step. Unlit
    // materials short-circuit to flat material color (white).
    RasterColor0 = 4,
    // texgen 1's coord as red/green.
    // `texcoord01.zw`: red = u1, green = v1, blue = 0. On the 12 lit materials
    // texgen 1 is GX_TG_SRTG, so the coord *is* RasterColor0's (r, g) — this
    // view is mode 4 with blue zeroed, and it is the ramp lookup: u picks the
    // shadow/lit step out of the ramp's red, v picks the eflight step out of
    // its green, both flipping at ~0.49. On eyeL/eyeR it is instead the
    // TEXMTX1-translated pupil UV, which is the useful reading there. Materials
    // with one texgen never write uv[1], so they read black.
    Texgen1Coord = 5,
    // raw tex0 at texcoord 0.
    // The albedo texel (linktexS3TC) sampled at `texcoord01.xy`, bypassing the
    // swap tables, the konst/reg selects and the whole stage chain — the
    // texture as authored, for checking a binding or a UV against the source
    // PNG.
    RawTex0 = 6,
    // raw tex1 at texcoord 1.
    // Slot 1 sampled at `texcoord01.zw`: the ZBtoonEX ramp on the lit
    // materials, the hitomi pupil on eyeL/eyeR, and a 1x1 white dummy wherever
    // slot 1 is unreferenced. On a ramp material this shows the ramp already
    // evaluated at the current lighting coordinate — red is the shadow/lit
    // step, green the eflight step.
    RawTex1 = 7,
    // channel recomputed per fragment.
    // The same final TEV rgb as FinalTev, but with the color channel and the
    // texgens re-evaluated from the interpolated normal instead of
    // interpolating the vertex results. A/B against mode 0 isolates GX's
    // per-vertex lighting: the difference is how cleanly the toon band edge
    // crosses a large triangle (phase_08 decision 4).
    ChannelPerPixel = 8,
    // texgen matrices forced to identity.
    // Also the final TEV rgb, with `evalTexGens` told to use raw uv0 in place
    // of any composed MTX2x4 result. Only eyeL/eyeR reference a non-identity
    // matrix — TEXMTX1's [-0.05, 0] pupil translate — so this is identical to
    // mode 0 everywhere but the pupils, which makes it an A/B on the sign and
    // magnitude of that offset. The SRTG branch is unaffected.
    IdentityTexMtx = 9,
}

// GXCompare (GXEnum.h:466–473): the alpha test's compare functions, as raw
// MAT3 codes.
enum GXCompare : uint {
    Never = 0,
    Less = 1,
    Equal = 2,
    LessEqual = 3,
    Greater = 4,
    NotEqual = 5,
    GreaterEqual = 6,
    Always = 7,
}

// GXAlphaOp (GXEnum.h:477–480): how the alpha test's two compare results
// combine.
enum GXAlphaOp : uint {
    And = 0,
    Or = 1,
    Xor = 2,
    Xnor = 3,
}

// GX alpha-compare state, straight from MAT3 via the manifest:
//     keep = op(comp0(alpha, ref0), comp1(alpha, ref1))
struct AlphaCompare {
    GXCompare comp0;
    uint ref0;
    GXCompare comp1;
    uint ref1;
    GXAlphaOp op;
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

    AlphaCompare alphaCompare;
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
bool gxCompare(GXCompare comp, uint alpha, uint reference) {
    switch (comp) {
    case GXCompare.Never: return false;
    case GXCompare.Less: return alpha < reference;
    case GXCompare.Equal: return alpha == reference;
    case GXCompare.LessEqual: return alpha <= reference;
    case GXCompare.Greater: return alpha > reference;
    case GXCompare.NotEqual: return alpha != reference;
    case GXCompare.GreaterEqual: return alpha >= reference;
    default: return true; // Always
    }
}

// Combine the two compare results per GXAlphaOp.
bool gxAlphaOp(GXAlphaOp op, bool r0, bool r1) {
    switch (op) {
    case GXAlphaOp.And: return r0 && r1;
    case GXAlphaOp.Or: return r0 || r1;
    case GXAlphaOp.Xor: return r0 != r1;
    default: return r0 == r1; // Xnor
    }
}

// The GX alpha test, on the TEV output alpha quantized to the 8-bit value the
// hardware compares.
bool gxAlphaTest(AlphaCompare ac, float alpha) {
    let alpha8 = uint(round(saturate(alpha) * 255.0));
    let keep0 = gxCompare(ac.comp0, alpha8, ac.ref0);
    let keep1 = gxCompare(ac.comp1, alpha8, ac.ref1);
    return gxAlphaOp(ac.op, keep0, keep1);
}

// sRGB → linear on one channel. The color target is _SRGB, so the hardware
// applies linear → sRGB *encode* on store; we hold raw GX values that must
// reach the stored bits unchanged, so we apply the *decode* first and the two
// transfers cancel. Applied to RGB only, never alpha.
//
// Verified numerically in P7 (2026-07-27): a flat tunic texel round-trips to the
// source PNG's byte value with 0 LSB error.
float3 srgbDecode(float3 c) {
    return select(c <= 0.04045, c / 12.92, pow((c + 0.055) / 1.055, 2.4));
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
    if (!gxAlphaTest(params.alphaCompare, tevOut.a)) {
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
    default:
        // final TEV output (FinalTev, ChannelPerPixel and IdentityTexMtx)
        color = tevOut.rgb;
        break;
    }

    // The real TEV alpha, not 1.0: four materials blend with Source_Alpha, so a
    // hardcoded 1.0 would silently turn their alpha blend into an opaque write.
    return float4(srgbDecode(color), tevOut.a);
}
