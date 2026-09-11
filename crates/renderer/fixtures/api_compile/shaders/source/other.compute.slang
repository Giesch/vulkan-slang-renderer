#language slang 2026

module other_compute;

import mltrs;
import particle;

ParameterBlock<OtherParams> otherParams;

struct OtherParams {
    mltrs::Addr<OtherElement> items;
    float scale;
}

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    uint index = dispatchThreadID.x;

    OtherElement e = otherParams.items[index];
    e.value *= otherParams.scale;

    otherParams.items[index] = e;
}
