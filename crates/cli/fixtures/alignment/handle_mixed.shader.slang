#language slang 2026

module handle_mixed;

// Test: a handle sharing a std140 block with the other 8-byte field kind (a
// BDA pointer) and with fields that force padding around it. A handle is
// 8-aligned like a pointer, so a lone float before one leaves a 4-byte gap, and
// a float4 after one leaves another — the two gaps are what pin the width.

struct Item {
    float4 value;   // 0
}                   // size 16

struct Params {
    float scale;                                // 0
    Sampler2D.Handle tex;                       // 8  (4-byte gap after scale)
    LayoutPtr<Item, Std430DataLayout> items;    // 16
    Sampler2D.Handle mask;                      // 24
    float4 tint;                                // 32
    float2 offset;                              // 48
}                                               // size 64

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return params.items[id].value * params.scale + float4(params.offset, 0.0, 0.0);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    let uv = float2(0.5);
    return params.tex.Sample(uv) * params.mask.Sample(uv) * params.tint;
}
