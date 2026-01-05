#language slang 2026

// Raymarching is a 3D rendering technique suited to working with SDFs instead of triangle meshes.
// https://michaelwalczyk.com/blog-ray-marching.html
// https://youtu.be/mL8U8tIiRRg?list=PLImQaTpSAdsCnJon-Eir92SZMl7tPBS4Z&t=7288

module ray_marching;

import fullscreen_triangle;

ParameterBlock<RayMarchingParams> params;

static const float3 lightColor = float3(1);

// TODO FIXME the order of these fields can break the renderer?
// do resources have to go last?
struct RayMarchingParams {
    float3 cameraPosition;
    uint sphereCount;

    float3 lightPosition;
    // TODO use this to rescale
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
    // z = field-of-view
    // TODO move the image plane based on camera position
    let rayDirection = float3(input.centeredCoords, 1.0);
    let color = rayMarch(params.cameraPosition, rayDirection);

    return float4(color, 1.0);
}

func rayMarch(rayOrigin: float3, rayDirection: float3) -> float3 {
    const int N_STEPS = 32;
    const float MIN_HIT_DISTANCE = 0.001;
    const float MAX_TRACE_DISTANCE = 1000.0;

    var distanceTraveled: float = 0.0;

    for (int i = 0; i < N_STEPS; i++) {
        let currentPosition: float3 = rayOrigin + distanceTraveled * rayDirection;
        let closestDistance: float = closestShape(currentPosition);

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

            let objectColor = float3(0.2, 0.2, 0.6);
            return (ambient + diffuse + specular) * objectColor;
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

func closestShape(currentPosition: float3) -> float {
    let closestSphere = Sphere.closest(params.spheres, params.sphereCount, currentPosition);
    return closestSphere;
}

// TODO replace this with autodiff?
func calculateNormal(p: float3) -> float3 {
    const float2 smallStep = float2(0.001, 0.0);

    let gradientX = closestShape(p + smallStep.xyy) - closestShape(p - smallStep.xyy);
    let gradientY = closestShape(p + smallStep.yxy) - closestShape(p - smallStep.yxy);
    let gradientZ = closestShape(p + smallStep.yyx) - closestShape(p - smallStep.yyx);

    return normalize(
        float3(gradientX, gradientY, gradientZ)
    );
}

interface IShape {
    static float closest(StructuredBuffer<This> buffer, int count, float3 position) {
        var closestDistance = float.maxValue;

        for (int i = 0; i < count; i++) {
            let shape = buffer[i];
            let shapeDistance = shape.distance(position);
            if (shapeDistance < closestDistance) {
                closestDistance = shapeDistance;
            }
        }

        return closestDistance;
    }

    float distance(float3 position);
}

struct Sphere : IShape {
    float3 center;
    float radius;

    float distance(float3 point) {
        return length(point - center) - radius;
    }
}
