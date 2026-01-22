// translated to slang from: https://www.shadertoy.com/view/sdtcR2
//
// "Koch in 3d"
// by Martijn Steinrucken aka The Art of Code/BigWings - 2022
//
// The MIT License
// Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions: The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
// Email: countfrolic@gmail.com
// Twitter: @The_ArtOfCode
// YouTube: youtube.com/TheArtOfCodeIsCool
// Facebook: https://www.facebook.com/groups/theartofcode/
//
// To make this yourself, follow the tutorial on YouTube:
// https://youtu.be/BYv47pQPRDs
// NFT of this video:
// https://www.fxhash.xyz/generative/13884

#language slang 2026

module koch_curve;

import fullscreen_triangle;

#define MAX_STEPS 100
#define MAX_DIST 100.
#define SURF_DIST .001
#define TAU 6.283185
#define PI 3.141592

ParameterBlock<KochCurveParams> params;

struct KochCurveParams {
    float2 resolution;
    float2 mouse;
    float time;
    Sampler2D cubeMap;
}

struct FragInput {
    float4 position : SV_Position;
    float2 pixel : TEXCOORD0;
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    let pixel = fp.centeredCoords * params.resolution;
    return FragInput(fp.svPosition, pixel);
}

float2x2 Rot(float a) {
    float s=sin(a), c=cos(a);
    return float2x2(c, -s, s, c);
}

float sdBox(float3 p, float3 s) {
    p = abs(p)-s;
    return length(max(p, 0.))+min(max(p.x, max(p.y, p.z)), 0.);
}

float2 N(float angle) {
    // angle to vector
    return float2(sin(angle), cos(angle));
}

float2 Koch(float2 uv) {
    uv.x = abs(uv.x);

    float3 col = float3(0);
    float d;

    float angle = 0.;
    float2 n = N((5./6.)*3.1415);

    uv.y += tan((5./6.)*3.1415)*.5;
    d = dot(uv-float2(.5, 0), n);
    uv -= max(0.,d)*n*2.;

    float scale = 1.;

    n = N((2./3.)*3.1415);
    uv.x += .5;
    for(int i=0; i<4; i++) {
        uv *= 3.;
        scale *= 3.;
        uv.x -= 1.5;

        uv.x = abs(uv.x);
        uv.x -= .5;
        d = dot(uv, n);
        uv -= min(0.,d)*n*2.;
    }
    uv /= scale;
    return uv;
}


float GetDist(float3 p) {
    p.xz = mul(Rot(params.time * 0.2), p.xz);

    float2 xz = Koch(float2(length(p.xz), p.y));
    float2 yz = Koch(float2(length(p.yz), p.x));
    float2 xy = Koch(float2(length(p.xy), p.z));
    float d = max(xy.x, max(yz.x, xz.x));

    d = lerp(d, length(p)-.5, .5);
    return d;
}

float RayMarch(float3 ro, float3 rd) {
    float dO=0.;

    for(int i=0; i<MAX_STEPS; i++) {
        float3 p = ro + rd*dO;
        float dS = GetDist(p);
        dO += dS;
        if(dO>MAX_DIST || abs(dS)<SURF_DIST) break;
    }

    return dO;
}

float3 GetNormal(float3 p) {
    float d = GetDist(p);
    float2 e = float2(.001, 0);

    float3 n = d - float3(
        GetDist(p-e.xyy),
        GetDist(p-e.yxy),
        GetDist(p-e.yyx));

    return normalize(n);
}

float3 GetRayDir(float2 uv, float3 p, float3 l, float z) {
    float3 f = normalize(l-p),
        r = normalize(cross(float3(0,1,0), f)),
        u = cross(f,r),
        c = f*z,
        i = c + uv.x*r + uv.y*u,
        d = normalize(i);
    return d;
}

[shader("fragment")]
float4 mainImage(FragInput input)
{
    let fragCoord = input.pixel;

    float2 uv = fragCoord / params.resolution.y;
    float2 m = params.mouse.xy / params.resolution.xy;

    float3 ro = float3(0, 3, -3);
    ro.yz = mul(Rot(-m.y*PI+1.), ro.yz);
    ro.xz = mul(Rot(-m.x*TAU), ro.xz);

    float3 rd = GetRayDir(uv, ro, float3(0,0.,0),3.);
    float3 col = float3(0);

    float d = RayMarch(ro, rd);

    if(d<MAX_DIST) {
        float3 p = ro + rd * d;
        float3 n = GetNormal(p);
        float3 r = reflect(rd, n);

        float dif = dot(n, normalize(float3(1,2,3)))*.5+.5;
        col = float3(dif);

        col = n*.5+.5;
        col *= params.cubeMap.Sample(r.xy).rgb;
    }

    col = pow(col, float3(.4545)); // gamma correction

    return float4(col,1.0);
}
