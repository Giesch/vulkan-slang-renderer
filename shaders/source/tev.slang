#language slang 2026

// The GX TEV pipeline as a data-driven interpreter: one `TevParams` uniform per
// material, built by `src/tev_pack.rs` from the manifest's raw GX bytes. Nothing
// here is specialized to Link — the converter's `tev_ir.rs` gate is what
// guarantees the incoming configuration stays inside what this file implements.
//
// Evaluation order matches the hardware: the XF unit computes the color channel
// and the texture coordinates per *vertex*, then the TEV unit runs the stage
// chain per *fragment*. That split is deliberate (phase_08 decision 4) — running
// the channel per-fragment gives rounder, smoother toon bands than GX produces,
// so `toon_link`'s debug mode 8 exists to A/B it.
//
// Only `TevParams` is a struct here, because `reflect_shared_module_types`
// (src/shaders.rs) keys a *global* type-name map by struct name with silent
// last-write-wins, so every public struct name has to be unique across all of
// shaders/source/. Helpers are functions returning builtins, which cannot
// collide.
//
// Plan: llm_notes/link_rendering/phase_08.md Step 2.

module tev;

// --- GX constants -----------------------------------------------------------

// GXTexCoordID / GXTexMapID / GXChannelID all use 0xFF for "none".
static const uint GX_NULL_ID = 0xFF;

// GXTexMtx: TEXMTXn = 30 + 3n, GX_IDENTITY = 60. The packer keeps the raw code
// so the value is readable in a debugger and against mat3_dump.txt; only the
// identity test needs the number.
static const uint GX_IDENTITY_MTX = 60;

// GXTexGenType
static const uint GX_TG_MTX2x4 = 1;
static const uint GX_TG_SRTG = 10;

// TEV intermediates live in signed 10-bit registers. GXSetTevColorOp's clamp
// flag chooses between clamping to [0,1] and clamping to that register range --
// clearing the bit does NOT mean "no clamp", which is the easy mistake here.
//
// UNMEASURED (master plan risk #6): this range is reasoned from the GX docs and
// matches noclip, but P8 deliberately skipped the software-renderer capture that
// would confirm the exact edge behavior. Practical exposure is tiny: the only
// unclamped stages in cl.bdl are eyeL/eyeR stage 1, whose values stay in [0,1]
// anyway.
static const float TEV_S10_MIN = -1024.0 / 255.0;
static const float TEV_S10_MAX = 1023.0 / 255.0;

// --- small helpers ----------------------------------------------------------

[ForceInline]
float3 tevClamp3(float3 v, bool clampBit) {
    if (clampBit) {
        return saturate(v);
    }
    return clamp(v, float3(TEV_S10_MIN), float3(TEV_S10_MAX));
}

[ForceInline]
float tevClamp1(float v, bool clampBit) {
    if (clampBit) {
        return saturate(v);
    }
    return clamp(v, TEV_S10_MIN, TEV_S10_MAX);
}

// GXTevColorChan: R 0, G 1, B 2, A 3. Masked so a junk selector cannot index
// out of bounds.
[ForceInline]
float tevChannelOf(float4 c, uint chan) {
    float ch[4] = { c.r, c.g, c.b, c.a };
    return ch[chan & 3];
}

// GXTevBias: ZERO 0, ADDHALF 1, SUBHALF 2. The 0x3 "compare mode" marker is
// gate-rejected, so it falls through to no bias rather than inventing behavior.
[ForceInline]
float tevBiasValue(uint bias) {
    switch (bias) {
    case 1: return 0.5;
    case 2: return -0.5;
    default: return 0.0;
    }
}

// GXTevScale: SCALE_1 0, SCALE_2 1, SCALE_4 2, DIVIDE_2 3.
[ForceInline]
float tevScaleValue(uint scale) {
    switch (scale) {
    case 1: return 2.0;
    case 2: return 4.0;
    case 3: return 0.5;
    default: return 1.0;
    }
}

