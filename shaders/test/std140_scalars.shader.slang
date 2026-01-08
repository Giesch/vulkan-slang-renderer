#language slang 2026

module std140_scalars;

// Test: Sequential scalar types in uniform buffer (std140)
// Expected: No padding needed between sequential 4-byte scalars

ParameterBlock<ScalarData> params;

struct ScalarData {
    float a;    // offset 0, size 4, align 4
    uint b;     // offset 4, size 4, align 4
    float c;    // offset 8, size 4, align 4
    uint d;     // offset 12, size 4, align 4
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.a, float(params.b), params.c, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
