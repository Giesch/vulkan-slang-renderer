#language slang 2026

module addr;

// A buffer device address pointing at std430-laid-out Ts.
//
// matches Rust's Addr<T> (src/renderer/addr.rs)
public typealias Addr<T> =
    Ptr<T, Access.ReadWrite, AddressSpace.Device, Std430DataLayout>;

// Read-only counterpart of Addr<T>; writes through it are compile errors.
// This is distinct from ImmutableAddr<T>;
// ReadAddr only promises that *this shader* cannot write through *this pointer*
//
// matches Rust's ReadAddr<T> (src/renderer/addr.rs).
public typealias ReadAddr<T> =
    Ptr<T, Access.Read,      AddressSpace.Device, Std430DataLayout>;

// A pointer to a buffer that the GPU never writes to
// The CPU may still update the buffer between frames
//
// emits SPIR-V Restrict, so it must not change during a shader execution (UB otherwise)
//
// matches Rust's ImmutableAddr<T> (src/renderer/addr.rs)
public typealias ImmutableAddr<T> =
    Ptr<T, Access.Immutable, AddressSpace.Device, Std430DataLayout>;
