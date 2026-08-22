#language slang 2026

module bindless_options;

// see `lib.rs` and `docs/bindless.md`
export T getDescriptorFromHandle<T : IOpaqueDescriptor>(DescriptorHandle<T> handleValue)
{
    return defaultGetDescriptorFromHandle(nonuniform(handleValue), BindlessDescriptorOptions.None);
}
