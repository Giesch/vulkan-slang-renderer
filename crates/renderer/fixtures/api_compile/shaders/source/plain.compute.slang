#language slang 2026
module plain_compute;

ParameterBlock<PlainParams> plainParams;

struct PlainParams {
    float scale;
}

[numthreads(1, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {}
