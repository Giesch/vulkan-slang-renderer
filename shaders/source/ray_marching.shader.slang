#language slang 2026

module ray_marching;

import ray_march;
import fullscreen_triangle;
import ray_march_camera;
import projection;

ParameterBlock<RayMarchingParams> params;

static const float3 lightColor = float3(1);

struct RayMarchingParams {
    RayMarchCamera camera;

    float3 lightPosition;
    uint sphereCount;

    uint boxCount;

    StructuredBuffer<Sphere> spheres;
    StructuredBuffer<BoxRect> boxes;
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

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let world = RayMarchWorld(
        params.spheres,
        params.sphereCount,
        params.boxes,
        params.boxCount
    );

    let rayDirection = params.camera.rayDirection(input.centeredCoords);
    let color = rayMarch(
        world,
        params.camera.position,
        rayDirection,
        params.lightPosition,
        lightColor
    );

    return float4(color, 1.0);
}

struct RayMarchWorld : IRayMarchWorld {
    StructuredBuffer<Sphere> spheres;
    uint sphereCount;

    StructuredBuffer<BoxRect> boxes;
    uint boxCount;

    Optional<RayHit> closestShape(float3 pos) {
        let sphereRes = Sphere.closest(spheres, sphereCount, pos);
        let boxRes = BoxRect.closest(boxes, boxCount, pos);

        if (sphereRes.distance <= boxRes.distance) {
            return sphereRes.rayHit;
        }

        return boxRes.rayHit;
    }
}


// https://iquilezles.org/articles/distfunctions/

public struct Sphere : IShape {
    float3 center;
    float radius;

    float3 color;

    float distance(float3 point) {
        return length(point - center) - radius;
    }
}

// https://www.youtube.com/watch?v=62-pRVZuS5c
public struct BoxRect : IShape {
    Projection transform;
    float3 radii;

    float3 color;

    float distance(float3 point) {
        let localPoint = transform.project(float4(point, 1.0)).xyz;
        let q = abs(localPoint) - radii;
        return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
    }
}
