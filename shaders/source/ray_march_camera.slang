#language slang 2026

module ray_march_camera;

// provided by the cpu
extern static const bool columnMajor;

public struct RayMarchCamera {
    public float4x4 inverseViewProj;  // Y-flip applied on CPU before inverting
    public float3 position;

    public float3 rayDirection(float2 centeredCoords) {
        let ndc = float4(centeredCoords * 2.0, 1.0, 1.0);

        // see the comments in mvp.slang
        var worldPoint = columnMajor
            ? mul(this.inverseViewProj, ndc)
            : mul(ndc, this.inverseViewProj);
        worldPoint /= worldPoint.w;

        return normalize(worldPoint.xyz - this.position);
    }
}

