#language slang 2026

module pointer_dual_context;

// Test: one struct accessed through two pointer fields with different access
// modes in the same parameter block. Both contexts must produce the identical
// Rust struct (std430); divergence panics in codegen. Also pins Addr<T> and
// ReadAddr<T> coexisting in one param block.

struct DualData {
    float3 pos;     // 0
    float w;        // 12
    float4 color;   // 16
}                   // size 32

struct Params {
    LayoutPtr<DualData, Std430DataLayout> by_pointer;                          // Addr<DualData>
    Ptr<DualData, Access.Read, AddressSpace.Device, Std430DataLayout> by_read; // ReadAddr<DualData>
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    DualData a = params.by_pointer[id];
    DualData b = params.by_read[id];
    return a.color + b.color + float4(a.pos + b.pos, a.w + b.w);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
