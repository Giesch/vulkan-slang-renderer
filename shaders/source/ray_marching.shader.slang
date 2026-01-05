#language slang 2026

// Raymarching is a 3D rendering technique suited to working with SDFs instead of triangle meshes.
// https://michaelwalczyk.com/blog-ray-marching.html
// https://youtu.be/mL8U8tIiRRg?list=PLImQaTpSAdsCnJon-Eir92SZMl7tPBS4Z&t=7288

module ray_marching;

import fullscreen_triangle;

ParameterBlock<RayMarchingParams> params;

// this assumes a 2x2 x-y image plane at float3(0.0)
// TODO move to uniform
static const float3 cameraPosition = float3(0.0, 0.0, -5.0);

struct RayMarchingParams {
    float2 resolution;
    StructuredBuffer<Sphere> spheres;
    uint sphereCount;
}

struct Sphere {
    float3 center;
    float radius;

    float distance(float3 point) {
        return length(point - center) - radius;
    }
}

struct FragInput {
    float4 position : SV_Position;
    float2 centeredCoords : TEXCOORD0;
}

static const float2x2 reflectY = float2x2(
    1.0,  0.0,
    0.0, -1.0,
);

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.centeredCoords);
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
    // z = field-of-view
    let rayDirection = float3(input.centeredCoords, 1.0);
    let color = rayMarch(cameraPosition, rayDirection);

    return float4(color, 1.0);
}

func rayMarch(rayOrigin: float3, rayDirection: float3) -> float3 {
    const int N_STEPS = 32;
    const float MIN_HIT_DISTANCE = 0.001;
    const float MAX_TRACE_DISTANCE = 1000.0;

    var distanceTraveled: float = 0.0;

    for (int i = 0; i < N_STEPS; i++) {
        let currentPosition: float3 = rayOrigin + distanceTraveled * rayDirection;
        let closestDistance: float = mapTheWorld(currentPosition);

        if (closestDistance < MIN_HIT_DISTANCE) {
            // hit

            let normal = calculateNormal(currentPosition);
            // normal map color; [-1, 1] to [0, 1]
            // return normal * 0.5 + 0.5;

            // TODO move to uniform
            let lightPosition = float3(2.0, -5.0, 2.0);
            let directionToLight = normalize(currentPosition - lightPosition);

            let lightColor = float3(0.45, 0.45, 0.6);

            // lambertian diffuse
            let diffuseIntensity = max(0.0, dot(normal, directionToLight));

            // 'nikita diffuse'
            // https://lisyarus.github.io/blog/posts/a-silly-diffuse-shading-model.html
            // let diffuseIntensity = (1 + dot(normal, directionToLight)) / 2.0;

            return lightColor * diffuseIntensity;
        }

        if (distanceTraveled >= MAX_TRACE_DISTANCE) {
            // miss
            break;
        }

        distanceTraveled += closestDistance;
    }

    // miss
    return float3(0.0);
}

// returns the closest distance to any shape
func mapTheWorld(currentPosition: float3) -> float {
    var closestDistance = float.maxValue;

    // TODO make this part of a Shape interface
    for (int i = 0; i < params.sphereCount; i++) {
        let sphere = params.spheres[i];
        let sphereDistance = sphere.distance(currentPosition);
        if (sphereDistance < closestDistance) {
            closestDistance = sphereDistance;
        }
    }

    return closestDistance;
}

// TODO replace this with autodiff?
func calculateNormal(p: float3) -> float3 {
    const float2 smallStep = float2(0.001, 0.0);

    let gradientX = mapTheWorld(p + smallStep.xyy) - mapTheWorld(p - smallStep.xyy);
    let gradientY = mapTheWorld(p + smallStep.yxy) - mapTheWorld(p - smallStep.yxy);
    let gradientZ = mapTheWorld(p + smallStep.yyx) - mapTheWorld(p - smallStep.yyx);

    return normalize(
        float3(gradientX, gradientY, gradientZ)
    );
}
