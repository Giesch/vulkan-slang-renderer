#language slang 2026

// Shared GPU skeletal deformation for both Toon Link render modes.
//
// Both `toon_link.shader.slang` (GameCube) and `toon_link_modern.shader.slang`
// (Modern) construct `SkinnedVertex` before projection and before any
// mode-specific shading. `skinning_oracle.compute.slang` imports the same
// functions and runs them over synthetic fixtures; `src/skinning.rs` holds the
// CPU reference and the test that compares the two
// (`just toon_link test-skinning-gpu`).
//
// Deformation
//   Each palette entry is `animated_world * inverse_bind_world` for one joint,
//   in model space. The converter exports bind-baked model-space geometry, so a
//   vertex's deformed position is `sum_i w_i * (M_i * p)`, which is
//   `(sum_i w_i * M_i) * p`. Rigid vertices carry one weight of 1 and use the
//   same path as four-influence vertices.
//
// Matrix orientation
//   The CPU uploads `glam::Mat4` (column-major storage). The shaders are
//   compiled row-major (`mltrs::columnMajor == false`), so the reflected
//   `float4x4` `m` holds the mathematical matrix transposed: Slang row `m[i]`
//   is glam column `i`. Positions therefore transform as `mul(float4(p, 1), m)`,
//   exactly like `mltrs::MVPMatrices::project`, and the three Slang rows
//   `m[0..2].xyz` are the columns of the linear part `L`. The oracle's
//   nonsymmetric-matrix case pins this convention.
//
// Normal transformation for blended joint matrices
//   Let `L` be the blended 3x3 linear part with columns `c0, c1, c2`, and
//   `det = dot(c0, cross(c1, c2))`. The scale-aware conditioning number is
//   the Hadamard ratio
//       cond = det / (|c0| * |c1| * |c2|)        (0 when any column is zero)
//   which is invariant under uniform (and per-column) scaling and lies in
//   [-1, 1]: +1 for a rotation, 0 for a singular blend, negative for a
//   mirrored blend.
//
//   Well-conditioned domain: cond >= SKIN_NORMAL_CONDITION_MIN (1e-3).
//     normal = skinSafeNormalize(cofactor(L) * n)
//     cofactor(L) = det * transpose(inverse(L)) = [c1 x c2 | c2 x c0 | c0 x c1],
//     so the direction equals the inverse-transpose normal (det > 0 inside the
//     domain) with no division and no explicit inverse. The Rust oracle checks
//     this against an independent glam inverse-transpose reference with an
//     angular tolerance of 1e-3 rad.
//
//   Fallback (cond < 1e-3, which includes singular, near-singular and
//   mirrored blends):
//     normal = skinSafeNormalize(L * n)
//     The linear part applied directly (the old `rotateDirection` convention).
//     It is exact when the blend happens to be a rotation, finite for every
//     input, free of cancellation-prone products, and the authoritative
//     expected value outside the domain. A rank-2 flattening can map the
//     normal to zero; that is a documented degenerate result, not an error.
//
//   Translation never reaches the normal path (only `L` is used).
//
// Safe normalization
//   `skinSafeNormalize` returns the unit vector when |v|^2 >=
//   SKIN_SAFE_NORMALIZE_MIN_LENGTH_SQ (1e-12, i.e. |v| >= 1e-6) and exactly
//   `float3(0)` otherwise. Every stage that normalizes a normal uses it: this
//   helper, tev.slang's raster lighting, the GameCube WorldNormals and
//   ChannelPerPixel paths, and the Modern fragment shader. Fragment
//   interpolation can cancel nonzero vertex normals to exactly zero; a zero
//   normal shades as unlit (every dot product is 0) and never as NaN.

module skinning;

import mltrs;

// One entry of the joint palette: `animated_world * inverse_bind_world`.
// std430, 64 bytes; pointee of `mltrs::ImmutableAddr<SkinJoint>`.
public struct SkinJoint {
    public float4x4 transform;
}

// One vertex's four influences, indexed by the shared-buffer vertex index
// (`SV_VertexID`; every indirect command uses `vertex_offset = 0`).
// Unused slots carry weight 0. The CPU validates joint bounds and the weight
// sum before upload; the shader does not bounds-check.
// std430, 32 bytes; pointee of `mltrs::ImmutableAddr<VertexSkinning>`.
public struct VertexSkinning {
    public uint4 joints;
    public float4 weights;
}

