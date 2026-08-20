#language slang 2026

module storage_handle_params_compute;

// Covers bindless storage-image handles in a compute parameter block.
// handle_params.compute.slang covers the combined-image-sampler side.
//
// Both element types are necessary. The shape gate accepts any `RWTexture2D<`
// prefix. Slang puts every storage-image handle in one heap array at one
// binding, whatever the element type. A `float` handle and a `float4` handle
// are therefore the same 8-byte field in the generated struct.

struct Params {
    Sampler2D<float>.Handle src;         // 0, size 8
    RWTexture2D<float>.Handle dstR;      // 8, size 8
    RWTexture2D<float4>.Handle dstRgba;  // 16, size 8
    float scale;                         // 24, leaving a 4-byte tail gap
    float2 gridSize;                     // 32
}                                        // std140 block rounds to 48

ParameterBlock<Params> params;

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID: SV_DispatchThreadID) {
    let pixel = uint2(dispatchThreadID.xy);
    if (pixel.x >= uint(params.gridSize.x) || pixel.y >= uint(params.gridSize.y)) {
        return;
    }

    let fetched = params.src[pixel] * params.scale;

    // read-modify-write through a storage handle
    let p = int2(pixel);
    params.dstR[p] = params.dstR[p] + fetched;
    params.dstRgba[p] = float4(fetched);
}
