#language slang 2026

module dragon;

import dragon_curve;
import fullscreen_triangle;
import ray_march_camera;
import ray_march;

ParameterBlock<DragonParams> params;

static const float3 lightColor = float3(1.0);
static const float3 lightPosition = float3(5.0);

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
    // return dragonCurve2D(
    //     input.centeredCoords + float2(0.15, 0.0),
    //     params.time
    // );

    let uv = input.centeredCoords;

    let rayDirection = params.camera.rayDirection(uv);
    let color = rayMarch(
        RayMarchWorld(),
        params.camera.position,
        rayDirection,
        lightPosition,
        lightColor
   );

    return float4(color, 1.0);
}

float4 dragonCurve2D(float2 uv, float time) {
    let dist = dragonCurveSDF(uv);

    float dragonEdge = dist < 0.25 ? 1.0 : 0.0;
    let color = rainbowAnimation(uv);

    return float4(color * dragonEdge, 1.0);
}

struct RayMarchWorld : IRayMarchWorld {
    float minHitDistance = 0.25;

    Optional<RayHit> closestShape(float3 pos) {
        // box sdf
        let radii = float3(5.0);
        let q = abs(pos) - radii;
        let boxDist = length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);


        let xy = dragonCurveSDF(pos.xy);
        // let yz = dragonCurveSDF(pos.yz);
        // let zx = dragonCurveSDF(pos.zx);
        // let dist = max(xy, max(yz, zx));
        // let dist = xy;

        // let planeDist = max(0.0, abs(pos.z) - 0.5);
        // let dist = length(float2(xy, planeDist));

        let dist = max(boxDist, xy);

        // claude tubes
        // let dist2d = dragonCurveSDF(pos.xy);
        // // Tube SDF: Z contributes to distance for proper tube normals
        // // zScale = 0.5 makes tube extend to z = ±0.5 (matching slab bounds)
        // let zScale = 0.5;
        // let tubeDist = sqrt(dist2d * dist2d + (pos.z * zScale) * (pos.z * zScale));
        // // Bound by Z planes (adds smooth falloff outside slab)
        // let halfDepth = 0.5;
        // let planeDist = max(0.0, abs(pos.z) - halfDepth);
        // let dist = length(float2(tubeDist, planeDist));

        float3 color = 0.25 + rainbowAnimation(pos.xy);
        // float3 color = float3(0.5);
        return RayHit(dist, color);
    }
}

float3 rainbowAnimation(float2 pos) {
    return 0.5 + 0.5 * cos(params.time + 8 * pos.xyx + float3(0, 2, 4));
}
