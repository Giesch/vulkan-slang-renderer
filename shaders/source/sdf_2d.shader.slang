#language slang 2026

module sdf_2d;

import fullscreen_triangle;

// A partial Slang implementation of this blog post:
// https://danielchasehooper.com/posts/code-animated-rick

ParameterBlock<SDF2DParams> params;

struct SDF2DParams {
    float2 resolution;
    float time;
}

struct FragInput {
    float4 position : SV_Position;
    float2 pixel : TEXCOORD0;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let p = fullscreenPosition(id, params.resolution);
    return FragInput(p.svPosition, p.pixel);
}

[shader("fragment")]
float4 fragMain(FragInput input) : SV_Target {
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

    let screenWidth = 2.0;
    let time = (time + screenWidth / slowMo) * slowMo;
    let tiledPixel = fmod(pixel + time, tileWidth);
    let radius = tileWidth / 2.0 - tileWidth / 10.0;

    let warpingAnimation = true;
    if (warpingAnimation) {
        let warpStrength = lerp(0.0, 0.25, sin(time * 20.0) - 1.0);
        let warpedTiledPixel = warp(tiledPixel, 4.0, warpStrength);
        if (circle(warpedTiledPixel - tileWidth / 2.0, radius) < 0) {
            return foreground;
        }
    } else {
        if (circle(tiledPixel - tileWidth / 2.0, radius) < 0) {
            return foreground;
        }
    }

    return background;
}

// primitives

func circle(pixel: float2, radius: float) -> float {
    return length(pixel) - radius;
}

// https://www.shadertoy.com/view/MlKcDD
func bezier(p: float2, c0: float2, c1: float2, c2: float2) -> float {
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
    float t = saturate(
        (0.5 * x + y + r * dot(s, w)) / (x + y + z),
    );

    float2 d = v0 + t * (k + k + t * w);

    return length(d);
}

static const float pi = 3.141593;

// https://www.shadertoy.com/view/MlKcDD
func star(p: float2, float r, float points, float ratio) -> float {
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

// https://www.shadertoy.com/view/4llXD7
func roundRect(float2 p, float2 size, float4 radii) -> float {
    var radii = radii;

    radii.xy = (p.x > 0) ? radii.xy : radii.zw;
    radii.x = (p.y > 0.0) ? radii.x : radii.y;

    float2 q = abs(p) - size + radii.x;

    return min(max(q.x, q.y), 0.0) + length(max(q, 0.0)) - radii.x;
}

func axes(pixel: float2) -> float {
    return min(abs(pixel.x), abs(pixel.y));
}

// https://www.shadertoy.com/view/XdXGW8
func grad(z: int2) -> float2 {
    // convert to 1D
    var n: int = z.x + z.y * 11111;
    // Hugo Elias hash
    n = (n << 13)^n;
    n = (n * (n*n * 15731 + 789221) + 1376312589) >> 16;

    // Perlin style vectors
    n &= 7;
    float2 gr = float2(n&1, n>>1) * 2.0 - 1.0;

    return (n >= 6) ? float2(0.0, gr.x) :
           (n >= 4) ? float2(gr.x, 0.0) :
           gr;
}

// https://www.shadertoy.com/view/XdXGW8
func noise(p: float2) -> float {
    let i = int2(floor(p));
    let f: float2 = fract(p);
    let u: float2 = f*f * (3.0 - 2.0 * f);

    return lerp( lerp( dot( grad( i+int2(0,0) ), f - float2(0.0,0.0) ),
                       dot( grad( i+int2(1,0) ), f - float2(1.0,0.0) ), u.x),
                 lerp( dot( grad( i+int2(0,1) ), f - float2(0.0,1.0) ),
                       dot( grad( i+int2(1,1) ), f - float2(1.0,1.0) ), u.x), u.y);
}

func warp(p: float2, scale: float, strength: float) -> float2 {
    let offsetX: float = noise(p * scale + float2(0.0, 100.0));
    let offsetY: float = noise(p * scale + float2(100.0, 0.0));
    return p + float2(offsetX, offsetY) * strength;
}

// https://www.shadertoy.com/view/ws3GD7
func parabola(pos: float2, k: float) -> float {
    var pos = pos;
    pos.x = abs(pos.x);

    let ik = 1.0 / k;
    let p = ik * (pos.y - 0.5 * ik) / 3.0;
    let q = 0.25 * ik*ik * pos.x;
    let h = q*q - p*p*p;
    let r = sqrt(abs(h));
    let x = h > 0.0
        ? pow(q + r, 1.0 / 3.0) - pow(abs(q - r), 1.0 / 3.0) * sign(r - q)
        : 2.0 * cos(atan2(r, q) / 3.0) * sqrt(p);

    return length(pos - float2(x, k * x*x)) * sign(pos.x - x);
}
