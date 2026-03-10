#language slang 2026

module watercolor_common;

// Bilinear sample from a R32F texture at fractional position (in texel coords)
public float bilinearSampleR(Texture2D<float> tex, float2 position, float2 gridSize) {
    float2 p = position - 0.5;
    int2 intPosition = int2(floor(p));
    float2 fracPosition = p - floor(p);

    // Clamp to valid range
    int2 pos00 = clamp(intPosition, int2(0), int2(gridSize) - 1);
    int2 pos10 = clamp(intPosition + int2(1, 0), int2(0), int2(gridSize) - 1);
    int2 pos01 = clamp(intPosition + int2(0, 1), int2(0), int2(gridSize) - 1);
    int2 pos11 = clamp(intPosition + int2(1, 1), int2(0), int2(gridSize) - 1);

    float val00 = tex[pos00];
    float val10 = tex[pos10];
    float val01 = tex[pos01];
    float val11 = tex[pos11];

    return lerp(lerp(val00, val10, fracPosition.x), lerp(val01, val11, fracPosition.x), fracPosition.y);
}

// Bilinear sample from an RGBA32F texture at fractional position (in texel coords)
public float4 bilinearSampleRGBA(Texture2D<float4> tex, float2 position, float2 gridSize) {
    float2 p = position - 0.5;
    int2 intPosition = int2(floor(p));
    float2 fracPosition = p - floor(p);

    int2 pos00 = clamp(intPosition, int2(0), int2(gridSize) - 1);
    int2 pos10 = clamp(intPosition + int2(1, 0), int2(0), int2(gridSize) - 1);
    int2 pos01 = clamp(intPosition + int2(0, 1), int2(0), int2(gridSize) - 1);
    int2 pos11 = clamp(intPosition + int2(1, 1), int2(0), int2(gridSize) - 1);

    float4 val00 = tex[pos00];
    float4 val10 = tex[pos10];
    float4 val01 = tex[pos01];
    float4 val11 = tex[pos11];

    return lerp(lerp(val00, val10, fracPosition.x), lerp(val01, val11, fracPosition.x), fracPosition.y);
}

public bool inBounds(uint2 pixel, float2 gridSize) {
    return pixel.x < uint(gridSize.x) && pixel.y < uint(gridSize.y);
}

// Gaussian blur constants shared by wc_gaussian_blur and wc_blur_v_and_flow_outward
public static const float WET_MASK_THRESHOLD = 0.001;

public static const int BLUR_RADIUS = 10;

// Precomputed weights for exp(-float(i * i) / (2.0 * SIGMA * SIGMA)) where SIGMA = 3.333
public static const float BLUR_WEIGHTS[11] = {
    1.0, 0.955997, 0.835214, 0.667013, 0.486752,
    0.324652, 0.197899, 0.110251, 0.056149, 0.026133, 0.011109
};
