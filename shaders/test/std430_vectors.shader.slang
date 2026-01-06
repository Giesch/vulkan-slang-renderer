#language slang 2026

module std430_vectors;

// Test: Vector type alignment
// Vec2 = 8-byte alignment, Vec4 = 16-byte alignment
// Expected: Padding inserted between Vec2 and Vec4

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<VectorData> data;
}

struct VectorData {
    float2 a;   // offset 0, size 8, align 8
    float4 b;   // offset 16 (needs 8-byte padding), size 16, align 16
    float2 c;   // offset 32, size 8, align 8
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.data[id].a, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
