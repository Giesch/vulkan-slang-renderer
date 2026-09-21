#language slang 2026
module readback_compute;

enum ReadbackTag : uint { Ready = 3, Done = 7 }

struct ReadbackInner {
    float2 xy;
    uint id;
}

struct ReadbackOutput {
    uint marker;
    float4 vector;
    ReadbackInner nested;
    ReadbackTag tag;
    float4x4 transform;
    uint4 values[2];
}

struct ReadbackPush {
    LayoutPtr<ReadbackOutput, Std430DataLayout> output;
    uint invalidTag;
    uint reserved;
}

struct ReadbackInput {
    uint4 baseMarker;
}

ParameterBlock<ReadbackInput> params;
[[vk::push_constant]] ConstantBuffer<ReadbackPush> pc;

[numthreads(1, 1, 1)]
[shader("compute")]
void computeMain(uint3 id : SV_DispatchThreadID) {
    let i = id.x;
    let matrix = float4x4(
        2, 3, 5, 7,
        11, 13, 17, 19,
        23, 29, 31, 37,
        41, 43, 47, 53);
    pc.output[i].marker = params.baseMarker.x + i;
    pc.output[i].vector = mul(matrix, float4(1, 2, 3, 1)) + float(i);
    pc.output[i].nested.xy = float2(-2.5, 8.25) + float(i);
    pc.output[i].nested.id = 100 + i;
    pc.output[i].tag = pc.invalidTag != 0
        ? ReadbackTag(99)
        : (i == 0 ? ReadbackTag.Ready : ReadbackTag.Done);
    pc.output[i].transform = matrix * float(i + 1);
    pc.output[i].values[0] = uint4(10, 20, 30, 40) + i;
    pc.output[i].values[1] = uint4(50, 60, 70, 80) + i;
}
