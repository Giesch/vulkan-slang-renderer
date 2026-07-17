#language slang 2026

module std430_matrices;

// Test: Matrix alignment (column-major, columns are vectors)
// Mat4x4: 4 columns of Vec4 = 64 bytes, 16-byte aligned
// Note: Mat3x3 and Mat2x2 are not tested because glam's representations
// don't match GPU layout (glam::Mat3 = 36 contiguous bytes, GPU expects 48
// with 16-byte column stride) — the codegen rejects them with a hard error.

ParameterBlock<Params> params;

struct Params {
    StructuredBuffer<MatrixData> data;
}

struct MatrixData {
    float4x4 c;     // offset 0, size 64, align 16
    float d;        // scalar after matrix, offset 64
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return mul(params.data[id].c, float4(0.0, 0.0, 0.0, 1.0));
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
