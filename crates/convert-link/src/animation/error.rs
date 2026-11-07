//! Typed parse errors for the J3D animation decoders.
//!
//! Each failure kind is a small struct carrying the offsets, counts and field
//! labels that identify it. The clip being parsed is not stored in the error:
//! the format entry points (`bck::parse`, `btp::parse`, `btk::parse`) attach
//! it as eros context, together with the joint/target/axis being decoded, so
//! `{:?}` shows the whole path while `{}` stays a single specific message.

use std::fmt;

/// Escaped rendering of a four-character code (`b"J3D1"` -> `J3D1`, bytes
/// outside printable ASCII as `\xNN`).
struct FourCc<'a>(&'a [u8; 4]);

impl fmt::Display for FourCc<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\"", self.0.escape_ascii())
    }
}

/// A four-character code in the J3D1 header (file magic, file type, or the
/// chunk tag) is not the one the dispatched format requires.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field} {} does not match expected {}", FourCc(.found), FourCc(.expected))]
pub struct FourCcMismatch {
    /// `"magic"`, `"file type"` or `"chunk"`.
    pub field: &'static str,
    pub found: [u8; 4],
    pub expected: [u8; 4],
}

/// The J3D1 single-chunk file header is structurally invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidJ3dHeader {
    #[error("{len} bytes is smaller than a J3D1 header + chunk tag")]
    TooShort { len: usize },
    #[error("header claims {declared} bytes but file is {actual}")]
    FileLength { declared: usize, actual: usize },
    #[error("expected exactly 1 chunk, found {found}")]
    ChunkCount { found: u32 },
    #[error("chunk size {size} is smaller than its own header")]
    ChunkTooSmall { size: u64 },
    #[error("chunk claims {size} bytes, ending {past} bytes past the {file_len}-byte file")]
    ChunkOverrun {
        size: u64,
        past: u64,
        file_len: usize,
    },
}

/// A read of `len` bytes at `offset` does not fit in the `available`-byte
/// buffer (chunk, table, pool, or whole file).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what}: {len} bytes at {offset:#x} exceed the {available:#x}-byte buffer")]
pub struct OutOfBounds {
    pub what: String,
    pub offset: usize,
    pub len: usize,
    pub available: usize,
}

/// An offset + length (or count × stride) computation overflowed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what}: range size overflows")]
pub struct RangeOverflow {
    pub what: String,
}

/// A key descriptor (or one of its words) reaches past its value pool.
/// `start..end` and `pool_words` are in pool words, not bytes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what}: key data {start}..{end} exceeds the {pool_words}-word pool")]
pub struct KeyDataOutOfPool {
    pub what: String,
    pub start: usize,
    pub end: usize,
    pub pool_words: usize,
}

/// A keyed track's times do not strictly increase. i16 times are widened
/// losslessly to f32.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{what}: key time {time} does not strictly increase (previous {previous})")]
pub struct KeyTimesNotIncreasing {
    pub what: String,
    pub time: f32,
    pub previous: f32,
}

/// A decoded f32 (key time, value, tangent, constant, or center component)
/// is NaN or infinite.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{what}: non-finite {field} {value}")]
pub struct NonFiniteValue {
    pub what: String,
    /// Which word, e.g. `"constant value"`, `"key 2 tangent-in"`.
    pub field: String,
    pub value: f32,
}

/// Why a name-table string could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringDefect {
    NotNulTerminated,
    NotUtf8,
}

impl fmt::Display for StringDefect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            StringDefect::NotNulTerminated => "is not NUL-terminated",
            StringDefect::NotUtf8 => "is not UTF-8/ASCII",
        })
    }
}

/// A NUL-terminated name string is malformed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what}: string at {offset:#x} {defect}")]
pub struct InvalidString {
    pub what: String,
    pub offset: usize,
    pub defect: StringDefect,
}

/// A table set's row count disagrees with its material-name table.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{what} has {rows} rows but {names} material names")]
pub struct NameCountMismatch {
    /// e.g. `"TPT1"`, `"TTK1 main set"`, `"TTK1 post set"`.
    pub what: &'static str,
    pub rows: usize,
    pub names: usize,
}

/// A header count violates its format rule (implausible joint count, track
/// count that is not a multiple of 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field} {count} {rule}")]
pub struct InvalidCount {
    pub field: &'static str,
    pub count: usize,
    /// The violated rule, phrased to follow the count, e.g.
    /// `"is not a multiple of 3"`.
    pub rule: &'static str,
}

/// A TTK1 post set is half present: a names offset without tracks, or
/// tracks without a names offset.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "inconsistent TTK1 post set (names offset {names_offset:#x}, post track count {track_count})"
)]
pub struct InconsistentPostSet {
    pub names_offset: usize,
    pub track_count: usize,
}

/// The BCK BAS sound trailer starts inside the declared ANK1 chunk.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("BAS trailer at {offset:#x} starts inside the declared chunk (ends {chunk_end:#x})")]
pub struct BasInsideChunk {
    pub offset: u64,
    pub chunk_end: u64,
}

/// Failures of a bounds-checked fixed-width read.
pub type ReadErrors = (OutOfBounds,);
/// Failures of a bounds-checked sub-slice.
pub type PoolErrors = (OutOfBounds, RangeOverflow);
/// Failures of [`super::j3d_single_chunk`].
pub type HeaderErrors = (InvalidJ3dHeader, FourCcMismatch, OutOfBounds);
/// Failures of decoding one key track against its pool.
pub type TrackErrors = (KeyDataOutOfPool, KeyTimesNotIncreasing, NonFiniteValue);
/// Failures of reading a name string or ResNTAB name table.
pub type StringErrors = (OutOfBounds, InvalidString);
/// Failures of locating the BCK BAS trailer.
pub type BasErrors = (OutOfBounds, BasInsideChunk);

/// Every way a BCK/BTP/BTK clip can fail to parse.
pub type AnimationParseErrors = (
    InvalidJ3dHeader,
    FourCcMismatch,
    OutOfBounds,
    RangeOverflow,
    KeyDataOutOfPool,
    KeyTimesNotIncreasing,
    NonFiniteValue,
    InvalidString,
    NameCountMismatch,
    InvalidCount,
    InconsistentPostSet,
    BasInsideChunk,
);
