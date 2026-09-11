#language slang 2026

module tex_compute;

import mltrs;

ParameterBlock<TexParams> texParams;

struct TexParams {
    Sampler2D<float>.Handle heightIn;
    RWTexture2D<float>.Handle heightOut;
    float dt;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel = int2(dispatchThreadID.xy);

    texParams.heightOut[pixel] = texParams.heightIn[pixel] * texParams.dt;
}