public struct SkinnedVertex {
    public float3 position;
    // Unit length, or exactly zero under the policy above.
    public float3 normal;

    // Deform one model-space bind vertex. The result stays in model space; the
    // caller applies the model/view/projection matrices afterwards.
    public __init(
        mltrs::ImmutableAddr<SkinJoint> palette,
        VertexSkinning skin,
        float3 position,
        float3 normal
    ) {
        let blended = skinBlendedTransform(palette, skin);
        let columns = skinLinearColumns(blended);

        this.position = skinTransformPoint(blended, position);
        this.normal = skinNormal(columns, normal);
    }

    private __init() {}
}

public static const float SKIN_NORMAL_CONDITION_MIN = 1e-3;
public static const float SKIN_SAFE_NORMALIZE_MIN_LENGTH_SQ = 1e-12;

// Normalize, or return exactly zero for a zero / near-zero vector.
public float3 skinSafeNormalize(float3 vector) {
    let lengthSq = dot(vector, vector);
    let nearZero = lengthSq < SKIN_SAFE_NORMALIZE_MIN_LENGTH_SQ;
    if (nearZero) {
        return float3(0.0);
    }

    return vector * rsqrt(lengthSq);
}

// The weighted sum of the four influences' palette matrices.
public float4x4 skinBlendedTransform(mltrs::ImmutableAddr<SkinJoint> palette, VertexSkinning skin) {
    var blended = palette[skin.joints.x].transform * skin.weights.x;
    blended += palette[skin.joints.y].transform * skin.weights.y;
    blended += palette[skin.joints.z].transform * skin.weights.z;
    blended += palette[skin.joints.w].transform * skin.weights.w;

    return blended;
}

// `M * p` for a reflected matrix (see "Matrix orientation" above).
//
// `mltrs::columnMajor` is module-internal, so this assumes the engine's fixed
// row-major compile (`MATRIX_LAYOUT` in crates/slang-reflection/src/lib.rs;
// `mltrs::MVPMatrices::project` takes the same branch). The oracle's
// nonsymmetric-matrix case fails if that convention ever changes.
float3 skinTransformPoint(float4x4 transform, float3 point) {
    return mul(float4(point, 1.0), transform).xyz;
}

// The columns of the linear part `L`, as Slang rows (row-major compile).
public float3x3 skinLinearColumns(float4x4 transform) {
    return float3x3(transform[0].xyz, transform[1].xyz, transform[2].xyz);
}

// `L * n` where `columns` holds the columns of `L`.
float3 skinApplyLinear(float3x3 columns, float3 normal) {
    return columns[0] * normal.x + columns[1] * normal.y + columns[2] * normal.z;
}

public float skinDeterminant(float3x3 columns) {
    return dot(columns[0], cross(columns[1], columns[2]));
}

// The Hadamard ratio `det / (|c0| |c1| |c2|)`; 0 when any column is zero.
public float skinConditioning(float3x3 columns) {
    let det = skinDeterminant(columns);
    let scale = length(columns[0]) * length(columns[1]) * length(columns[2]);

    // Written so that a NaN scale also takes the early return.
    let zeroColumn = !(scale > 0.0);
    if (zeroColumn) {
        return 0.0;
    }

    return det / scale;
}

public bool skinNormalUsesFallback(float3x3 columns) {
    // Written so that a NaN conditioning number also selects the fallback.
    let wellConditioned = skinConditioning(columns) >= SKIN_NORMAL_CONDITION_MIN;

    return !wellConditioned;
}

// `cofactor(L) * n`: the inverse-transpose direction scaled by det, no division.
float3 skinCofactorNormal(float3x3 columns, float3 normal) {
    return cross(columns[1], columns[2]) * normal.x
        + cross(columns[2], columns[0]) * normal.y
        + cross(columns[0], columns[1]) * normal.z;
}

// Transform a normal using the blended linear part `columns`.
float3 skinNormal(float3x3 columns, float3 normal) {
    let useFallback = skinNormalUsesFallback(columns);
    if (useFallback) {
        return skinSafeNormalize(skinApplyLinear(columns, normal));
    }

    return skinSafeNormalize(skinCofactorNormal(columns, normal));
}
