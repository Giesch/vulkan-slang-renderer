#language slang 2026

module gpu_picking_id;

import mltrs;
import ray_march_camera;
import gpu_picking_common;

ParameterBlock<GpuPickingIdParams> params;

struct GpuPickingIdParams {
    RayMarchCamera camera;
    uint cubeCount;
    mltrs::ReadAddr<Cube> cubes;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = mltrs::fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.svPosition.xy);
}

[shader("fragment")]
uint fragMain(FragInput input) : SV_Target {
    let rayDir = params.camera.rayDirection(input.centeredCoords);

    if (let hit = marchCubes(params.camera.position, rayDir, params.cubeCount, params.cubes)) {
        return hit.closestIdx + 1u;
    }

    return 0u; // miss
}
