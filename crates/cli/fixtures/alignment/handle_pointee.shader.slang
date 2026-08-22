#language slang 2026

module handle_pointee;

// Test: handles inside a Std430DataLayout pointee — the shape that makes
// bindless worth doing. One pipeline plus a buffer of these replaces one
// pipeline and one uniform buffer per material.

struct Material {
    Sampler2D.Handle albedo;    // 0, size 8
    Sampler2D.Handle normal;    // 8
    float4 tint;                // 16
    float roughness;            // 32
}                               // size 48 (16-byte struct alignment)

struct Params {
    LayoutPtr<Material, Std430DataLayout> materials;    // 0
    uint material_index;                                // 8
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(params.materials[params.material_index].roughness);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    let uv = float2(0.5);
    Material material = params.materials[params.material_index];
    return material.albedo.Sample(uv) * material.normal.Sample(uv) * material.tint;
}
