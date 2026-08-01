#language slang 2026

module dragon_curve;

// original dragon curve by Blake447 on shadertoy:
// https://www.shadertoy.com/view/WdGcW3

static const int ITERATIONS = 12;
static const float SIZE = 5.0;

// Smooth minimum (Inigo Quilez technique)
float smin(float a, float b, float k) {
    k *= 4.0;
    float h = max(k - abs(a - b), 0.0) / k;
    return min(a, b) - h * h * k * 0.25;
}

public float dragonCurveSDF(float2 uv) {
    for (int k = 0; k < ITERATIONS; k++) {
        uv = float2(uv.x + uv.y, uv.y - uv.x);
    }

    float2 uv_scaled = uv * SIZE * 0.5;

    // Continuous position for cell blending
    float2 cell_pos = uv_scaled + float2(0.5, 0.5);
    float2 cell_floor = floor(cell_pos);
    float2 frac = cell_pos - cell_floor;

    // Sample 4 neighboring cells and blend with smin
    float d00 = dragonCurveSDF_cell(uv_scaled, cell_floor + float2(0.0, 0.0));
    float d10 = dragonCurveSDF_cell(uv_scaled, cell_floor + float2(1.0, 0.0));
    float d01 = dragonCurveSDF_cell(uv_scaled, cell_floor + float2(0.0, 1.0));
    float d11 = dragonCurveSDF_cell(uv_scaled, cell_floor + float2(1.0, 1.0));

    // Blend using smooth minimum for continuous distance field
    const float blend_k = 0.1;
    float d = smin(smin(d00, d10, blend_k), smin(d01, d11, blend_k), blend_k);

    return d;
}

// Compute SDF for a single cell
float dragonCurveSDF_cell(float2 uv_scaled, float2 uv_midpoint) {
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

    // Raw divergence values for soft invalidation
    float div0 = length(v0 - c0);
    float div1 = length(v1 - c0);
    float div2 = length(v2 - c0);
    float div3 = length(v3 - c0);

    float d0_sdf = sdf0(uv_scaled, p0, uv_midpoint, div0);
    float d1_sdf = sdf0(uv_scaled, p1, uv_midpoint, div1);
    float d2_sdf = sdf0(uv_scaled, p2, uv_midpoint, div2);
    float d3_sdf = sdf0(uv_scaled, p3, uv_midpoint, div3);

    return min(min(d0_sdf, d1_sdf), min(d2_sdf, d3_sdf));
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

// Soft invalidation: blend based on divergence instead of hard cutoff
float sdf0(float2 uv, float2 p, float2 middle_point, float divergence) {
    float d = sdfLineSegment(uv, p, middle_point);

    const float threshold = 0.05;
    const float transitionWidth = 0.1;
    float blend = smoothstep(threshold - transitionWidth * 0.5,
                             threshold + transitionWidth * 0.5,
                             divergence);
    return lerp(d, max(d, 0.5), blend);
}

float sdfLineSegment(float2 p, float2 a, float2 b) {
    float2 pa = p - a;
    float2 ba = b - a;
    float h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}