// One TEV swap table applied to a color: 4 independent channel selects.
// Branchless, and masked for the same reason as tevChannelOf.
[ForceInline]
float4 tevSwap(float4 c, uint4 table) {
    float ch[4] = { c.r, c.g, c.b, c.a };
    return float4(ch[table.x & 3], ch[table.y & 3], ch[table.z & 3], ch[table.w & 3]);
}

// GXDiffuseFn: None_ 0 (unlit surfaces still get the light's full contribution),
// Signed 1 (may go negative, which GX allows), Clamp 2.
[ForceInline]
float tevDiffuse(uint fn, float3 n, float3 l) {
    switch (fn) {
    case 1: return dot(n, l);
    case 2: return max(dot(n, l), 0.0);
    default: return 1.0;
    }
}

// The combiner, for color and for alpha:
//     out = clamp?( ((d +/- lerp(a, b, c)) + bias) * scale )
// `op` is (op, bias, scale, clamp). GXTevOp >= 8 are the comparison modes, which
// reinterpret a/b/c/d entirely; tev_ir.rs rejects them, so they degrade to ADD
// here rather than being silently half-implemented.
[ForceInline]
float3 tevCombine3(float3 a, float3 b, float3 c, float3 d, uint4 op) {
    float3 mixed = lerp(a, b, c);
    float3 v = (op.x == 1) ? (d - mixed) : (d + mixed);
    v = (v + tevBiasValue(op.y)) * tevScaleValue(op.z);
    return tevClamp3(v, op.w != 0);
}

[ForceInline]
float tevCombine1(float a, float b, float c, float d, uint4 op) {
    float mixed = lerp(a, b, c);
    float v = (op.x == 1) ? (d - mixed) : (d + mixed);
    v = (v + tevBiasValue(op.y)) * tevScaleValue(op.z);
    return tevClamp1(v, op.w != 0);
}

// GXTevKColorSel, in full. Bit structure:
//   < 0x08          the eight constant fractions, (8 - sel)/8, so 0x00 is 1.0
//                   and 0x07 is 1/8
//   0x0C .. 0x0F    K0..K3 rgb
//   >= 0x10         a single-channel splat of K{sel & 3}, channel (sel-0x10)>>2
//                   (R at 0x10, G at 0x14, B at 0x18, A at 0x1C)
// 0x08..0x0B is a hole in the GX enum, so KonstColorSel::try_from already
// rejects it at MAT3 parse time and that branch is unreachable.
[ForceInline]
float3 tevKonstColor(uint sel, float4 konst[4]) {
    if (sel < 0x08) {
        return float3(float(8 - sel) * 0.125);
    }
    if (sel < 0x0C) {
        return float3(0.0);
    }
    if (sel < 0x10) {
        return konst[sel - 0x0C].rgb;
    }
    return float3(tevChannelOf(konst[sel & 3], (sel - 0x10) >> 2));
}

// GXTevKAlphaSel: same fractions, same splats, but no flat K0..K3 form -- an
// alpha cannot take three channels.
[ForceInline]
float tevKonstAlpha(uint sel, float4 konst[4]) {
    if (sel < 0x08) {
        return float(8 - sel) * 0.125;
    }
    if (sel < 0x10) {
        return 0.0;
    }
    return tevChannelOf(konst[sel & 3], (sel - 0x10) >> 2);
}

// All 16 GXTevColorArg selectors. The first eight are exactly regs[sel >> 1],
// taking .rgb for the even (color) half and .aaa for the odd (alpha) half:
// CPREV/APREV -> regs[0], C0/A0 -> regs[1], C1/A1 -> regs[2], C2/A2 -> regs[3].
[ForceInline]
float3 tevColorArg(uint sel, float4 regs[4], float4 texel, float4 ras, float3 konstColor) {
    if (sel < 8) {
        float4 r = regs[sel >> 1];
        return ((sel & 1) == 0) ? r.rgb : float3(r.a);
    }
    switch (sel) {
    case 0x8: return texel.rgb;       // TEXC
    case 0x9: return float3(texel.a); // TEXA
    case 0xA: return ras.rgb;         // RASC
    case 0xB: return float3(ras.a);   // RASA
    case 0xC: return float3(1.0);     // ONE
    case 0xD: return float3(0.5);     // HALF
    case 0xE: return konstColor;      // KONST
    default: return float3(0.0);      // ZERO (0xF)
    }
}

