#language slang 2026

module paint_display;

import fullscreen_triangle;

ParameterBlock<DisplayParams> displayParams;

struct DisplayParams {
    Sampler2D canvas;
}

struct FragInput {
    float4 position : SV_Position;
    float2 texCoord : TEXCOORD0;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.texCoord);
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let paint = displayParams.canvas.Sample(input.texCoord);

    let white = float3(1.0);
    let color = paint.rgb * paint.a + white * (1.0 - paint.a);

    return float4(color, 1.0);
}
