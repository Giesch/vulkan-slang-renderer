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
    return dragonCurve2D(input.centeredCoords, params.time);
}

float4 dragonCurve2D(float2 uv, float time) {
    let recenterTweak = float2(0.15, 0.0);
    let dist = dragonCurveSDF(uv + recenterTweak);

    float distScale = 0.25;
    float dragonEdge = 1.0 - step(distScale, dist);
    float3 rainbowAnimation = 0.5 + 0.5 * cos(time + 8 * uv.xyx + float3(0, 2, 4));

    return float4(rainbowAnimation * dragonEdge, 1);
}
