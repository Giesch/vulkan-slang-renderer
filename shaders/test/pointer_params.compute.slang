#language slang 2026

module pointer_params_compute;

// Test: pointer field in a compute parameter block; exercises the compute
// template's layout asserts in check_crate (this is the only compute shader
// in shaders/test).

struct PtrItem {
    float2 a;   // 0
    float b;    // 8
}               // size 16, align 8

struct Params {
    LayoutPtr<PtrItem, Std430DataLayout> data;                        // 0
    Ptr<PtrItem, Access.Read, AddressSpace.Device, Std430DataLayout> src; // 8
    uint count;                                                       // 16
}

ParameterBlock<Params> params;

[numthreads(64, 1, 1)]
[shader("compute")]
void computeMain(uint3 dispatchThreadID : SV_DispatchThreadID) {
    uint index = dispatchThreadID.x;
    if (index >= params.count) {
        return;
    }

    PtrItem item = params.src[index];
    params.data[index].b = item.a.x + item.a.y;
}
