#language slang 2026

module paint_display;

import fullscreen_triangle;

ParameterBlock<DisplayParams> displayParams;

struct DisplayParams {
    Sampler2D canvas;
    Sampler2D paperHeight;
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
    let h = displayParams.paperHeight.Sample(input.texCoord).r;

    // paper height texture
    let paperBase = float3(0.96, 0.94, 0.90);
    let paperColor = paperBase * lerp(0.95, 1.02, h);

    let color = paint.rgb * paint.a + paperColor * (1.0 - paint.a);

    return float4(color, 1.0);
}
