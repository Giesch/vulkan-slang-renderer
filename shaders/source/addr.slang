#language slang 2026

module addr;

// A buffer device address pointing at std430-laid-out Ts.
// Rust-side counterpart: renderer::Addr<T> (src/renderer/addr.rs).
public typealias Addr<T> =
    Ptr<T, Access.ReadWrite, AddressSpace.Device, Std430DataLayout>;

// Read-only counterpart of Addr<T>; writes through it are compile errors.
// Rust-side counterpart: renderer::ReadAddr<T> (src/renderer/addr.rs).
// NOTE: this is distinct from ImmutablePtr, which is NEVER written to on the GPU
//   rather than only not written to by the shader with a ReadAddr/Access.Read
public typealias ReadAddr<T> =
    Ptr<T, Access.Read,      AddressSpace.Device, Std430DataLayout>;
