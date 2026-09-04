#language slang 2026

module recipe_render;

import font;
import mltrs;
import shared;

ParameterBlock<RenderParams> params;

struct RenderParams {
    mltrs::ReadAddr<Solution> solution;
    float2 resolution;
}

struct FragInput {
    float4 position : SV_Position;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = mltrs::fullscreenPosition(id);
    return FragInput(fp.svPosition);
}

static const float PIXEL_SCALE = 10.0;
static const float4 TEXT_COLOR = float4(1.0);
static const float4 BACKGROUND_COLOR = float4(0.08, 0.08, 0.10, 1.0);

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let text = glyphsForUint(params.solution.maximumScore);

    let digitsFont = Digits3x5Font(PIXEL_SCALE, TEXT_COLOR, BACKGROUND_COLOR);
    let textSize = digitsFont.textSize(text.count);
    let origin = (params.resolution - textSize) * 0.5;

    return digitsFont.render(
        input.position.xy,
        origin,
        textSize,
        text.chars,
        text.count
    );
}
