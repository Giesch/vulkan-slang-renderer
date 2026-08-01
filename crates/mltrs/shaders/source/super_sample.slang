#language slang 2026

module super_sample;

public interface ISuperSampler {
    float3 sampleAtOffset(float2 subPixelOffset);
}

[ForceInline]
public float3 superSample<S : ISuperSampler, let N : int>(S sampler) {
    var sum = float3(0.0);
    [ForceUnroll]
    for (int m = 0; m < N; m++) {
        [ForceUnroll]
        for (int n = 0; n < N; n++) {
            let offset = (float2(m, n) + 0.5) / float(N);
            sum += sampler.sampleAtOffset(offset);
        }
    }
    return sum / float(N * N);
}
