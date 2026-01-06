#language slang 2026

module std430_vec3_padding;

// Test: Vec3 has 16-byte alignment despite only 12 bytes of data
// This is the trickiest std430 edge case
// A scalar following vec3 can fit in the 4-byte gap

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<Vec3Data> data;
}

struct Vec3Data {
    float3 a;   // offset 0, size 12, align 16
    float b;    // offset 12, size 4 (fits in vec3's padding!)
    float3 c;   // offset 16, size 12, align 16
    float d;    // offset 28, size 4 (fits in vec3's padding!)
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.data[id].a, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
