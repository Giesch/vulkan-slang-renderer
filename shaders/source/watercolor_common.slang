#language slang 2026

module watercolor_common;

// Bilinear sample from a R32F texture at fractional position (in texel coords)
public float bilinearSampleR(Texture2D<float> tex, float2 pos, float2 gridSize) {
    float2 p = pos - 0.5;
    int2 ip = int2(floor(p));
    float2 fp = p - floor(p);

    // Clamp to valid range
    int2 p00 = clamp(ip, int2(0), int2(gridSize) - 1);
    int2 p10 = clamp(ip + int2(1, 0), int2(0), int2(gridSize) - 1);
    int2 p01 = clamp(ip + int2(0, 1), int2(0), int2(gridSize) - 1);
    int2 p11 = clamp(ip + int2(1, 1), int2(0), int2(gridSize) - 1);

    float v00 = tex[p00];
    float v10 = tex[p10];
    float v01 = tex[p01];
    float v11 = tex[p11];

    return lerp(lerp(v00, v10, fp.x), lerp(v01, v11, fp.x), fp.y);
}

// Bilinear sample from an RGBA32F texture at fractional position (in texel coords)
public float4 bilinearSampleRGBA(Texture2D<float4> tex, float2 pos, float2 gridSize) {
    float2 p = pos - 0.5;
    int2 ip = int2(floor(p));
    float2 fp = p - floor(p);

    int2 p00 = clamp(ip, int2(0), int2(gridSize) - 1);
    int2 p10 = clamp(ip + int2(1, 0), int2(0), int2(gridSize) - 1);
    int2 p01 = clamp(ip + int2(0, 1), int2(0), int2(gridSize) - 1);
    int2 p11 = clamp(ip + int2(1, 1), int2(0), int2(gridSize) - 1);

    float4 v00 = tex[p00];
    float4 v10 = tex[p10];
    float4 v01 = tex[p01];
    float4 v11 = tex[p11];

    return lerp(lerp(v00, v10, fp.x), lerp(v01, v11, fp.x), fp.y);
}

public bool inBounds(uint2 pixel, float2 gridSize) {
    return pixel.x < uint(gridSize.x) && pixel.y < uint(gridSize.y);
}
