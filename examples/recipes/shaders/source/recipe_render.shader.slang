#language slang 2026

module recipe_render;

import mltrs;
import shared;

ParameterBlock<RenderParams> params;

struct RenderParams {
    mltrs::ReadAddr<Solution> solution;
}

struct FragInput {
    float4 color : TEXCOORD0;
    float4 position : SV_Position;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    if (id == 0) {
        printf("maximum score: %1d", params.solution.maximumScore);
    }

    FragInput output;
    return output;
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    return input.color;
}