// All 8 GXTevAlphaArg selectors: APREV/A0/A1/A2 are regs[sel].a directly.
[ForceInline]
float tevAlphaArg(uint sel, float4 regs[4], float texelA, float rasA, float konstAlpha) {
    if (sel < 4) {
        return regs[sel].a;
    }
    switch (sel) {
    case 4: return texelA;     // TEXA
    case 5: return rasA;       // RASA
    case 6: return konstAlpha; // KONST
    default: return 0.0;       // ZERO (7)
    }
}

// Dynamic sampler indexing does not exist: tex0/tex1 are two distinct
// ParameterBlock fields, so the selector has to be a branch. It branches on a
// *uniform* value, which keeps the implicit-LOD `Sample` well defined because
// every fragment in the quad takes the same path.
//
// A null texmap yields GX's "no texture" identity rather than reading out of
// bounds. Any stage that actually selects TEXC/TEXA with a null texmap is
// gate-rejected, so this only guards stages that never look.
[ForceInline]
public float4 tevSampleTexmap(uint texmap, float2 uv, Sampler2D tex0, Sampler2D tex1) {
    if (texmap == 0) {
        return tex0.Sample(uv);
    }
    if (texmap == 1) {
        return tex1.Sample(uv);
    }
    return float4(1.0);
}

// GXChannelID -> the rasterized color. Only channel pair 0 exists (the gate
// enforces num_color_chans == 1), so COLOR0 / ALPHA0 / COLOR0A0 all resolve to
// the one interpolant. COLOR_ZERO and COLOR_NULL are GX zero; the COLOR1 forms
// are gate-rejected.
[ForceInline]
public float4 tevRasColor(uint chanId, float4 color0) {
    switch (chanId) {
    case 0:            // COLOR0
    case 2:            // ALPHA0
    case 4: return color0; // COLOR0A0
    default: return float4(0.0);
    }
}

// --- the uniform block ------------------------------------------------------

// One GX value per vector component, no bit-packing anywhere (phase_08
// decision 3): the whole risk of this phase is misencoding an interpreter's
// configuration, and a packed field is unreadable both in a debugger and against
// mat3_dump.txt. ~1.3 KB against a 16 KB guaranteed maxUniformBufferRange.
public struct TevParams {
    // Per-stage combiner configuration, indexed 0..control.x.
    uint4 stageColorIn[8];  // a, b, c, d           GXTevColorArg
    uint4 stageColorOp[8];  // op, bias, scale, clamp
    uint4 stageAlphaIn[8];  // a, b, c, d           GXTevAlphaArg
    uint4 stageAlphaOp[8];  // op, bias, scale, clamp
    uint4 stageDest[8];     // colorReg, alphaReg, kcsel, kasel
    uint4 stageOrder[8];    // texcoord, texmap, rasChannel, 0   (0xFF = null)
    uint4 stageSwap[8];     // rasSel, texSel, 0, 0

    // GX_TEV_SWAP0..3: four channel selects (r, g, b, a) each. Absent slots are
    // filled with the identity by the packer.
    uint4 swapTable[4];

    // Per-texgen: type, source, raw GX matrix code (60 = GX_IDENTITY), 0.
    uint4 texgen[2];
    // Two composed MTX2x4 rows per texgen, laid out for
    // dot(row, float4(uv, 1, 1)) so the translation sits in .z. Stored per
    // *texgen* rather than per texmatrix slot: slots run TEXMTX0..TEXMTX9, so a
    // slot-indexed array would need ten entries to address two.
    float4 texgenMtx[4];

