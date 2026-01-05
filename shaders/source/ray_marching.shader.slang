#language slang 2026

// Raymarching is a 3D rendering technique suited to working with SDFs instead of triangle meshes.
// https://michaelwalczyk.com/blog-ray-marching.html
// https://youtu.be/mL8U8tIiRRg?list=PLImQaTpSAdsCnJon-Eir92SZMl7tPBS4Z&t=7288

module ray_marching;

import fullscreen_triangle;
import ray_march_camera;

ParameterBlock<RayMarchingParams> params;

static const float3 lightColor = float3(1);

struct RayMarchingParams {
    RayMarchCamera camera;

    float3 lightPosition;
    uint sphereCount;

    float2 resolution;
    StructuredBuffer<Sphere> spheres;
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
    let rayDirection = params.camera.rayDirection(input.centeredCoords);
    let color = rayMarch(params.camera.position, rayDirection);

    return float4(color, 1.0);
}

func rayMarch(rayOrigin: float3, rayDirection: float3) -> float3 {
    const int N_STEPS = 32;
    const float MIN_HIT_DISTANCE = 0.001;
    const float MAX_TRACE_DISTANCE = 1000.0;

    var distanceTraveled: float = 0.0;

    for (int i = 0; i < N_STEPS; i++) {
        let currentPosition: float3 = rayOrigin + distanceTraveled * rayDirection;
        var closestDistance = float.maxValue;

        if (let hit = closestShape(currentPosition)) {
            closestDistance = hit.distance;

            if (closestDistance < MIN_HIT_DISTANCE) {
                // hit

                let normal = calculateNormal(currentPosition);

                // https://learnopengl.com/Lighting/Basic-Lighting

                let ambientStrength = 0.005;
                let ambient = ambientStrength * lightColor;

                let lightDirection = normalize(params.lightPosition - currentPosition);

                // lambertian diffuse
                let diffuse = lightColor * max(0.0, dot(normal, lightDirection));

                // blinn-phong specular reflection
                let specularStrength = 0.5;
                let shininess = 16.0;
                let viewDirection = -rayDirection;
                float3 halfwayDirection = normalize(lightDirection + rayDirection);
                let specularIntensity = pow(max(dot(normal, halfwayDirection), 0.0), shininess);
                let specular = specularStrength * lightColor * specularIntensity;

                return (ambient + diffuse + specular) * hit.objectColor;
            }
        }

        if (distanceTraveled >= MAX_TRACE_DISTANCE) { // miss
            break;
        }

        distanceTraveled += closestDistance;
    }

    // miss
    return float3(0.0);
}

func closestShape(pos: float3) -> Optional<RayHit> {
    let sphereHit = Sphere.closest(params.spheres, params.sphereCount, pos);

    return sphereHit;
}

func closestShapeDist(pos: float3) -> float {
    if (let hit = closestShape(pos)) {
        return hit.distance;
    }

    return float.maxValue;
}

func calculateNormal(p: float3) -> float3 {
    const float2 smallStep = float2(0.001, 0.0);

    let gradientX = closestShapeDist(p + smallStep.xyy) - closestShapeDist(p - smallStep.xyy);
    let gradientY = closestShapeDist(p + smallStep.yxy) - closestShapeDist(p - smallStep.yxy);
    let gradientZ = closestShapeDist(p + smallStep.yyx) - closestShapeDist(p - smallStep.yyx);

    return normalize(
        float3(gradientX, gradientY, gradientZ)
    );
}

struct RayHit {
    float distance;
    float3 objectColor;
}

// a generic 3D SDF
interface IShape {
    static Optional<RayHit> closest(StructuredBuffer<This> buffer, int count, float3 position) {
        var rayHit: Optional<RayHit> = none;

        for (int i = 0; i < count; i++) {
            let shape = buffer[i];
            let shapeDistance = shape.distance(position);

            if (let existingHit = rayHit) {
                if (shapeDistance < existingHit.distance) {
                    rayHit = RayHit(shapeDistance, shape.color);
                }
            } else {
                rayHit = RayHit(shapeDistance, shape.color);
            }
        }

        return rayHit;
    }

    property float3 color { get; }

    float distance(float3 position);
}

struct Sphere : IShape {
    float3 center;
    float radius;

    float3 color;

    float distance(float3 point) {
        return length(point - center) - radius;
    }
}
