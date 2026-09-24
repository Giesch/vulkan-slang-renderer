#language slang 2026

// The production-shader numerical oracle for skinning.
//
// One thread per synthetic case runs the SAME `skinning.slang` functions the
// two render modes call, and writes every intermediate the CPU test compares:
// the deformed position, the policy normal, the conditioning number, the
// fallback decision and a direct `skinSafeNormalize` of an arbitrary vector
// (the fragment-interpolation cancellation input). `src/skinning.rs` builds
// the fixtures, drives `Renderer::dispatch_readback` and checks the results
// against independent CPU expectations. No formula is duplicated here.

module skinning_oracle_compute;

import mltrs;
import skinning;

// One synthetic vertex. `skinning[i]` holds its influences.
struct OracleCase {
    // xyz: model-space bind position
    float4 position;
    // xyz: model-space bind normal (may be zero)
    float4 normal;
    // xyz: fed straight to skinSafeNormalize, like an interpolated fragment normal
    float4 rawVector;
}

struct OracleOutput {
    // xyz: SkinnedVertex position; w: det(L) of the blended linear part
    float4 position;
    // xyz: SkinnedVertex normal; w: skinConditioning(L)
    float4 normal;
    // xyz: skinSafeNormalize(rawVector); w: length(rawVector)
    float4 safeNormalized;
    // x: 1 when the fallback branch decided the normal; y: the case index
    uint4 flags;
}

struct SkinningOracleParams {
    mltrs::ImmutableAddr<SkinJoint> palette;
    mltrs::ImmutableAddr<VertexSkinning> skinning;
    mltrs::ImmutableAddr<OracleCase> cases;
    uint caseCount;
}

struct SkinningOraclePush {
    mltrs::Addr<OracleOutput> output;
}

ParameterBlock<SkinningOracleParams> params;
[[vk::push_constant]] ConstantBuffer<SkinningOraclePush> pc;

[numthreads(1, 1, 1)]
[shader("compute")]
void computeMain(uint3 id : SV_DispatchThreadID) {
    let i = id.x;
    let outOfRange = i >= params.caseCount;
    if (outOfRange) {
        return;
    }

    let oracleCase = params.cases[i];
    let skin = params.skinning[i];
    let skinned = SkinnedVertex(params.palette, skin, oracleCase.position.xyz, oracleCase.normal.xyz);
    let blended = skinBlendedTransform(params.palette, skin);
    let columns = skinLinearColumns(blended);
    let rawVector = oracleCase.rawVector.xyz;

    pc.output[i].position = float4(skinned.position, skinDeterminant(columns));
    pc.output[i].normal = float4(skinned.normal, skinConditioning(columns));
    pc.output[i].safeNormalized = float4(skinSafeNormalize(rawVector), length(rawVector));
    pc.output[i].flags = uint4(skinNormalUsesFallback(columns) ? 1 : 0, i, 0, 0);
}