    float4 konst[4]; // K0..K3, /255
    // PREV, REG0, REG1, REG2 -- in *register* order, which is NOT the manifest's
    // order. reg_colors[i] loads register i+1 (J3DMatBlock.cpp's loadTevColor),
    // so reg[1..=3] come from reg_colors[0..=2] and reg[0] (PREV) gets no MAT3
    // value. Reading it unshifted makes stage 0 of every toon material a no-op
    // and the bands vanish with no other symptom.
    float4 reg[4];

    // Unit vectors pointing FROM the surface TOWARD each light, world space, and
    // their colors. The manifest's light_colors is null on every material (the
    // game writes them per frame from dKy_tevstr_c), so both come from the
    // example. Two lights because lit_mask is 3; widening means resizing these
    // and relaxing the gate.
    //
    // The colors are single-channel by design, not by simplification: the game's
    // light 0 is red-only and its light 1 green-only, which is what lets the
    // separable ZBtoonEX ramp read two independent terms off one channel color.
    // See examples/toon_link.rs's LIGHT0_COLOR.
    float4 lightDir[2];
    float4 lightColor[2];

    // [0] = GX_COLOR0, [1] = GX_ALPHA0: (lightingEnabled, diffuseFn, attnFn,
    // litMask). MAT3's four color_channels slots are pairs, which is why one
    // channel pair means two live controls.
    uint4 chanControl[2];
    // One RGBA register per channel *pair*, matching
    // GXSetChanMatColor(GX_COLOR0A0, …): the color channel takes .rgb and the
    // alpha channel takes .a.
    float4 chanMatColor;
    float4 chanAmbColor;

    uint4 control; // numStages, numTexgens, numChanPairs, 0

    // One GX color channel. Returns the whole float4 even though the caller only
    // wants .rgb from slot 0 and .a from slot 1, because both halves run the same
    // math on the same registers.
    //
    // Attenuation is forced to 1.0 for Spot and Specular alike. That started as
    // a deliberate approximation -- our light is a directional with no position,
    // so there is nothing to attenuate from -- but it turns out to be exact:
    // the game sets every actor light's coefficients to the identity,
    // mCosAtten = (1,0,0) and mDistAtten = (1,0,0)
    // (../tww/src/d/d_kankyo.cpp:1548-1553 and :3413-3418), which makes GX's
    // attenuation term 1 regardless of geometry.
    public func evalChannel(uint slot, float3 normal) -> float4 {
        let chan = this.chanControl[slot];
        let matColor = this.chanMatColor;
        if (chan.x == 0) {
            return matColor;
        }

        let n = normalize(normal);
        float3 illumRgb = this.chanAmbColor.rgb;
        float illumA = this.chanAmbColor.a;

        for (uint l = 0; l < 2; l++) {
            if ((chan.w & (1u << l)) == 0) {
                continue;
            }
            let d = tevDiffuse(chan.y, n, normalize(this.lightDir[l].xyz));
            illumRgb += d * this.lightColor[l].rgb;
            illumA += d * this.lightColor[l].a;
        }
        return float4(matColor.rgb * saturate(illumRgb), matColor.a * saturate(illumA));
    }

    // The rasterized color TEV sees as RASC/RASA: COLOR0's rgb paired with
    // ALPHA0's alpha.
    public func evalRasterColor(float3 normal) -> float4 {
        let color0 = this.evalChannel(0, normal);
        let alpha0 = this.evalChannel(1, normal);
        return float4(color0.rgb, alpha0.a);
    }

