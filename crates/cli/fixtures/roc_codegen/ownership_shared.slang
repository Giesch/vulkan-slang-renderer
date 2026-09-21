#language slang 2026

module ownership_shared;

import ownership_leaf;

public struct SharedPayload {
    public LeafData data;
    public SharedMode mode;
}
