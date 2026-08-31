#language slang 2026

module ray_marching;

import mltrs;
import ray_march;
import ray_march_camera;

ParameterBlock<RayMarchingParams> params;

static const float3 lightColor = float3(1.0);

struct RayMarchingParams {
    RayMarchCamera camera;

    float3 lightPosition;
    uint sphereCount;

    uint boxCount;

    float2 resolution;

    mltrs::ReadAddr<Sphere> spheres;
    mltrs::ReadAddr<BoxRect> boxes;
}

struct FragInput {
    float4 position : SV_Position;
    float2 centeredCoords : TEXCOORD0;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = mltrs::fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.svPosition.xy);
}

struct RayMarchSampler : mltrs::ISuperSampler {
    float2 centeredCoords;
    float2 pixelSize;
    RayMarchWorld world;
    RayMarchCamera camera;
    float3 lightPosition;

    float3 sampleAtOffset(float2 subPixelOffset) {
        let offset = (subPixelOffset - 0.5) * pixelSize;
        let coords = centeredCoords + offset;
        let rayDirection = camera.rayDirection(coords);
        return rayMarch(
            world,
            camera.position,
            rayDirection,
            lightPosition,
            lightColor
        );
    }
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    let world = RayMarchWorld(
        params.spheres,
        params.sphereCount,
        params.boxes,
        params.boxCount
    );

    let sampler = RayMarchSampler(
        input.centeredCoords,
        2.0 / params.resolution,
        world,
        params.camera,
        params.lightPosition
    );
    let color = mltrs::superSample(sampler);
    return float4(color, 1.0);
}

struct RayMarchWorld : IRayMarchWorld {
    mltrs::ReadAddr<Sphere> spheres;
    uint sphereCount;

    mltrs::ReadAddr<BoxRect> boxes;
    uint boxCount;

    float minHitDistance = 0.001;

    Optional<ClosestShape> closestShape(float3 pos) {
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
    mltrs::Projection transform;
    float3 radii;

    float3 color;

    float distance(float3 point) {
        let localPoint = transform.project(float4(point, 1.0)).xyz;
        let q = abs(localPoint) - radii;
        return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
    }
}