    // Both texcoords, packed as (uv0, uv1) to match FragVertex.texcoord01.
    // `forceIdentityMtx` is the example's debug mode 9: it makes the pupil's
    // TEXMTX1 offset a toggle instead of something to be assumed.
    public func evalTexGens(float2 uvIn, float4 chanColor, bool forceIdentityMtx) -> float4 {
        float2 uv[2] = { float2(0.0), float2(0.0) };
        let count = min(this.control.y, 2);

        for (uint i = 0; i < count; i++) {
            let g = this.texgen[i];
            if (g.x == GX_TG_SRTG) {
                // The texcoord *is* the rasterized channel color's (r, g). This
                // is how the ZBtoonEX ramp gets indexed, and the ramp happens to
                // be separable -- its R varies only with u and its G only with
                // v, both stepping sharply at ~0.49 -- so stage 0's RRR swizzle
                // reads f(color.r) and stage 2's GGG swizzle reads g(color.g)
                // independently. Sampler wrap is ClampToEdge, so a channel value
                // outside [0,1] is harmless.
                uv[i] = chanColor.rg;
            } else {
                // GX_TG_MTX2x4 from TEX0.
                if (g.z == GX_IDENTITY_MTX || forceIdentityMtx) {
                    uv[i] = uvIn;
                } else {
                    let src = float4(uvIn, 1.0, 1.0);
                    uv[i] = float2(dot(this.texgenMtx[2 * i], src),
                                   dot(this.texgenMtx[2 * i + 1], src));
                }
            }
        }
        return float4(uv[0], uv[1]);
    }

    // A texcoord by GX id; null (or past numTexgens) samples at the origin.
    [ForceInline]
    public func texcoordFor(uint coordId, float4 texcoord01) -> float2 {
        if (coordId == 0) {
            return texcoord01.xy;
        }
        if (coordId == 1) {
            return texcoord01.zw;
        }
        return float2(0.0);
    }

    // The stage chain. Returns GX_TEVPREV, which is the pixel.
    public func evalStages(float4 color0, float4 texcoord01, Sampler2D tex0, Sampler2D tex1)
        -> float4 {
        // reg[0] is PREV. GX leaves it undefined across materials and MAT3 never
        // loads it, so the packer parks it at 0 -- the only reproducible choice,
        // and the one noclip makes.
        float4 regs[4] = { this.reg[0], this.reg[1], this.reg[2], this.reg[3] };
        let count = min(this.control.x, 8);

        for (uint i = 0; i < count; i++) {
            let order = this.stageOrder[i];
            let swap = this.stageSwap[i];
            let dest = this.stageDest[i];

            let uv = this.texcoordFor(order.x, texcoord01);
            let texel = tevSwap(tevSampleTexmap(order.y, uv, tex0, tex1),
                                this.swapTable[swap.y & 3]);
            let ras = tevSwap(tevRasColor(order.z, color0), this.swapTable[swap.x & 3]);

            let konstColor = tevKonstColor(dest.z, this.konst);
            let konstAlpha = tevKonstAlpha(dest.w, this.konst);

            let ci = this.stageColorIn[i];
            let ai = this.stageAlphaIn[i];

            // BOTH halves are computed from the pre-write register file: GX reads
            // every input at stage start, so writing the color before computing
            // the alpha would corrupt a stage whose alpha reads the register its
            // color just overwrote.
            let outColor = tevCombine3(
                tevColorArg(ci.x, regs, texel, ras, konstColor),
                tevColorArg(ci.y, regs, texel, ras, konstColor),
                tevColorArg(ci.z, regs, texel, ras, konstColor),
                tevColorArg(ci.w, regs, texel, ras, konstColor),
                this.stageColorOp[i]);
            let outAlpha = tevCombine1(
                tevAlphaArg(ai.x, regs, texel.a, ras.a, konstAlpha),
                tevAlphaArg(ai.y, regs, texel.a, ras.a, konstAlpha),
                tevAlphaArg(ai.z, regs, texel.a, ras.a, konstAlpha),
                tevAlphaArg(ai.w, regs, texel.a, ras.a, konstAlpha),
                this.stageAlphaOp[i]);

            // Color and alpha go to independent GX registers. Written through
            // full-vector stores rather than swizzle assignment so a dynamic
            // index needs no lvalue-swizzle support; the alpha store reads back
            // the color store, which is correct when both name the same register.
            let colorReg = dest.x & 3;
            regs[colorReg] = float4(outColor, regs[colorReg].a);
            let alphaReg = dest.y & 3;
            regs[alphaReg] = float4(regs[alphaReg].rgb, outAlpha);
        }
        return regs[0];
    }
};
