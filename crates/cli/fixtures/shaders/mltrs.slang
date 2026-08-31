#language slang 2026

module mltrs;

// The engine's slang module: `import mltrs;` brings in every declaration
// below, all under `namespace mltrs` (eg. `mltrs::MVPMatrices`).
//
// Note the namespace is ergonomics, not isolation: reflection records type
// names unqualified into a flat map, so every public struct/enum name still
// has to be unique across all of shaders/source/.
namespace mltrs {

// provided by the cpu; see shaders::MATRIX_LAYOUT
extern static const bool columnMajor;

// ---------------------------------------------------------------- addr

// A buffer device address pointing at std430-laid-out Ts.
//
// matches Rust's Addr<T> (src/renderer/addr.rs)
public typealias Addr<T> =
    Ptr<T, Access.ReadWrite, AddressSpace.Device, Std430DataLayout>;

// Read-only counterpart of Addr<T>; writes through it are compile errors.
// This is distinct from ImmutableAddr<T>;
// ReadAddr only promises that *this shader* cannot write through *this pointer*
//
// matches Rust's ReadAddr<T> (src/renderer/addr.rs).
public typealias ReadAddr<T> =
    Ptr<T, Access.Read,      AddressSpace.Device, Std430DataLayout>;

// A pointer to a buffer that the GPU never writes to
// The CPU may still update the buffer between frames
//
// emits SPIR-V Restrict, so it must not change during a shader execution (UB otherwise)
//
// matches Rust's ImmutableAddr<T> (src/renderer/addr.rs)
public typealias ImmutableAddr<T> =
    Ptr<T, Access.Immutable, AddressSpace.Device, Std430DataLayout>;

// ------------------------------------------------- fullscreen_triangle

// Coordinate conventions are documented in docs/coordinates.md.
public struct FullscreenPosition {
    // [-1, 1] clip space, Y-up
    public float4 svPosition;
    // [0, 1] texture space, Y-down (0 is the top row)
    public float2 texCoord;
}

// a screen-covering triangle in [0, 1] Y-up coordinates, counter-clockwise
static const float2 fullScreenTriangleVerts[3] = {
    {0.0, 0.0},
    {2.0, 0.0},
    {0.0, 2.0},
};

// for use with exactly 3 vertices
public FullscreenPosition fullscreenPosition(uint svVertexId) {
    let v = fullScreenTriangleVerts[svVertexId];
    let svPosition = float4(2 * v - 1.0, 0.0, 1.0);
    let texCoord = float2(v.x, 1.0 - v.y);

    return FullscreenPosition(svPosition, texCoord);
}

// ----------------------------------------------------------------- mvp

public struct MVPMatrices {
    float4x4 model;
    float4x4 view;
    float4x4 proj;

    [ForceInline]
    public func project(position: float3) -> float4 {
        let position = float4(position, 1.0);

        if (columnMajor) {
            let mvpMatrix = mul(this.proj, mul(this.view, this.model));
            return mul(mvpMatrix, position);
        }

        // GLM & glam use column-major matrices,
        // but D3D12 and Slang use row-major matrices by default.
        //
        // We can either transpose the component matrices,
        // or multiply everything backwards.
        let mvpMatrix = mul(this.model, mul(this.view, this.proj));
        return mul(position, mvpMatrix);
    }

    // Rotate a direction (e.g. a normal) into world space by the model
    // matrix, ignoring its translation (w = 0). Correct for rotation and
    // uniform scale; a non-uniform scale would need the inverse-transpose
    // of the model matrix instead.
    [ForceInline]
    public func rotateDirection(direction: float3) -> float3 {
        let d = float4(direction, 0.0);

        if (columnMajor) {
            return mul(this.model, d).xyz;
        }

        return mul(d, this.model).xyz;
    }
};

// ---------------------------------------------------------- projection

// a wrapped 4x4 matrix that handles column/row -major matrix layout
public struct Projection {
    float4x4 matrix;

    [ForceInline]
    public func project(position: float4) -> float4 {
        // see the comments in MVPMatrices.project
        return columnMajor ? mul(matrix, position) : mul(position, matrix);
    }
}

// -------------------------------------------------------- super_sample

public interface ISuperSampler {
    float3 sampleAtOffset(float2 subPixelOffset);
}

static const int sides = 3;

[ForceInline]
public float3 superSample<S : ISuperSampler>(S sampler) {
    var sum = float3(0.0);

    [ForceUnroll]
    for (int m = 0; m < sides; m++) {
        [ForceUnroll]
        for (int n = 0; n < sides; n++) {
            let offset = (float2(m, n) + 0.5) / float(sides);
            sum += sampler.sampleAtOffset(offset);
        }
    }

    return sum / float(sides * sides);
}

}
