#language slang 2026

module addr;

// A buffer device address pointing at std430-laid-out Ts.
// Rust-side counterpart: renderer::Addr<T> (src/renderer/addr.rs).
public typealias Addr<T> = LayoutPtr<T, Std430DataLayout>;
