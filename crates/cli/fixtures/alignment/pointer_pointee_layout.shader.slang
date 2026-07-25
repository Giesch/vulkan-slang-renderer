#language slang 2026

module pointer_pointee_layout;

// Test: PhysicalStorageBuffer pointer field (BDA) with a layout-hostile
// pointee. Pointers must use Std430DataLayout (LayoutPtr<T, Std430DataLayout>);
// a default T* pointee uses slang's natural layout, which codegen rejects.
// Expected std430 offsets in comments; natural layout would diverge from the
// first float3 on, std140 would diverge at InnerB.

struct InnerA {
    float x;     // 0
    float3 v;    // 16 (natural layout would put v at 4)
}                // size 32

struct InnerB {
    float2 p;    // 0
    float q;     // 8
}                // size 16, align 8 (std140 would align it 16)

struct HostileData {
    float3 a;    // 0
    float b;     // 12  (tail-packs into float3's pad)
    float c;     // 16  (lone float before a float4)
    float4 d;    // 32  (20 under natural layout)
    InnerA ia;   // 48
    float e;     // 80
    InnerB ib;   // 88  (96 under std140)
    float f;     // 104
}                // total size 112

struct Params {
    float scale;                                    // 0
    LayoutPtr<HostileData, Std430DataLayout> items; // 8
    float2 post;                                    // 16
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    HostileData item = params.items[id];
    float4 acc = item.d * params.scale;
    acc += float4(item.a, item.b);
    acc += float4(item.ia.v, item.ia.x);
    acc += float4(item.ib.p, item.ib.q, item.c);
    acc += float4(params.post, item.e, item.f);
    return acc;
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
