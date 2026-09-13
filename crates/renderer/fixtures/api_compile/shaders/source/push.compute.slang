#language slang 2026

module push_compute;

import mltrs;
import particle;

ParameterBlock<ScaleParams> scaleParams;

struct ScaleParams {
    mltrs::Addr<OtherElement> items;
    float scale;
}

// a push block, so the fixture can type-check .with_push_constant on every
// command family against a real generated push type
[[vk::push_constant]] ConstantBuffer<ScalePush> scalePush;

struct ScalePush {
    mltrs::Addr<OtherElement> items;
    float factor;
}

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    uint index = dispatchThreadID.x;

    OtherElement e = scaleParams.items[index];
    e.value *= scaleParams.scale * scalePush.factor;

    scaleParams.items[index] = e;
}
