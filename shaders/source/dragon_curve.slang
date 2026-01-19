#language slang 2026

module dragon_curve;

// dragon curve by Blake447 on shadertoy:
// https://www.shadertoy.com/view/WdGcW3

public struct DragonResult {
    // distance to the curve
    public float dist;
    // the rotated and scaled uv used to build the curve
    public float2 uv;
}

public DragonResult dragonCurveSDF(float2 uv) {
    int ITERATIONS = 12;

    const float size = 5.0;

    for (int k = 0; k < ITERATIONS; k++) {
        uv = float2(uv.x + uv.y, uv.y - uv.x);
    }

    float2 uv_scaled = uv * size * 0.5;
    float2 uv_midpoint = floor(uv * size*0.5 + float2(0.5, 0.5));

    float2 p_cont = uv_scaled;

    float2 p0 = uv_midpoint + float2(0.5, 0);
    float2 p1 = uv_midpoint + float2(0, 0.5);
    float2 p2 = uv_midpoint - float2(0.5, 0);
    float2 p3 = uv_midpoint - float2(0, 0.5);

    float2 m = uv_midpoint;

    float2 c0 =  float2(0.5, 0.0);
    float2 c1 =  float2(0.0, 0.5);
    float2 c2 = -float2(0.5, 0.0);
    float2 c3 = -float2(0.0, 0.5);

    float2 v0 = p0;
    float2 v1 = p1;
    float2 v2 = p2;
    float2 v3 = p3;

    for (int j = 0; j < ITERATIONS; j++) {
        c0 = T(c0, j);
        c1 = T(c1, j);
        c2 = T(c2, j);
        c3 = T(c3, j);

        v0 = T(v0, j);
        v1 = T(v1, j);
        v2 = T(v2, j);
        v3 = T(v3, j);

        p_cont = T(p_cont, j);
    }

    float4 invalidate0 = float4(
        step(0.05, length(v0 - c0)),
        step(0.05, length(v1 - c0)),
        step(0.05, length(v2 - c0)),
        step(0.05, length(v3 - c0))
    );

    float d0_sdf = sdf0(uv_scaled, p0, uv_midpoint, size, invalidate0.x);
    float d1_sdf = sdf0(uv_scaled, p1, uv_midpoint, size, invalidate0.y);
    float d2_sdf = sdf0(uv_scaled, p2, uv_midpoint, size, invalidate0.z);
    float d3_sdf = sdf0(uv_scaled, p3, uv_midpoint, size, invalidate0.w);

    let dist = min(min(d0_sdf, d1_sdf), min(d2_sdf, d3_sdf));

    return DragonResult(dist, uv);
}

float2 T(float2 v, int i) {
    float t = (float(i) + 1.0) * 3.14159265 * 0.25;

    float scale = pow(2.0, float(i - 1) / 2.0);
    float2x2 change_of_basis = float2x2(cos(t), sin(t), -sin(t), cos(t)) / scale;
    float2x2 invert_of_basis = float2x2(cos(t), -sin(t), sin(t), cos(t)) * scale;

    float2 v_index = floor(mul(change_of_basis, v) * 0.5 + float2(0.5, 0.5));
    float direction = 1.0 - 2.0 * fmod(v_index.x + v_index.y + 4096.0, 2.0);

    float2 v_center = mul(invert_of_basis, v_index) * 2.0;
    float2 v_off = v - v_center;

    float2 v_prime = float2(v_off.x + v_off.y * direction, v_off.y - v_off.x * direction) + v_center;

    return v_prime;
}

float sdf0(float2 uv, float2 p, float2 middle_point, float size, float invalidation) {
    float d = sdfLineSegment(uv, p, middle_point);
    if (invalidation > 0.5) {
        d = 0.5;
    }

    return d;
}

float sdfLineSegment(float2 p, float2 a, float2 b) {
    float h = clamp(dot(p - a, normalize(b - a)), 0.0, 1.0);
    return length(p - (a + (b - a) * h) );
}
