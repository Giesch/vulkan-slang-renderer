#language slang 2026

module push_constants;

// Test: a [[vk::push_constant]] block alongside an ordinary ParameterBlock.
//
// The block is laid out std430, not std140, and the nested struct is what
// proves it: std140 rounds a nested struct's alignment *and* size up to 16,
// std430 uses its natural 8. `tail` is what makes the difference observable —
// without it, `model`'s 16-byte alignment would round both rule sets back to
// the same offset and the fixture would pass under either.
//
// The handle is here to pin that a handle living *only* in a push block still
// sets bindlessHeapSet, and both stages read the block, which is what produces
// a single `All`-stage push constant range rather than one per stage.

struct DrawInner {
    float2 v;   // 0
}               // std430: align 8, size 8 | std140: align 16, size 16

struct DrawConstants {
    float scale;                // 0
    Sampler2D.Handle tex;       // 8   (4-byte gap after the scalar)
    DrawInner inner;            // 16
    float tail;                 // std430: 24 | std140: 32
    float4x4 model;             // std430: 32 | std140: 48
}                               // std430: 96 | std140: 112

struct Params {
    float4 tint;    // 0
}                   // size 16

ParameterBlock<Params> params;
[[vk::push_constant]] ConstantBuffer<DrawConstants> draw;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    let local = float4(float(id) * draw.scale + draw.tail, draw.inner.v, 1.0);
    return mul(draw.model, local);
}

// reads every member of the block: push_constant_spirv_layout asserts against
// this stage's module, and an unread member would be free for the optimizer to
// drop, shifting the member indices the test pins
[shader("fragment")]
float4 fragMain() : SV_Target {
    let uv = draw.inner.v * draw.scale + draw.tail;
    return mul(draw.model, draw.tex.Sample(uv)) * params.tint;
}
