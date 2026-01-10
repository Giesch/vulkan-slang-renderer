#language slang 2026

module projection;

// provided by the cpu; see shaders::MATRIX_LAYOUT
extern static const bool columnMajor;

// a wrapped 4x4 matrix that handles column/row -major matrix layout
public struct Projection {
    float4x4 matrix;

    [ForceInline]
    public func project(position: float4) -> float4 {
        // see the comments in mvp.slang
        return columnMajor ? mul(matrix, position) : mul(position, matrix);
    }
}
