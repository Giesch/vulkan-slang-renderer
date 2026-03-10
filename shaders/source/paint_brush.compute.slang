#language slang 2026

module paint_brush;

ParameterBlock<BrushParams> brushParams;

public struct StrokePoint {
    public float2 position;
}

struct BrushParams {
    RWTexture2D<float> wetMask;
    RWTexture2D<float> pressure;
    RWTexture2D<float4> pigment;
    RWTexture2D<float> saturation;
    StructuredBuffer<StrokePoint> strokePoints;
    uint pointCount;
    float brushRadius;
    float brushOpacity;
    float brushPressure;
    float4 pigmentColor; // concentration per channel (RGBA = 4 pigment species)
    float2 canvasSize;
}

[numthreads(16, 16, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    let pixel: uint2 = dispatchThreadID.xy;

    let outOfBounds = pixel.x >= uint(brushParams.canvasSize.x) || pixel.y >= uint(brushParams.canvasSize.y);
    if (outOfBounds) {
        return;
    }

    if (brushParams.pointCount == 0) {
        return;
    }

    let pixelCenter = float2(pixel) + 0.5;

    for (uint i = 0; i < brushParams.pointCount; i++) {
        let brushPos = brushParams.strokePoints[i].position;
        let distance = distance(pixelCenter, brushPos);

        if (distance > brushParams.brushRadius) {
            continue;
        }

        // Smooth falloff using smoothstep
        let normalizedDistance = distance / brushParams.brushRadius;
        let alpha = (1.0 - smoothstep(0.6, 1.0, normalizedDistance)) * brushParams.brushOpacity;

        // Mark wet area
        brushParams.wetMask[pixel] = 1.0;

        // Add pressure (drives initial outward flow)
        brushParams.pressure[pixel] += brushParams.brushPressure * alpha;

        // Saturate paper where painted
        brushParams.saturation[pixel] = 1.0;

        // Add pigment concentration
        brushParams.pigment[pixel] += brushParams.pigmentColor * alpha;
    }
}
