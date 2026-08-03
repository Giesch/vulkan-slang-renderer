#language slang 2026

module std140_matrices;

// Test: Matrix alignment in uniform buffer (std140)
// Matrices are column-major with each column as a vector
// Mat4x4: 4 columns of Vec4 = 64 bytes, 16-byte aligned
// Note: Mat3x3 and Mat2x2 are not tested because glam's representations
// don't match GPU layout (glam::Mat3 = 36 bytes, GPU expects 48 bytes)

ParameterBlock<MatrixData> params;

struct MatrixData {
    float4x4 a;     // offset 0, size 64, align 16
    float4x4 b;     // offset 64, size 64, align 16
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return mul(params.a, float4(0.0, 0.0, 0.0, 1.0));
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
