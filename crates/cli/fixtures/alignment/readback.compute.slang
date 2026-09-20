#language slang 2026
module readback_compute;

enum ReadbackTag : uint { Ready = 3, Done = 7 }
struct ReadbackInner { float2 xy; uint id; }
struct ReadbackOutput {
    uint marker;                  // 0
    float4 vector;                // 16
    ReadbackInner nested;         // 32, size 16
    ReadbackTag tag;              // 48
    float4x4 transform;           // 64
    uint4 values[2];              // 128
}                                // size 160
struct ReadbackParams {
    LayoutPtr<ReadbackOutput, Std430DataLayout> output;
}
ParameterBlock<ReadbackParams> params;
[numthreads(1, 1, 1)]
[shader("compute")]
void computeMain(uint3 id : SV_DispatchThreadID) {
    params.output[id.x].marker = 42;
}
