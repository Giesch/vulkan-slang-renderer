#language slang 2026

module push_constants_compute;

// Test: a [[vk::push_constant]] block in a compute shader.
//
// Mirrors push_constants.shader.slang's pins for the compute reflection and
// codegen path: the nested struct + `tail` prove std430 over std140, and the
// handles pin that a handle living *only* in a push block still sets
// bindlessHeapSet — Params carries no handle, so the push block is the only
// possible source. `target` is a storage-image handle in a push block: the
// write target selected per dispatch. SampleLevel, not Sample: implicit-LOD
// sampling is illegal in compute.

struct DispatchInner {
    float2 v;   // 0
}               // std430: align 8, size 8 | std140: align 16, size 16

struct DispatchConstants {
    float scale;                        // 0
    Sampler2D.Handle tex;               // 8   (4-byte gap after the scalar)
    RWTexture2D<float4>.Handle target;  // 16
    DispatchInner inner;                // std430: 24 | std140: 32
    float tail;                         // std430: 32 | std140: 48
    float4x4 model;                     // std430: 48 | std140: 64
}                                       // std430: 112 | std140: 128

struct Params {
    float4 tint;    // 0
}                   // size 16

ParameterBlock<Params> params;
[[vk::push_constant]] ConstantBuffer<DispatchConstants> dispatchConstants;

// reads every member of the block: push_constant_compute_spirv_layout asserts
// against this module, and an unread member would be free for the optimizer to
// drop, shifting the member indices the test pins
[numthreads(8, 8, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let uv = dispatchConstants.inner.v * dispatchConstants.scale + dispatchConstants.tail;
    let color = mul(dispatchConstants.model, dispatchConstants.tex.SampleLevel(uv, 0.0));
    dispatchConstants.target[int2(dispatchThreadID.xy)] = color * params.tint;
}
