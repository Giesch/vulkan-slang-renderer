#language slang 2026

module handle_params;

// Test: a bindless texture handle alone in a ParameterBlock (std140).
// A DescriptorHandle lowers to a uint2 of ordinary data, so it is a uniform
// field the app writes — not a descriptor, and not a Resources entry. Expect
// size 8, align 8, and bindlessHeapSet set (rather than null) in the reflection
// json, since this shader is the reason the heap gets bound at all.

struct Params {
    Sampler2D.Handle tex;   // 0, size 8
}                           // std140 block rounds to 16

ParameterBlock<Params> params;

[shader("vertex")]
float4 vertMain(uint id: SV_VertexID) : SV_Position {
    return float4(1.0);
}

// a constant uv keeps the entry point parameter-free (like the other fixtures)
// while still forcing the handle to be sampled, and so kept
[shader("fragment")]
float4 fragMain() : SV_Target {
    return params.tex.Sample(float2(0.5));
}
