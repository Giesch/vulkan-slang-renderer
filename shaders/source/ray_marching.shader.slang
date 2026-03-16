#language slang 2026

module ray_marching;

import ray_march;
import fullscreen_triangle;
import ray_march_camera;
import projection;

ParameterBlock<RayMarchingParams> params;

static const float3 lightColor = float3(1.0);

struct RayMarchingParams {
    RayMarchCamera camera;

    float3 lightPosition;
    uint sphereCount;

    uint boxCount;

    float2 resolution;

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

    // super-sampling
    let rootSamples = 3; // 9 samples per pixel
    let pixelSize = 1.0 / params.resolution;
    var sum = float3(0.0);

    [ForceUnroll]
    for (int m = 0; m < rootSamples; m++) {
        [ForceUnroll]
        for (int n = 0; n < rootSamples; n++) {
            let offset = ((float2(m, n) + 0.5) / float(rootSamples) - 0.5) * pixelSize;
            let coords = input.centeredCoords + offset;
            let rayDirection = params.camera.rayDirection(coords);
            sum += rayMarch(
                world,
                params.camera.position,
                rayDirection,
                params.lightPosition,
                lightColor
            );
        }
    }

    let sampleCount = rootSamples * rootSamples;
    return float4(sum / float(sampleCount), 1.0);
}

struct RayMarchWorld : IRayMarchWorld {
    StructuredBuffer<Sphere> spheres;
    uint sphereCount;

    StructuredBuffer<BoxRect> boxes;
    uint boxCount;

    float minHitDistance = 0.001;

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
