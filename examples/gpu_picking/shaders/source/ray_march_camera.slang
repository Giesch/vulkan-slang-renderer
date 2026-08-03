#language slang 2026

module ray_march_camera;

import mltrs;

public struct RayMarchCamera {
    public mltrs::Projection inverseViewProj;
    public float3 position;

    public float3 rayDirection(float2 centeredCoords) {
        let ndc = float4(centeredCoords * 2.0, 1.0, 1.0);

        var worldPoint = inverseViewProj.project(ndc);
        worldPoint /= worldPoint.w;

        return normalize(worldPoint.xyz - this.position);
    }
}

