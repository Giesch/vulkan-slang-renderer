#language slang 2026

module std430_matrices;

// Test: Matrix alignment (column-major, columns are vectors)
// All matrix columns have 16-byte alignment in std430

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<MatrixData> data;
}

struct MatrixData {
    float2x2 a;     // 2 columns of vec2: size 16 (2x8), align 8
    float3x3 b;     // 3 columns of vec3 (padded to vec4): size 48 (3x16), align 16
    float4x4 c;     // 4 columns of vec4: size 64 (4x16), align 16
    float d;        // scalar after matrices
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return mul(params.data[id].c, float4(0.0, 0.0, 0.0, 1.0));
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
