#language slang 2026

module particle;

public struct Particle {
    public float2 position;
    public float2 velocity;
}

// A second buffer element type, so a wrong buffer element type is a type error
// rather than a size coincidence.
public struct OtherElement {
    public float value;
}
