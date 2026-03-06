#language slang 2026

module paint_display;

import fullscreen_triangle;

ParameterBlock<DisplayParams> displayParams;

struct DisplayParams {
    Sampler2D canvas;
    Sampler2D paperHeight;
    float2 texelSize;
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
    let uv = input.texCoord;
    let ts = displayParams.texelSize;

    // Sample height map neighbors for central finite differences
    let hL = displayParams.paperHeight.Sample(uv + float2(-ts.x, 0.0)).r;
    let hR = displayParams.paperHeight.Sample(uv + float2( ts.x, 0.0)).r;
    let hD = displayParams.paperHeight.Sample(uv + float2(0.0, -ts.y)).r;
    let hU = displayParams.paperHeight.Sample(uv + float2(0.0,  ts.y)).r;
    let hC = displayParams.paperHeight.Sample(uv).r;

    // Compute surface normal via bump mapping
    let bumpStrength = 10.0;
    let dhdx = (hR - hL) * bumpStrength;
    let dhdy = (hU - hD) * bumpStrength;
    let normal = normalize(float3(-dhdx, -dhdy, 1.0));

    // Lambertian lighting from upper-left raking light
    let lightDir = normalize(float3(-1.0, -1.0, 2.0));
    let ambient = 0.6;
    let diffuse = max(dot(normal, lightDir), 0.0);
    let lighting = ambient + (1.0 - ambient) * diffuse;

    // Paper base color with bump-mapped lighting
    let paperBase = float3(0.96, 0.94, 0.90);
    let litPaper = paperBase * lighting;

    // Composite paint over lit paper
    // Allow some paper grain to show through painted areas
    let paint = displayParams.canvas.Sample(uv);
    let paintLighting = lerp(1.0, lighting, 0.3);
    let paintColor = paint.rgb * paintLighting;
    let color = paintColor * paint.a + litPaper * (1.0 - paint.a);

    return float4(color, 1.0);
}
