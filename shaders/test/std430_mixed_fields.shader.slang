#language slang 2026

module std430_mixed_fields;

// Test: Complex realistic case with multiple padding gaps
// Various types interleaved to create padding requirements

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<MixedData> data;
}

struct MixedData {
    float a;        // offset 0, size 4, align 4
    float3 b;       // offset 16 (needs 12-byte padding for vec3 align 16), size 12
    float c;        // offset 28, size 4 (fits in vec3 gap)
    float2 d;       // offset 32, size 8, align 8
    uint e;         // offset 40, size 4, align 4
    float4 f;       // offset 48 (needs 4-byte padding for vec4 align 16), size 16
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return params.data[id].f;
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
