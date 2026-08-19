#language slang 2026

module handle_params_compute;

// Covers bindless texture handles in a compute parameter block.
// handle_params.shader.slang covers the graphics side.
//
// Both element types are necessary. The shape gate accepts any `Sampler2D<`
// prefix. Slang puts every combined-image-sampler handle in one heap array at
// one binding, whatever the element type. A `float` handle and a `float4`
// handle are therefore the same 8-byte field in the generated struct.

struct Params {
    Sampler2D.Handle tex;           // 0, size 8
    Sampler2D<float>.Handle mask;   // 8, size 8
    RWTexture2D<float4> outTex;     // a descriptor, not a uniform field
    float2 gridSize;                // 16
}                                   // std140 block rounds to 32

ParameterBlock<Params> params;

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID: SV_DispatchThreadID) {
    let pixel = uint2(dispatchThreadID.xy);
    if (pixel.x >= uint(params.gridSize.x) || pixel.y >= uint(params.gridSize.y)) {
        return;
    }

    // `[]` on a handle does a texel fetch. `.Sample` uses the sampler half of
    // the handle.
    let fetched = params.tex[pixel];
    let masked = params.mask[pixel];
    let sampled = params.tex.Sample(float2(0.5));

    params.outTex[int2(pixel)] = fetched * masked + sampled;
}
