#language slang 2026

module ray_march_camera;

public struct RayMarchCamera {
    public float4x4 inverseViewProj;  // Y-flip applied on CPU before inverting
    public float3 position;
    float padding;

    public float3 rayDirection(float2 centeredCoords) {
        let ndc = float4(centeredCoords * 2.0, 1.0, 1.0);

        // TODO does this need to use matrix layout?
        var worldPoint = mul(ndc, this.inverseViewProj);
        worldPoint /= worldPoint.w;

        return normalize(worldPoint.xyz - this.position);
    }
}

