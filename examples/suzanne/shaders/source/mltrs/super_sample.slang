#language slang 2026

module super_sample;

namespace mltrs {

public interface ISuperSampler {
    float3 sampleAtOffset(float2 subPixelOffset);
}

static const int sides = 3;

[ForceInline]
public float3 superSample<S : ISuperSampler>(S sampler) {
    var sum = float3(0.0);

    [ForceUnroll]
    for (int m = 0; m < sides; m++) {
        [ForceUnroll]
        for (int n = 0; n < sides; n++) {
            let offset = (float2(m, n) + 0.5) / float(sides);
            sum += sampler.sampleAtOffset(offset);
        }
    }

    return sum / float(sides * sides);
}

}
