#language slang 2026

module ray_march_camera;

import projection;

public struct RayMarchCamera {
    public Projection inverseViewProj;
    public float3 position;

    public float3 rayDirection(float2 centeredCoords) {
        let ndc = float4(centeredCoords * 2.0, 1.0, 1.0);

        var worldPoint = inverseViewProj.project(ndc);
        worldPoint /= worldPoint.w;

        return normalize(worldPoint.xyz - this.position);
    }
}

