#language slang 2026

module watercolor_common;

// Bilinear sample from a R32F texture at fractional position (in texel coordinates)
public float bilinearSampleR(
    Texture2D<float> texture,
    float2 position,
    float2 gridSize
) {
    float2 p = position - 0.5;
    int2 intPosition = int2(floor(p));
    float2 fracPosition = p - floor(p);
    int2 maxCell = int2(gridSize) - 1;

    int2 pos00 = clamp(intPosition,              int2(0), maxCell);
    int2 pos10 = clamp(intPosition + int2(1, 0), int2(0), maxCell);
    int2 pos01 = clamp(intPosition + int2(0, 1), int2(0), maxCell);
    int2 pos11 = clamp(intPosition + int2(1, 1), int2(0), maxCell);

    float val00 = texture[pos00];
    float val10 = texture[pos10];
    float val01 = texture[pos01];
    float val11 = texture[pos11];

    return lerp(
        lerp(val00, val10, fracPosition.x),
        lerp(val01, val11, fracPosition.x),
        fracPosition.y
    );
}

public bool inBounds(uint2 pixel, float2 gridSize) {
    return pixel.x < uint(gridSize.x) && pixel.y < uint(gridSize.y);
}

// Gaussian blur constants shared by wc_gaussian_blur and wc_blur_v_and_flow_outward

public static const int BLUR_RADIUS = 10;

// Precomputed weights for exp(-float(i * i) / (2.0 * SIGMA * SIGMA)) where SIGMA = 3.333
public static const float BLUR_WEIGHTS[11] = {
    1.0, 0.955997, 0.835214, 0.667013, 0.486752,
    0.324652, 0.197899, 0.110251, 0.056149, 0.026133, 0.011109
};
