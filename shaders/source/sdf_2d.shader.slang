#language slang 2026

module sdf_2d;

// A partial Slang implementation of this blog post:
// https://danielchasehooper.com/posts/code-animated-rick

ParameterBlock<SDF2DParams> params;

struct SDF2DParams {
    float2 resolution;
    float time;
}

struct VertexOutput {
    float4 position : SV_Position;
    float2 pixel : TEXCOORD0;
}

static const float2 fullScreenTriangleVerts[3] = {
    {0.0, 0.0},
    {0.0, 2.0},
    {2.0, 0.0},
};

static const float2x2 reflectY = float2x2(
    1.0, 0.0,
    0.0, -1.0,
);

[shader("vertex")]
VertexOutput vertMain(uint id : SV_VertexID) {
    let texCoord = fullScreenTriangleVerts[id];

    let position = float4(texCoord * 2.0 + -1.0, 0.0, 1.0);

    var pixel = mul(reflectY, texCoord); // reflect Y-axis to match OpenGL
    pixel = pixel + float2(0.0, 1.0); // move origin to center
    pixel *= params.resolution; // scale to screen

    return VertexOutput(position, pixel);
}

[shader("fragment")]
float4 fragMain(VertexOutput input) : SV_Target {
    let res = params.resolution;

    let rootSamples = 3; // 9 samples per pixel
    var sum = float3(0.0);
    for (int m = 0; m < rootSamples; m++) {
        for (int n = 0; n < rootSamples; n++) {
            let offset = (float2(m, n) + 0.5) / float(rootSamples);
            let sampleCenter = ((2.0 * input.pixel + offset) - res) / res.y;
            sum += colorForPixel(sampleCenter, params.time);
        }
    }

    let sampleCount = rootSamples * rootSamples;
    let averageColor = sum / float(sampleCount);

    return float4(averageColor, 1.0);
}

func colorForPixel(pixel: float2, time: float) -> float3 {
    // return pastelCircles(pixel, time);

    // return somePrimitives(pixel, time);

    return panningCircles(pixel, time);
}

// examples

func pastelCircles(pixel: float2, time: float) -> float3 {
    if (circle(pixel - float2(0.3, -0.3), 0.4) < 0.0) {
        return float3(0.2, 0.7, 0.5);
    }

    if (circle(pixel - float2(-0.4, 0.0), 0.8) < 0.0) {
        return float3(0.7, 0.5, 0.3);
    }

    return float3(0.2);
}

func somePrimitives(pixel: float2, time: float) -> float3 {
    let bezierDist = bezier(
        pixel,
        float2(-0.7, -0.35),
        float2(-1.5, -0.4),
        float2(-1.2, 0.35)
    );
    if (bezierDist < 0.1) {
        return float3(0.9, 0.3, 0.3);
    }

    if (roundRect(pixel, float2(0.3, 0.4), float4(0.1)) < 0.0) {
        return float3(0.2, 0.9, 0.3);
    }

    if (star(pixel - float2(1.0, 0.0), 0.45, 5.0, 0.3) < 0.0) {
        return float3(0.2, 0.4, 0.9);
    }

    return float3(1.0);
}

// a tweaked version of the super-sampling demo at the end of the post
func panningCircles(pixel: float2, time: float) -> float3 {
    let tileWidth = 0.5;
    let slowMo = 0.05;

    let background = float3(0.8, 1.0, 1.0);
    let foreground = float3(0.02, 0.16, 0.1);

    if (axes(fmod(pixel, tileWidth)) < 0.002) {
        return background;
    }

    let time = (time + 1.0 / slowMo) * slowMo;
    let tiledPixel = fmod(pixel + time, tileWidth);
    let radius = tileWidth / 2.0 - tileWidth / 10.0;

    if (circle(tiledPixel - tileWidth / 2.0, radius) < 0) {
        return foreground;
    }

    return background;
}

// primitives

func circle(pixel: float2, radius: float) -> float {
    return length(pixel) - radius;
}

func bezier(p: float2, c0: float2, c1: float2, c2: float2) -> float {
    // https://www.shadertoy.com/view/MlKcDD

    float2 i = c0 - c2;
    float2 j = c2 - c1;
    float2 k = c1 - c0;
    float2 w = j - k;

    var v0 = c0 - p;
    var v1 = c1 - p;
    var v2 = c2 - p;

    float x = v0.x * v2.y - v0.y * v2.x;
    float y = v1.x * v0.y - v1.y * v0.x;
    float z = v2.x * v1.y - v2.y * v1.x;

    float2 s = 2.0 * (y * j + z * k) - x * i;

    float r = (y * z - x * x * 0.25) / dot(s, s);
    float t = clamp(
        (0.5 * x + y + r * dot(s, w)) / (x + y + z),
        0.0,
        1.0
    );

    float2 d = v0 + t * (k + k + t * w);

    return length(d);
}

static const float pi = 3.141593;

func star(p: float2, float r, float points, float ratio) -> float {
    // https://www.shadertoy.com/view/MlKcDD

    var p = p;

    float an = pi / points;
    float en = pi / (ratio * (points - 2.0) + 2.0);
    float2 acs = float2(cos(an), sin(an));
    float2 ecs = float2(cos(en), sin(en));

    // Compared to GLSL's two-argument atan() overload,
    // HLSL's atan2() has a reversed argument order.
    // HLSL's fmod() also handles negative numbers differently to GLSL's mod().
    float bn = fmod(atan2(abs(p.y), p.x), 2.0 * an) - an;
    p = length(p) * float2(cos(bn), abs(sin(bn)));
    p -= r * acs;
    p += ecs * clamp( -dot(p, ecs), 0.0, r * acs.y / ecs.y );

    return length(p) * sign(p.x);
}

func roundRect(float2 p, float2 size, float4 radii) -> float {
    // https://www.shadertoy.com/view/4llXD7

    var radii = radii;

    radii.xy = (p.x > 0) ? radii.xy : radii.zw;
    radii.x = (p.y > 0.0) ? radii.x : radii.y;

    float2 q = abs(p) - size + radii.x;

    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radii.x;
}

func axes(pixel: float2) -> float {
    return min(abs(pixel.x), abs(pixel.y));
}
