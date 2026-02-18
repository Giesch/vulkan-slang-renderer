#language slang 2026

module gpu_picking;

import fullscreen_triangle;
import ray_march_camera;
import projection;

ParameterBlock<GpuPickingParams> params;

struct GpuPickingParams {
    RayMarchCamera camera;
    float3 cubePosition;
    float3 cubeRadii;
    uint pickedObjectId;
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

// SDF box distance
func sdBox(p: float3, radii: float3) -> float {
    let q = abs(p) - radii;
    return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
}

// https://iquilezles.org/articles/normalsSDF/
func calcNormal(p: float3, cubePos: float3, cubeRadii: float3) -> float3 {
    const float h = 0.001;
    const float2 k = float2(1.0, -1.0);
    return normalize(
        k.xyy * sdBox(p + k.xyy * h - cubePos, cubeRadii) +
        k.yyx * sdBox(p + k.yyx * h - cubePos, cubeRadii) +
        k.yxy * sdBox(p + k.yxy * h - cubePos, cubeRadii) +
        k.xxx * sdBox(p + k.xxx * h - cubePos, cubeRadii)
    );
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let rayDir = params.camera.rayDirection(input.centeredCoords);
    let rayOrigin = params.camera.position;

    const int MAX_STEPS = 64;
    const float MAX_DIST = 100.0;
    const float MIN_DIST = 0.001;

    var t: float = 0.0;
    for (int i = 0; i < MAX_STEPS; i++) {
        let p = rayOrigin + t * rayDir;
        let d = sdBox(p - params.cubePosition, params.cubeRadii);
        if (d < MIN_DIST) {
            // hit the cube
            let normal = calcNormal(p, params.cubePosition, params.cubeRadii);

            // base color or gold when picked
            float3 baseColor = float3(0.3, 0.5, 0.8);
            if (params.pickedObjectId == 1) {
                baseColor = float3(1.0, 0.84, 0.0);
            }

            // simple directional lighting
            let lightDir = normalize(float3(1.0, 1.0, -0.5));
            let diffuse = max(dot(normal, lightDir), 0.0);
            let ambient = 0.15;
            let color = baseColor * (ambient + diffuse);

            return float4(color, 1.0);
        }
        if (t > MAX_DIST) {
            break;
        }
        t += d;
    }

    // background gradient
    let grad = input.centeredCoords.y * 0.5 + 0.5;
    let bg = lerp(float3(0.1, 0.1, 0.15), float3(0.2, 0.2, 0.3), grad);
    return float4(bg, 1.0);
}
