#language slang 2026

module gpu_picking_common;

public struct Cube {
    public float3 position;
    public float3 radii;
}

public struct FragInput {
    public float4 position : SV_Position;
    public float2 centeredCoords : TEXCOORD0;
}

// SDF box distance
public func sdBox(p: float3, radii: float3) -> float {
    let q = abs(p) - radii;
    return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
}

public struct RayMarchHit {
    public float3 position;
    public uint closestIdx;
}

public func marchCubes(
    rayOrigin: float3,
    rayDir: float3,
    cubeCount: uint,
    cubes: StructuredBuffer<Cube>
) -> Optional<RayMarchHit> {
    const int MAX_STEPS = 128;
    const float MAX_DIST = 100.0;
    const float MIN_DIST = 0.001;

    var t: float = 0.0;
    for (int i = 0; i < MAX_STEPS; i++) {
        let p = rayOrigin + t * rayDir;

        var minDist: float = MAX_DIST;
        var closestIdx: uint = 0;
        for (uint c = 0; c < cubeCount; c++) {
            let d = sdBox(p - cubes[c].position, cubes[c].radii);
            if (d < minDist) {
                minDist = d;
                closestIdx = c;
            }
        }

        if (minDist < MIN_DIST) {
            return RayMarchHit(p, closestIdx);
        }

        if (t > MAX_DIST) {
            return none;
        }

        t += minDist;
    }

    return none;
}
