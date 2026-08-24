#language slang 2026

module nested_pointer;

// Test: a pointer field inside a Std430DataLayout pointee — a pointer chain.
// The inner pointer is Access.Immutable, so the generated field is
// ImmutableAddr<NestedInner>. Both entry points read through both levels, so
// both PhysicalStorageBuffer pointer types appear in each stage's SPIR-V.

struct NestedInner {
    float3 v;    // 0
    float w;     // 12
}                // size 16, align 16

struct NestedSlot {
    Ptr<NestedInner, Access.Immutable, AddressSpace.Device, Std430DataLayout> item; // 0, size 8
    uint tag;    // 8
}                // size 16, align 8

struct NestedDraw {
    LayoutPtr<NestedSlot, Std430DataLayout> slots; // 0
    uint index;                                    // 8
}

struct NestedParams {
    float4 tint;    // 0
}                   // size 16

ParameterBlock<NestedParams> params;
[[vk::push_constant]] ConstantBuffer<NestedDraw> draw;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    let slot = draw.slots[draw.index + id];
    return float4(slot.item[0].v, slot.item[0].w + float(slot.tag));
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(draw.slots[draw.index].item[0].v, 1.0) * params.tint;
}
