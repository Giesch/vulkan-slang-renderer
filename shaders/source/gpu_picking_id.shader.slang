#language slang 2026

module gpu_picking_id;

import fullscreen_triangle;
import ray_march_camera;
import projection;
import gpu_picking_common;

ParameterBlock<GpuPickingIdParams> params;

struct GpuPickingIdParams {
    RayMarchCamera camera;
    uint cubeCount;
    StructuredBuffer<Cube> cubes;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.centeredCoords);
}

[shader("fragment")]
uint fragMain(FragInput input) : SV_Target {
    let rayDir = params.camera.rayDirection(input.centeredCoords);

    if (let hit = marchCubes(params.camera.position, rayDir, params.cubeCount, params.cubes)) {
        return hit.closestIdx + 1u;
    }

    return 0u; // miss
}
