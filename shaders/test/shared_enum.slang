#language slang 2026

// A shared slang module holding *only* an enum — no struct, so the generated
// module file has no GPUWrite impl and its GPUWrite import must not warn.
module shared_enum;

public enum SharedMode : uint {
    Idle = 0,
    Busy = 1,
}
