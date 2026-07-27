#language slang 2026

module std140_enums;

// Test: enum fields in a uniform buffer (std140)
// Expected: each enum occupies exactly its tag type's bytes, like the
// equivalent scalar field, and codegen emits a Rust enum for each.

// non-zero-based and non-contiguous: the case values have to survive
// reflection, not just the case names
enum Mode : uint {
    Off = 0,
    Sparse = 7,
    Dense = 9,
}

// a bare enum has no tag annotation and reflects as int32
enum Untagged {
    First,
    Second,
}

ParameterBlock<EnumData> params;

struct EnumData {
    float a;        // offset 0, size 4
    Mode mode;      // offset 4, size 4, align 4
    Untagged kind;  // offset 8, size 4, align 4
    float b;        // offset 12, size 4
}

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    return float4(params.a, params.b, 0.0, 1.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    if (params.mode == Mode.Dense && params.kind == Untagged.Second) {
        return float4(0.0);
    }
    return float4(1.0);
}
