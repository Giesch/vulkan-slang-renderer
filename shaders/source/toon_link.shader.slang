#language slang 2026

module toon_link;

import mvp;

ParameterBlock<ToonLinkParams> params;

struct ToonLinkParams {
    // texmap slot 0 — the albedo. Slot 1 is the ZBtoonEX ramp (or the hitomi
    // pupil on eyeL/eyeR): bound now so P8 needs no descriptor-shape change,
    // but not sampled until the TEV interpreter exists.
    Sampler2D tex0;
    Sampler2D tex1;

    MVPMatrices mvp;

    // raw GX codes, straight from MAT3 via the manifest.
    // GXCompare: NEVER 0, LESS 1, EQUAL 2, LEQUAL 3,
    //            GREATER 4, NEQUAL 5, GEQUAL 6, ALWAYS 7
    uint4 alphaCompare;      // comp0, ref0, comp1, ref1
    uint  alphaCompareOp;    // GXAlphaOp: AND 0, OR 1, XOR 2, XNOR 3
    // 0 albedo, 1 world normals, 2 uv0, 3 albedo alpha as gray
    //
    // NOTE: changing this struct's shape while `just dev` runs trips
    // assert_shader_interface_unchanged — body edits hot-reload, shape
    // edits need `just shaders` + a restart.
    uint  debugMode;
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
};

[shader("vertex")]
FragVertex vertMain(Vertex vertex) {
    let position = params.mvp.project(vertex.position);
    // the model matrix is rotation-free uniform scale in P6, so
    // rotateDirection is exact; renormalized per-fragment after interpolation
    let worldNormal = params.mvp.rotateDirection(vertex.normal);
    return FragVertex(position, worldNormal, vertex.uv0);
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
    let texel = params.tex0.Sample(fragVertex.uv0);

    // GX alpha compare, on the 8-bit value the hardware actually compares.
    // Runs in every debug mode, so the cutout stays visible while inspecting
    // normals or UVs.
    let alpha8 = uint(round(saturate(texel.a) * 255.0));
    let keep0 = gxCompare(params.alphaCompare.x, alpha8, params.alphaCompare.y);
    let keep1 = gxCompare(params.alphaCompare.z, alpha8, params.alphaCompare.w);
    if (!gxAlphaOp(params.alphaCompareOp, keep0, keep1)) {
        discard;
    }

    float3 color;
    switch (params.debugMode) {
    case 1:
        // world-space normals as color (the P6 diagnostic)
        color = normalize(fragVertex.normal) * 0.5 + 0.5;
        break;
    case 2:
        // uv0: red = u, green = v
        color = float3(fragVertex.uv0, 0.0);
        break;
    case 3:
        // albedo alpha as grayscale — the cutout mask, before the discard
        color = float3(texel.a);
        break;
    default:
        // albedo (mode 0)
        color = texel.rgb;
        break;
    }

    return float4(srgbDecode(color), 1.0);
}
