#language slang 2026

module ownership_leaf;

public enum SharedMode : uint {
    Off = 0,
    On = 7,
}

public struct LeafData {
    public float4 color;
}
