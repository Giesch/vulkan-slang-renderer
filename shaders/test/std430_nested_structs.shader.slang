#language slang 2026

module std430_nested_structs;

// Test: Nested struct alignment
// Inner struct alignment is max of its member alignments
// Outer struct must align inner struct appropriately

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<OuterData> data;
}

struct InnerData {
    float a;    // align 4
    float2 b;   // align 8 -> InnerData has align 8
}

struct OuterData {
    float x;            // offset 0, size 4
    InnerData inner;    // offset 8 (InnerData has align 8), needs 4-byte padding
    float y;            // after inner
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.data[id].x, params.data[id].inner.a, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
