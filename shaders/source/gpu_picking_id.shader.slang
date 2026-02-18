#language slang 2026

module gpu_picking_id;

import fullscreen_triangle;
import ray_march_camera;
import projection;

struct Cube {
    float3 position;
    float3 radii;
}

ParameterBlock<GpuPickingIdParams> params;

struct GpuPickingIdParams {
    RayMarchCamera camera;
    uint cubeCount;
    StructuredBuffer<Cube> cubes;
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

[shader("fragment")]
uint fragMain(FragInput input) : SV_Target {
    let rayDir = params.camera.rayDirection(input.centeredCoords);
    let rayOrigin = params.camera.position;

    const int MAX_STEPS = 64;
    const float MAX_DIST = 100.0;
    const float MIN_DIST = 0.001;

    var t: float = 0.0;
    for (int i = 0; i < MAX_STEPS; i++) {
        let p = rayOrigin + t * rayDir;
        let d = sdBox(p - params.cubes[0].position, params.cubes[0].radii);
        if (d < MIN_DIST) {
            return 1u; // cube hit
        }
        if (t > MAX_DIST) {
            break;
        }
        t += d;
    }

    return 0u; // miss
}
