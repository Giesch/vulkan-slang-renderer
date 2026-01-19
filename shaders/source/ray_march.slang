#language slang 2026

module ray_march;

public interface IRayMarchWorld {
    Optional<RayHit> closestShape(float3 pos);

    float closestShapeDist(float3 pos) {
        if (let hit = closestShape(pos)) {
            return hit.distance;
        }

        return float.maxValue;
    }
}

public struct RayHit {
    public float distance;
    public float3 objectColor;
}

public struct RayHitDistance {
    public Optional<RayHit> rayHit;
    public float distance;
}

public float3 rayMarch<W>(
    W world,
    float3 rayOrigin,
    float3 rayDirection,
    float3 lightPosition,
    float3 lightColor,
) where W : IRayMarchWorld {
    const int MAX_STEPS = 32;
    const float MAX_TRACE_DISTANCE = 1000.0;
    const float MIN_HIT_DISTANCE = 0.001;

    var distanceTraveled: float = 0.0;

    for (int i = 0; i < MAX_STEPS; i++) {
        let currentPosition: float3 = rayOrigin + distanceTraveled * rayDirection;
        var closestDistance = float.maxValue;

        if (let hit = world.closestShape(currentPosition)) {
            closestDistance = hit.distance;

            if (closestDistance < MIN_HIT_DISTANCE) { // hit
                let blp = BasicLightingParams<W>(
                    world,
                    currentPosition,
                    rayDirection,
                    hit.objectColor,
                    lightColor,
                    lightPosition
                );

                return basicLighting(blp);
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

// https://iquilezles.org/articles/normalsSDF/
func calculateNormal<W>(world: W, p: float3) -> float3 where W : IRayMarchWorld {
    const float h = 0.001;
    const float2 k = float2(1.0, -1.0);

    return normalize(
        k.xyy * world.closestShapeDist(p + k.xyy * h) +
        k.yyx * world.closestShapeDist(p + k.yyx * h) +
        k.yxy * world.closestShapeDist(p + k.yxy * h) +
        k.xxx * world.closestShapeDist(p + k.xxx * h)
    );
}

// TODO just pass the arguments
struct BasicLightingParams<W> where W : IRayMarchWorld {
    W world;
    float3 currentPosition;
    float3 rayDirection;
    float3 objectColor;
    float3 lightColor;
    float3 lightPosition;
}

// https://learnopengl.com/Lighting/Basic-Lighting
func basicLighting<W>(blp: BasicLightingParams<W>) -> float3 where W : IRayMarchWorld {
    let normal = calculateNormal(blp.world, blp.currentPosition);
    let lightDirection = normalize(blp.lightPosition - blp.currentPosition);

    // naive ambient light
    let ambientStrength = 0.005;
    let ambient = ambientStrength * blp.lightColor;

    // lambertian diffuse
    let diffuse = blp.lightColor * max(0.0, dot(normal, lightDirection));

    // blinn-phong specular reflection
    let specularStrength = 0.25;
    let shininess = 16.0;
    float3 halfwayDirection = normalize(lightDirection + blp.rayDirection);
    let specularIntensity = pow(max(dot(normal, halfwayDirection), 0.0), shininess);
    let specular = specularStrength * blp.lightColor * specularIntensity;

    return (ambient + diffuse + specular) * blp.objectColor;
}


// a generic 3D SDF
public interface IShape {
    static RayHitDistance closest(StructuredBuffer<This> buffer, int count, float3 position) {
        var rayHit: Optional<RayHit> = none;

        for (int i = 0; i < count; i++) {
            let shape = buffer[i];
            let shapeDistance = shape.distance(position);

            if (rayHit.hasValue && shapeDistance < rayHit.value.distance) {
                rayHit = RayHit(shapeDistance, shape.color);
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
