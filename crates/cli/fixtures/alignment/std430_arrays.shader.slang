#language slang 2026

module std430_arrays;

// Test: float4/uint4/int4 arrays inside a BDA pointee struct (std430), the
// same field mix as std140_arrays. vec4-element array strides are 16 in both
// rule sets, so the offsets match std140_arrays; the codegen's pointee-size
// cross-assert re-checks the computed std430 size against reflection.

struct PtrNested {
    uint4 inner[2];  // offset 0, size 32
    float pad;       // offset 32, size 4 -> struct size rounds to 48
}

struct ArrayPointee {
    float lead;       // offset 0, size 4
    float4 konst[4];  // offset 16, size 64
    uint4 stages[8];  // offset 80, size 128
    float2 wedge;     // offset 208, size 8
    int4 offsets[3];  // offset 224, size 48
    float tail;       // offset 272, size 4
    PtrNested nested; // offset 288, size 48
    uint4 flags;      // offset 336, size 16
    int4 bias;        // offset 352, size 16
}                     // total size 368

struct Params {
    float scale;                                     // offset 0
    LayoutPtr<ArrayPointee, Std430DataLayout> items; // offset 8
}

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id : SV_VertexID) : SV_Position {
    ArrayPointee item = params.items[id];
    float4 acc = item.konst[id % 4] * item.lead * params.scale;
    acc += float4(item.stages[id % 8]);
    acc += float4(item.wedge, item.tail, 1.0);
    acc += float4(item.offsets[id % 3]);
    acc += float4(item.nested.inner[id % 2]) * item.nested.pad;
    acc += float4(item.flags) + float4(item.bias);
    return acc;
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
