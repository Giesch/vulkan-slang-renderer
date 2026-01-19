#language slang 2026

module dragon;

import dragon_curve;
import fullscreen_triangle;
import ray_march_camera;

ParameterBlock<DragonParams> params;

struct DragonParams {
    RayMarchCamera camera;
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
    let arbitraryOffset = float2(0.15, 0.075);
    return dragonCurve(input.centeredCoords + arbitraryOffset, params.time);
}

float4 dragonCurve(float2 uv, float time) {
    let dist = dragonCurveSDF(uv);

    float dist_scale = 0.25;
    float dragon_edge = 1.0 - step(dist_scale, dist);

    // kind of arbitrary color animation
    float3 col = 0.5 + 0.5 * cos(time + 8 * uv.xyx + float3(0, 2, 4));

    return float4(col * dragon_edge, 1);
}
