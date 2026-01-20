#language slang 2026

module dragon;

import dragon_curve;
import fullscreen_triangle;
import ray_march_camera;
import ray_march;

ParameterBlock<DragonParams> params;

struct DragonParams {
    float time;
}

struct FragInput {
    float4 position : SV_Position;
    float2 centeredCoords : TEXCOORD0;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.centeredCoords);
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let recenterOffset = float2(0.15, 0.0);
    let uv = input.centeredCoords + recenterOffset;
    return dragonCurve2D(uv, params.time);
}

float4 dragonCurve2D(float2 uv, float time) {
    let dist = dragonCurveSDF(uv);

    float dragonEdge = dist < 0.25 ? 1.0 : 0.0;
    let color = rainbowAnimation(uv);

    return float4(color * dragonEdge, 1.0);
}

float3 rainbowAnimation(float2 pos) {
    return 0.5 + 0.5 * cos(params.time + 8 * pos.xyx + float3(0, 2, 4));
}
