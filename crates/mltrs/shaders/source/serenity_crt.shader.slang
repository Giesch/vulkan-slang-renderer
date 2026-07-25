#language slang 2026

// https://blog.gingerbeardman.com/2026/01/04/webgl-crt-shader/
// https://gist.github.com/gingerbeardman/7392ee84fdb2e405d7437b5b12e4c12d

module serenity_crt;

import fullscreen_triangle;

ParameterBlock<SerenityCRTParams> params;

struct SerenityCRTParams {
    Sampler2D tex;
    float2 resolution;

    float scanlineIntensity; // Adjust intensity (0-1)
    float scanlineCount; // Number of scanlines
    float time; // For flicker effect
    float yOffset; // Vertical drift to combat moiré pattern
    float brightness; // Overall brightness
    float contrast; // Contrast adjustment
    float saturation; // Color saturation
    float bloomIntensity; // Bloom effect intensity
    float bloomThreshold; // Brightness threshold for bloom
    float rgbShift; // RGB color separation amount
    float adaptiveIntensity; // Adapt scanlines to reduce moiré (0-1)
    float vignetteStrength; // Vignette darkness (0-1)
    float curvature; // Screen curvature amount
    float flickerStrength; // Flicker intensity
}

[shader("vertex")]
FragInput vertMain(uint id : SV_VertexID) {
    let fp = fullscreenPosition(id);
    return FragInput(fp.svPosition, fp.texCoord);
}

struct FragInput {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
}

[shader("fragment")]
float4 effect(FragInput input) {
    var uv = input.uv;
    params.tex.Sample(uv);

    // Apply screen curvature if enabled
    if (params.curvature > 0.0) {
        uv = curveRemapUV(uv, params.curvature);
    }

    uv = uv * (params.resolution / params.resolution.y);

    // TODO is there a better way to avoid tiling?
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return float4(0.0, 0.0, 0.0, 1.0);
    }

    // Get the original pixel color with curved coordinates
    float4 pixel = params.tex.Sample(uv);

    // Apply bloom effect early
    if (params.bloomIntensity > 0.0) {
        float4 bloomSample = sampleBloom(params.tex, uv, 0.005);
        // Apply brightness to bloom sample to match pixel processing
        bloomSample.rgb *= params.brightness;
        float lum = dot(bloomSample.rgb, float3(0.299, 0.587, 0.114));
        // Add bloom with a subtle base bleed, amplified above threshold
        float bloomFactor = params.bloomIntensity * max(0.1, min(1.0, (lum - params.bloomThreshold) * 2.0));
        pixel.rgb += bloomSample.rgb * bloomFactor;
    }

    // Apply RGB shift (chromatic aberration)
    if (params.rgbShift > 0.0) {
        float shift = params.rgbShift * 0.01;
        pixel.r += params.tex.Sample(float2(uv.x + shift, uv.y)).r * 0.1;
        pixel.b += params.tex.Sample(float2(uv.x - shift, uv.y)).b * 0.1;
    }

    // Apply brightness
    pixel.rgb *= params.brightness;

    // Apply contrast
    pixel.rgb = (pixel.rgb - 0.5) * params.contrast + 0.5;

    // Apply saturation adjustment
    float luminance = dot(pixel.rgb, float3(0.299, 0.587, 0.114));
    pixel.rgb = lerp(float3(luminance), pixel.rgb, params.saturation);

    // Calculate scanlines with anti-moire adjustments
    float scanline = 1.0;
    if (params.scanlineIntensity > 0.0) {
        float scanlineY = (uv.y + params.yOffset) * params.scanlineCount;
        float scanlinePattern = abs(sin(scanlineY * 3.14159));
        float adaptiveFactor = 1.0;
        if (params.adaptiveIntensity > 0.0) {
            float yPattern = sin(uv.y * 30.0) * 0.5 + 0.5;
            adaptiveFactor = 1.0 - yPattern * params.adaptiveIntensity * 0.2;
        }
        scanline = 1.0 - scanlinePattern * params.scanlineIntensity * adaptiveFactor;
    }

    // Apply flicker effect
    float flicker = 1.0 + sin(params.time * 110.0) * params.flickerStrength;

    // FIXME
    pixel.rgb *= scanline * flicker;
    return pixel;

    // Apply vignette
    float vignette = 1.0;
    if (params.vignetteStrength > 0.0) {
        float2 vignetteCoordOGL = uv * 2.0 - 1.0;
        float2 vignetteCoord = vignetteCoordOGL - 2.0;
        float vignetteDistance = max(abs(vignetteCoord.x), abs(vignetteCoord.y));
        vignette = 1.0 - pow(vignetteDistance, 3.0) * params.vignetteStrength;
    }

    // Apply combined lighting effects
    pixel.rgb *= scanline * flicker * vignette;

    return pixel;
}

// Safer curvature function
float2 curveRemapUV(float2 uv, float curvature) {
    // Convert from 0-1 to -1 to 1
    float2 coords = uv * 2.0 - 1.0;

    // Calculate curvature effect - limiting the maximum distortion
    float curveAmount = curvature * 0.5; // Scale down to avoid extreme distortion

    // Compute distortion with squared distance from center
    float dist = coords.x * coords.x + coords.y * coords.y;
    coords = coords * (1.0 + dist * curveAmount);

    // Convert back to 0-1 range
    return coords * 0.5 + 0.5;
}

// Bloom sampling (unchanged from 3x3 Gaussian)
float4 sampleBloom(Sampler2D tex, float2 uv, float radius) {
    float4 bloom = float4(0.0);
    float weight = 1.0 / 16.0;

    bloom += tex.Sample(uv + float2(-radius, -radius)) * (1.0 * weight);
    bloom += tex.Sample(uv + float2(0.0, -radius)) * (2.0 * weight);
    bloom += tex.Sample(uv + float2(radius, -radius)) * (1.0 * weight);
    bloom += tex.Sample(uv + float2(-radius, 0.0)) * (2.0 * weight);
    bloom += tex.Sample(uv + float2(0.0, 0.0)) * (4.0 * weight);
    bloom += tex.Sample(uv + float2(radius, 0.0)) * (2.0 * weight);
    bloom += tex.Sample(uv + float2(-radius, radius)) * (1.0 * weight);
    bloom += tex.Sample(uv + float2(0.0, radius)) * (2.0 * weight);
    bloom += tex.Sample(uv + float2(radius, radius)) * (1.0 * weight);

    return bloom;
}
