#language slang 2026

module std140_nested_structs;

// Test: Nested struct alignment in uniform buffer (std140)
// In std140, nested structs must have 16-byte alignment (unlike std430 which uses max member alignment)

ParameterBlock<OuterData> params;

struct InnerData {
    float a;    // offset 0, size 4, align 4
    float2 b;   // offset 8, size 8, align 8 -> InnerData has align 16 in std140
}

struct OuterData {
    float x;            // offset 0, size 4
    InnerData inner;    // offset 16 (std140 requires 16-byte align for structs), needs 12-byte padding
    float y;            // after inner
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.x, params.inner.a, params.y, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
