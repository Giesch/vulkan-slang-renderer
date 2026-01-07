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
                let specularStrength = 0.25;
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
    let sphereRes = Sphere.closest(params.spheres, params.sphereCount, pos);
    let boxRes = BoxRect.closest(params.boxes, params.boxCount, pos);

    if (sphereRes.distance <= boxRes.distance) {
        return sphereRes.rayHit;
    }

    return boxRes.rayHit;
}

func closestShapeDist(pos: float3) -> float {
    if (let hit = closestShape(pos)) {
        return hit.distance;
    }

    return float.maxValue;
}

// https://iquilezles.org/articles/normalsSDF/
func calculateNormal(p: float3) -> float3 {
    const float h = 0.001;
    const float2 k = float2(1.0, -1.0);

    return normalize(
        k.xyy * closestShapeDist(p + k.xyy * h) +
        k.yyx * closestShapeDist(p + k.yyx * h) +
        k.yxy * closestShapeDist(p + k.yxy * h) +
        k.xxx * closestShapeDist(p + k.xxx * h)
    );
}

struct RayHit {
    float distance;
    float3 objectColor;
}

struct RayHitDistance {
    Optional<RayHit> rayHit;
    float distance;
}

// a generic 3D SDF
interface IShape {
    static RayHitDistance closest(StructuredBuffer<This> buffer, int count, float3 position) {
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

        var dist = float.maxValue;
        if (let hit = rayHit) {
            dist = hit.distance;
        }

        return RayHitDistance(rayHit, dist);
    }

    property float3 color { get; }

    float distance(float3 position);
}


// https://iquilezles.org/articles/distfunctions/

struct Sphere : IShape {
    float3 center;
    float radius;

    float3 color;

    float distance(float3 point) {
        return length(point - center) - radius;
    }
}

// Quaternion helpers for rotating SDFs in place
func quatConjugate(float4 q) -> float4 {
    return float4(-q.xyz, q.w);
}

func rotateByQuat(float3 v, float4 q) -> float3 {
    float3 u = q.xyz;
    float s = q.w;
    return 2.0 * dot(u, v) * u
         + (s * s - dot(u, u)) * v
         + 2.0 * s * cross(u, v);
}

// https://www.youtube.com/watch?v=62-pRVZuS5c
struct BoxRect : IShape {
    float3 center;
    float3 radii;
    float3 color;
    float4 rotation; // quaternion (x, y, z, w)

    float distance(float3 point) {
        // Transform point to local space (rotate around center)
        let localPoint = rotateByQuat(point - center, quatConjugate(rotation));
        let q = abs(localPoint) - radii;
        return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0);
    }
}
