#language slang 2026

module roc_blocks;

import mltrs;

// Test: Roc render-graph splits for each block shape in one shader. `tint` is
// pure data; `params` mixes float3 data with a pointer and a handle between
// them; `push` is a std430 push block that mixes a pointer, a storage handle,
// and tail-packed data. The pointees are the std430 float3 shapes.
// Expected offsets in comments.

struct PairVertex {
    float3 position; // 0
    float3 color;    // 16
}                    // size 32

struct TailVertex {
    float3 position; // 0
    float scale;     // 12
}                    // size 16

struct WideVertex {
    float3 position; // 0
    float4 color;    // 16
}                    // size 32

struct Tint {
    float4 color; // 0
}                 // size 16

struct BlockParams {
    float3 offset;                          // 0
    mltrs::ImmutableAddr<PairVertex> pairs; // 16
    float3 scale;                           // 32
    float bias;                             // 44
    Sampler2D.Handle tex;                   // 48
    mltrs::ReadAddr<TailVertex> tails;      // 56
}                                           // size 64

struct BlockPush {
    mltrs::Addr<WideVertex> wides;     // 0
    RWTexture2D<float4>.Handle target; // 8
    float3 tint;                       // 16
    float strength;                    // 28
}                                      // size 32

ParameterBlock<Tint> tint;
ParameterBlock<BlockParams> params;
[[vk::push_constant]] ConstantBuffer<BlockPush> push;

[shader("vertex")]
float4 vertexMain(uint vertexId : SV_VertexID) : SV_Position {
    let pair = params.pairs[vertexId];
    let tail = params.tails[vertexId];
    let wide = push.wides[vertexId];
    let position = pair.position * params.scale + tail.position * tail.scale
        + wide.position + params.offset;

    return float4(position + pair.color, wide.color.a + params.bias);
}

[shader("fragment")]
float4 fragmentMain() : SV_Target {
    let color = params.tex.Sample(float2(0.5)) * tint.color
        * float4(push.tint, push.strength);
    push.target[int2(0, 0)] = color;

    return color;
}
