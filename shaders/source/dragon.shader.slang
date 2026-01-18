#language slang 2026

module dragon;

import dragon_curve;
import fullscreen_triangle;
import ray_march_camera;
import projection;

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

