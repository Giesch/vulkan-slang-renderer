#language slang 2026

module gpu_picking;

import fullscreen_triangle;
import ray_march_camera;
import projection;
import gpu_picking_common;

ParameterBlock<GpuPickingParams> params;

struct GpuPickingParams {
    RayMarchCamera camera;
    uint pickedObjectId;
    uint cubeCount;
    StructuredBuffer<Cube> cubes;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.centeredCoords);
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

    if (let hit = marchCubes(params.camera.position, rayDir, params.cubeCount, params.cubes)) {
        let hitCube = params.cubes[hit.closestIdx];
        let normal = calcNormal(hit.position, hitCube.position, hitCube.radii);

        float3 baseColor = float3(0.3, 0.5, 0.8);
        if (params.pickedObjectId == hit.closestIdx + 1u) {
            baseColor = float3(1.0, 0.84, 0.0);
        }

        let lightDir = normalize(float3(1.0, 1.0, -0.5));
        let diffuse = max(dot(normal, lightDir), 0.0);
        let ambient = 0.15;
        let color = baseColor * (ambient + diffuse);

        return float4(color, 1.0);
    }

    let grad = input.centeredCoords.y * 0.5 + 0.5;
    let bg = lerp(float3(0.1, 0.1, 0.15), float3(0.2, 0.2, 0.3), grad);
    return float4(bg, 1.0);
}
