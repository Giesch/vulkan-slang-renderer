#language slang 2026

module font;

// Worst case text length: 10 digits + 3 commas for a u32 with comma grouping.
public static const uint MAX_TEXT_LENGTH = 13;

public interface BitMapFont {
    // On-screen size of a text block of `count` glyphs.
    float2 textSize(uint count);

    // Color of framebuffer pixel `position` (y down) for the text in
    // `chars[0 .. count)`, drawn with its top-left corner at `origin`
    // and scaled to fill `textSize`.
    float4 render(
        float2 position,
        float2 origin,
        float2 textSize,
        uint chars[MAX_TEXT_LENGTH],
        uint count
    );
}

// Glyph index of the comma; digits 0-9 are their own indices.
public static const uint GLYPH_COMMA = 10;

public struct GlyphText {
    // Glyph indices, least significant digit first; only chars[0 .. count) is set.
    public uint chars[MAX_TEXT_LENGTH];
    public uint count;

    [mutating]
    [ForceInline]
    void push(uint char) {
        this.chars[this.count] = char;
        this.count += 1;
    }
}

[ForceInline]
public func glyphsForUint(uint value) -> GlyphText {
    var text: GlyphText;
    text.count = 0;

    uint digits = 0;
    uint score = value;
    for (;;) {
        if (digits > 0 && digits % 3 == 0) {
            text.push(GLYPH_COMMA);
        }
        text.push(score % 10);
        digits += 1;
        score /= 10;
        if (score == 0) {
            break;
        }
    }

    return text;
}

// 3x5 bitmap digits, one uint per glyph, top row first.
// Leftmost pixel of a row is the MSB of its 3-bit group.
static const uint FONT[11] = {
    0b111_101_101_101_111, // 0
    0b010_110_010_010_111, // 1
    0b111_001_111_100_111, // 2
    0b111_001_111_001_111, // 3
    0b101_101_111_001_001, // 4
    0b111_100_111_001_111, // 5
    0b111_100_111_101_111, // 6
    0b111_001_001_001_001, // 7
    0b111_101_111_101_111, // 8
    0b111_101_111_001_111, // 9
    0b000_000_000_010_100, // comma (index 10)
};

static const uint GLYPH_WIDTH = 3;
static const uint GLYPH_HEIGHT = 5;
// Glyph width plus one spacing column.
static const uint CELL_ADVANCE = 4;

// Size of the text grid in font pixels; the last cell has no trailing
// spacing column.
func gridExtents(uint count) -> float2 {
    return float2(count * CELL_ADVANCE - 1.0, GLYPH_HEIGHT);
}

public struct Digits3x5Font : BitMapFont {
    // Screen pixels per font pixel; textSize() scales by it.
    public float pixelScale;
    public float4 lit;
    public float4 background;

    public float2 textSize(uint count) {
        return gridExtents(count) * pixelScale;
    }

    public float4 render(
        float2 position,
        float2 origin,
        float2 textSize,
        uint chars[MAX_TEXT_LENGTH],
        uint count
    ) {
        const float2 p = (position - origin) / textSize * gridExtents(count);

        // `position` is framebuffer coordinates, y down, which matches
        // the top-row-first glyph encoding. Guard before the int() casts:
        // truncation toward zero would fold -1 into column 0.
        if (p.x < 0.0 || p.y < 0.0) {
            return background;
        }

        const int col = int(p.x);
        const int row = int(p.y);

        const int cell = col / 4;
        const int glyphCol = col % 4;
        if (cell >= int(count) || glyphCol == 3 || row >= 5) {
            return background;
        }

        // chars is least-significant digit first, so the leftmost cell
        // is the last entry.
        const uint glyph = FONT[chars[count - uint(cell) - 1]];
        const int rowGroupOffset = (4 - row) * 3;
        const int columnOffset = 2 - glyphCol;
        const int bitOffset = rowGroupOffset + columnOffset;
        const uint bit = (glyph >> bitOffset) & 1;
        if (bit == 0) {
            return background;
        }

        return lit;
    }
}
