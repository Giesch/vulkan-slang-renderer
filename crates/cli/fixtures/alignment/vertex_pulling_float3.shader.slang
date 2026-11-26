#language slang 2026

module vertex_pulling_float3;

// Test: float3-bearing vertex structs pulled through ImmutableAddr<T> pointers
// (Ptr<T, Access.Immutable, AddressSpace.Device, Std430DataLayout>), indexed
// by SV_VertexID. Reading SV_DrawIndex makes this a GraphicsIndirect shader.
// Expected std430 offsets in comments.

struct PairVertex {
    float3 position; // 0
    float3 color;    // 16 (a float3 aligns to 16)
}                    // size 32

struct TailVertex {
    float3 position; // 0
    float scale;     // 12 (tail-packs into the float3's pad)
}                    // size 16

struct WideVertex {
    float3 position; // 0
    float4 color;    // 16
}                    // size 32

struct PullParams {
    Ptr<PairVertex, Access.Immutable, AddressSpace.Device, Std430DataLayout> pairs; // 0
    Ptr<TailVertex, Access.Immutable, AddressSpace.Device, Std430DataLayout> tails; // 8
    Ptr<WideVertex, Access.Immutable, AddressSpace.Device, Std430DataLayout> wides; // 16
}

ParameterBlock<PullParams> params;

[shader("vertex")]
float4 vertMain(uint vertexId : SV_VertexID, uint drawIndex : SV_DrawIndex) : SV_Position {
    let pair = params.pairs[vertexId];
    let tail = params.tails[vertexId];
    let wide = params.wides[drawIndex];

    let position = pair.position + tail.position * tail.scale + wide.position;
    let tint = pair.color + wide.color.rgb;

    return float4(position + tint, wide.color.a);
}

[shader("fragment")]
float4 fragMain() : SV_Target {
    return float4(1.0);
}
