#language slang 2026

module std140_vec3_padding;

// Test: Vec3 special case in uniform buffer (std140)
// Vec3 is 16-byte aligned but only uses 12 bytes
// A scalar after vec3 can fit in the 4-byte gap

ParameterBlock<Vec3Data> params;

struct Vec3Data {
    float3 a;   // offset 0, size 12, align 16
    float b;    // offset 12, size 4 (fits in vec3's 4-byte gap)
    float3 c;   // offset 16, size 12, align 16
    float d;    // offset 28, size 4 (fits in vec3's 4-byte gap)
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.a, params.b);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
