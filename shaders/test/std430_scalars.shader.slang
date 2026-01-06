#language slang 2026

module std430_scalars;

// Test: Sequential scalar types with 4-byte alignment
// Expected: No padding needed between fields

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<ScalarData> data;
}

struct ScalarData {
    float a;    // offset 0, size 4, align 4
    uint b;     // offset 4, size 4, align 4
    float c;    // offset 8, size 4, align 4
    uint d;     // offset 12, size 4, align 4
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.data[id].a, 0.0, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
