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
