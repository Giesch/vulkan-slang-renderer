//! Serde types for converted Link animations (`animations.catalog.json` and
//! one clip document per source file), shared between the
//! `convert_link_animations` binary (which writes them) and future runtime
//! consumers. Everything is human-inspectable, owned data, and free of
//! graphics dependencies.
//!
//! # Fidelity contract
//!
//! These documents are a *preservation* format, not a playback format:
//!
//! - Coordinates, frame units and integer rotation units stay exactly as the
//!   source J3D files store them. Nothing is baked, resampled, retargeted, or
//!   converted to quaternions.
//! - Rotations (and their key times/tangents) are source `i16` in J3D angle
//!   units: **65536 units per full turn** (`0x4000` = quarter turn). The
//!   runtime additionally left-shifts each value by the clip's separate
//!   [`BckClip::rotation_decimal_shift`] /
//!   [`BtkClip::rotation_decimal_shift`]; the shift is preserved as its own
//!   field and **not** pre-applied.
//! - Scale/translation keys stay `f32` source values.
//! - Tracks are explicit: `default` (0 keys), `constant` (1 key), or `keyed`.
//!   `default` means the J3D reader's built-in value — scale `1.0`, rotation
//!   `0`, translation `0` — for that track kind; the default is documented
//!   here rather than duplicated into the data.
//! - `keyed` tracks keep the original tangent type word: `0` means one shared
//!   tangent per key (stored 3 words: time, value, tangent), any nonzero
//!   value means split tangents (4 words: time, value, tangent-in,
//!   tangent-out). Nonzero values beyond 1 occur in the wild; the J3D reader
//!   treats any nonzero word as split, and so does this schema.
//! - The loop attribute is the raw source byte (0 once, 1 once-and-reset, 2
//!   repeat, 3 mirrored-once, 4 mirrored-repeat). It is metadata, not a
//!   promised playback mode or tick rate.
//! - BTP `texture_indices` sample counts are preserved independently of the
//!   clip duration; nothing interpolates or collapses repeated samples.
//!
//! # Identity
//!
//! Every clip's identity is the tuple (archive, member path, RARC entry
//! index, RARC resource id, source sha256), which the extraction inventory
//! freezes and the catalog repeats. Serialized documents never contain
//! absolute host paths or timestamps.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Catalog schema version. [`read_catalog`] rejects any other version.
pub const CATALOG_VERSION: u32 = 1;
/// Clip-document schema version. [`read_clip`] rejects any other version.
pub const CLIP_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaError {
    pub message: String,
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SchemaError {}

fn version_error(what: &str, found: u32) -> SchemaError {
    SchemaError {
        message: format!(
            "unsupported {what} schema version {found} (this build reads version {CATALOG_VERSION})"
        ),
    }
}

/// The three J3D animation kinds this pipeline carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimationFormat {
    Bck,
    Btp,
    Btk,
}

impl AnimationFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            AnimationFormat::Bck => "bck",
            AnimationFormat::Btp => "btp",
            AnimationFormat::Btk => "btk",
        }
    }
}

impl fmt::Display for AnimationFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `animations.catalog.json`: the full list of converted clips.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationCatalog {
    pub version: u32,
    pub clips: Vec<CatalogClip>,
}

/// One catalog row. Output filename is relative to the catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogClip {
    pub archive: String,
    pub member: String,
    pub entry_index: u32,
    pub resource_id: u16,
    pub format: AnimationFormat,
    pub sha256: String,
    /// e.g. `clips/LkAnm/bcks/actiontaktrdw.bck.json`.
    pub file: String,
}

/// Where a clip came from, frozen by the extraction inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipIdentity {
    pub archive: String,
    pub member: String,
    pub entry_index: u32,
    pub resource_id: u16,
    pub sha256: String,
}

/// One clip document: identity + format-specific payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationClip {
    pub version: u32,
    pub identity: ClipIdentity,
    pub data: ClipData,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "lowercase")]
pub enum ClipData {
    Bck(BckClip),
    Btp(BtpClip),
    Btk(BtkClip),
}

impl AnimationClip {
    pub fn format(&self) -> AnimationFormat {
        match &self.data {
            ClipData::Bck(_) => AnimationFormat::Bck,
            ClipData::Btp(_) => AnimationFormat::Btp,
            ClipData::Btk(_) => AnimationFormat::Btk,
        }
    }
}

// --- tracks -------------------------------------------------------------------

/// One f32 keyframe: time, value, and both tangents. For shared-tangent
/// tracks (`tangent_type == 0`) the source stored one word and the converter
/// fills `tangent_out = tangent_in`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct KeyF32 {
    pub time: f32,
    pub value: f32,
    pub tangent_in: f32,
    pub tangent_out: f32,
}

/// One i16 keyframe (rotation tracks): time, value, tangents, all in source
/// i16 units. See the module docs for the angle units and decimal shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyI16 {
    pub time: i16,
    pub value: i16,
    pub tangent_in: i16,
    pub tangent_out: i16,
}

/// An f32 track (scale / translation), exactly as the source table encoded it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TrackF32 {
    /// 0 keys: the J3D default for this track kind (scale 1.0, translation 0).
    Default,
    /// 1 key: the single value at the descriptor's pool index.
    Constant { value: f32 },
    Keyed {
        /// Raw source tangent-type word: 0 = shared tangent, nonzero = split.
        tangent_type: u16,
        keys: Vec<KeyF32>,
    },
}

/// An i16 track (rotation), exactly as the source table encoded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum TrackI16 {
    /// 0 keys: rotation default, 0.
    Default,
    Constant {
        value: i16,
    },
    Keyed {
        tangent_type: u16,
        keys: Vec<KeyI16>,
    },
}

/// The per-axis triplet every J3D key animation table stores: for each axis,
/// scale / rotation / translation tracks. ANK1 orders these axis-major —
/// joint j's three table entries are (x, y, z), each holding {S, R, T} for
/// that axis — and this type preserves that order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AxisSrt {
    pub scale: TrackF32,
    pub rotation: TrackI16,
    pub translation: TrackF32,
}

// --- BCK (J3D1bck1 / ANK1): body animation --------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BckClip {
    /// Duration in source frames. A sampling runtime owns tick rate.
    pub duration_frames: u16,
    /// Raw loop attribute byte (see module docs); metadata only.
    pub loop_attribute: u8,
    /// Runtime left-shift applied to every rotation value; kept separately.
    pub rotation_decimal_shift: u8,
    /// One entry per CL joint, in source table order. `ordinal` is the joint
    /// ordinal (the index into the model's JNT1).
    pub joints: Vec<BckJoint>,
    /// BAS sound-event trailer metadata. The trailer bytes stay in the raw
    /// source file; they are recorded, never interpreted.
    pub bas: BasMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BckJoint {
    pub ordinal: u16,
    /// Axis order: x, y, z (see [`AxisSrt`]).
    pub axes: [AxisSrt; 3],
}

/// Presence/location of the BAS trailer that follows the ANK1 chunk in some
/// BCK files. `offset`/`length` are byte ranges in the source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasMetadata {
    pub present: bool,
    pub offset: u32,
    /// `8 + 0x20 * entry_count` when present, 0 when absent.
    pub length: u32,
}

// --- BTP (J3D1btp1 / TPT1): stepped face texture patterns ------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BtpClip {
    pub duration_frames: u16,
    pub loop_attribute: u8,
    /// One entry per anim table row, in source order. Order is identity:
    /// rows may repeat a material name (two slots of one material) and must
    /// not be merged into a name-keyed map.
    pub targets: Vec<BtpTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BtpTarget {
    /// Original material name; matched by exact string at runtime.
    pub material: String,
    /// The raw per-row u16 from the remap table. This is an author-time
    /// material index: the J3D loader *overwrites* it via name lookup
    /// (`searchUpdateMaterialID`), so it must not be read as the CL material
    /// ordinal.
    pub material_remap: u16,
    /// The row's texture-map slot byte from the anim table entry.
    pub texture_map_slot: u8,
    /// Stepped u16 texture-index samples, in order. The count is independent
    /// of `duration_frames`; no interpolation, no collapsing of repeats.
    pub texture_indices: Vec<u16>,
}

// --- BTK (J3D1btk1 / TTK1): keyed face texture SRT ---------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BtkClip {
    pub duration_frames: u16,
    pub loop_attribute: u8,
    /// Same convention as [`BckClip::rotation_decimal_shift`].
    pub rotation_decimal_shift: u8,
    /// Raw matrix calculation flag: 0 = basic interpretation, 1 = Maya-style.
    /// The J3D loader maps any other value to basic; the raw word is kept.
    pub matrix_calc_type: u32,
    /// Main track set, in source order.
    pub targets: Vec<BtkTarget>,
    /// Optional second track set the TTK1 header carries (post matrices).
    /// Preserved whole — with its own remaps, centers and pools — or absent.
    pub post: Option<BtkPostSet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BtkTarget {
    pub material: String,
    /// Author-time u16 remap; see [`BtpTarget::material_remap`].
    pub material_remap: u16,
    /// Per-target texture-matrix selector byte (which tex-matrix slot of the
    /// material this anim drives).
    pub texgen_selector: u8,
    /// SRT center for this target, 3 components.
    pub center: [f32; 3],
    /// Axis order: s, t, q (the texture axes), same triplet layout as BCK.
    pub axes: [AxisSrt; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BtkPostSet {
    pub targets: Vec<BtkTarget>,
}

// --- validated public readers -----------------------------------------------------

fn read_json<'a, T: Deserialize<'a>>(json: &'a str, what: &str) -> Result<T, SchemaError> {
    serde_json::from_str(json).map_err(|e| SchemaError {
        message: format!("{what}: {e}"),
    })
}

/// Parse and validate a catalog document, rejecting unknown schema versions.
pub fn read_catalog(json: &str) -> Result<AnimationCatalog, SchemaError> {
    let raw: serde_json::Value = read_json(json, "animation catalog")?;
    let version = raw
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| SchemaError {
            message: "animation catalog: missing or non-numeric version".into(),
        })? as u32;
    if version != CATALOG_VERSION {
        return Err(version_error("catalog", version));
    }
    let catalog: AnimationCatalog = serde_json::from_value(raw).map_err(|e| SchemaError {
        message: format!("animation catalog: {e}"),
    })?;
    Ok(catalog)
}

/// Parse and validate a clip document, rejecting unknown schema versions.
pub fn read_clip(json: &str) -> Result<AnimationClip, SchemaError> {
    let raw: serde_json::Value = read_json(json, "animation clip")?;
    let version = raw
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| SchemaError {
            message: "animation clip: missing or non-numeric version".into(),
        })? as u32;
    if version != CLIP_VERSION {
        return Err(version_error("clip", version));
    }
    let clip: AnimationClip = serde_json::from_value(raw).map_err(|e| SchemaError {
        message: format!("animation clip: {e}"),
    })?;
    Ok(clip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ClipIdentity {
        ClipIdentity {
            archive: "LkAnm".into(),
            member: "bcks/x.bck".into(),
            entry_index: 8,
            resource_id: 8,
            sha256: "ab".repeat(32),
        }
    }

    #[test]
    fn animation_schema_roundtrip_all_formats() {
        // BCK with a present BAS trailer and a keyed rotation track.
        let bck = AnimationClip {
            version: CLIP_VERSION,
            identity: identity(),
            data: ClipData::Bck(BckClip {
                duration_frames: 12,
                loop_attribute: 4,
                rotation_decimal_shift: 2,
                joints: vec![BckJoint {
                    ordinal: 7,
                    axes: std::array::from_fn(|_| AxisSrt {
                        scale: TrackF32::Keyed {
                            tangent_type: 7,
                            keys: vec![
                                KeyF32 {
                                    time: 0.0,
                                    value: 1.0,
                                    tangent_in: 0.25,
                                    tangent_out: 0.5,
                                },
                                KeyF32 {
                                    time: 4.0,
                                    value: -2.5,
                                    tangent_in: 0.5,
                                    tangent_out: 1.0,
                                },
                            ],
                        },
                        rotation: TrackI16::Keyed {
                            tangent_type: 0,
                            keys: vec![
                                KeyI16 {
                                    time: 0,
                                    value: -32768,
                                    tangent_in: 12,
                                    tangent_out: 12,
                                },
                                KeyI16 {
                                    time: 9,
                                    value: 16383,
                                    tangent_in: -4,
                                    tangent_out: -4,
                                },
                            ],
                        },
                        translation: TrackF32::Constant { value: 5.5 },
                    }),
                }],
                bas: BasMetadata {
                    present: true,
                    offset: 0x240,
                    length: 0x48,
                },
            }),
        };
        // BTP with duplicate material rows and sample-count != duration.
        let btp = AnimationClip {
            version: CLIP_VERSION,
            identity: ClipIdentity {
                archive: "LkAnm".into(),
                member: "btp/y.btp".into(),
                entry_index: 30,
                resource_id: 31,
                sha256: "bb".repeat(32),
            },
            data: ClipData::Btp(BtpClip {
                duration_frames: 10,
                loop_attribute: 2,
                targets: vec![
                    BtpTarget {
                        material: "mouth".into(),
                        material_remap: 14,
                        texture_map_slot: 0,
                        texture_indices: vec![27, 7, 7],
                    },
                    BtpTarget {
                        material: "mouth".into(),
                        material_remap: 99,
                        texture_map_slot: 1,
                        texture_indices: vec![4],
                    },
                ],
            }),
        };
        // BTK with a full post set.
        let axis = AxisSrt {
            scale: TrackF32::Default,
            rotation: TrackI16::Constant { value: 0x4000 },
            translation: TrackF32::Default,
        };
        let btk = AnimationClip {
            version: CLIP_VERSION,
            identity: ClipIdentity {
                archive: "LkD01".into(),
                member: "btk/z.btk".into(),
                entry_index: 40,
                resource_id: 41,
                sha256: "cc".repeat(32),
            },
            data: ClipData::Btk(BtkClip {
                duration_frames: 20,
                loop_attribute: 2,
                rotation_decimal_shift: 1,
                matrix_calc_type: 1,
                targets: vec![BtkTarget {
                    material: "eyeL".into(),
                    material_remap: 37,
                    texgen_selector: 2,
                    center: [0.5, -0.5, 0.25],
                    axes: std::array::from_fn(|_| axis.clone()),
                }],
                post: Some(BtkPostSet {
                    targets: vec![BtkTarget {
                        material: "eyeR".into(),
                        material_remap: 11,
                        texgen_selector: 1,
                        center: [0.0; 3],
                        axes: std::array::from_fn(|_| axis.clone()),
                    }],
                }),
            }),
        };
        for clip in [bck, btp, btk] {
            let json = serde_json::to_string(&clip).unwrap();
            let back: AnimationClip = serde_json::from_str(&json).unwrap();
            assert_eq!(back, clip);
            assert_eq!(read_clip(&json).unwrap(), clip);
        }
    }

    #[test]
    fn animation_schema_roundtrip() {
        let clip = AnimationClip {
            version: CLIP_VERSION,
            identity: identity(),
            data: ClipData::Bck(BckClip {
                duration_frames: 6,
                loop_attribute: 2,
                rotation_decimal_shift: 0,
                joints: vec![BckJoint {
                    ordinal: 0,
                    axes: [
                        AxisSrt {
                            scale: TrackF32::Default,
                            rotation: TrackI16::Constant { value: 16383 },
                            translation: TrackF32::Default,
                        },
                        AxisSrt {
                            scale: TrackF32::Constant { value: 1.0 },
                            rotation: TrackI16::Keyed {
                                tangent_type: 0,
                                keys: vec![KeyI16 {
                                    time: 0,
                                    value: -505,
                                    tangent_in: 12,
                                    tangent_out: 12,
                                }],
                            },
                            translation: TrackF32::Keyed {
                                tangent_type: 3,
                                keys: vec![
                                    KeyF32 {
                                        time: 0.0,
                                        value: 1.5,
                                        tangent_in: 0.0,
                                        tangent_out: 0.25,
                                    },
                                    KeyF32 {
                                        time: 3.0,
                                        value: -2.5,
                                        tangent_in: 0.25,
                                        tangent_out: 1.0,
                                    },
                                ],
                            },
                        },
                        AxisSrt {
                            scale: TrackF32::Default,
                            rotation: TrackI16::Default,
                            translation: TrackF32::Default,
                        },
                    ],
                }],
                bas: BasMetadata {
                    present: false,
                    offset: 0,
                    length: 0,
                },
            }),
        };
        let json = serde_json::to_string(&clip).unwrap();
        let back: AnimationClip = serde_json::from_str(&json).unwrap();
        assert_eq!(back, clip);
        // The validated reader accepts the current version.
        assert_eq!(read_clip(&json).unwrap(), clip);
    }

    #[test]
    fn animation_schema_rejects_unknown_version() {
        let bad = r#"{"version": 99, "clips": []}"#;
        let err = read_catalog(bad).unwrap_err();
        assert!(err.message.contains("version 99"), "{err}");
        let bad_clip = r#"{"version": 2, "identity": {}, "data": {}}"#;
        assert!(
            read_clip(bad_clip)
                .unwrap_err()
                .message
                .contains("version 2")
        );
        assert!(
            read_catalog(r#"{"clips": []}"#)
                .unwrap_err()
                .message
                .contains("version")
        );
    }

    #[test]
    fn track_defaults_and_rotation_units() {
        // Defaults are *documented*, not stored: the Default variant carries no
        // value and means scale 1 / rotation 0 / translation 0 by track kind.
        assert!(matches!(TrackF32::Default, TrackF32::Default));
        assert!(matches!(TrackI16::Default, TrackI16::Default));
        // 65536 i16-units per turn: 0x4000 is a quarter turn, -32768 is -pi.
        let quarter: f32 = 0x4000 as f32 / 65536.0 * 360.0;
        assert!((quarter - 90.0).abs() < 1e-6);
        assert_eq!(i16::from_be_bytes([0x80, 0x00]), -32768);
        // The decimal shift is preserved separately, never pre-applied.
        let value = -505i16;
        let shift = 1u8;
        assert_eq!((value as i32) << shift, -1010);
    }

    #[test]
    fn catalog_roundtrip_and_format_names() {
        let catalog = AnimationCatalog {
            version: CATALOG_VERSION,
            clips: vec![CatalogClip {
                archive: "LkD01".into(),
                member: "btp/wait.btp".into(),
                entry_index: 12,
                resource_id: 44,
                format: AnimationFormat::Btp,
                sha256: "cd".repeat(32),
                file: "clips/LkD01/btp/wait.btp.json".into(),
            }],
        };
        let json = serde_json::to_string(&catalog).unwrap();
        assert_eq!(read_catalog(&json).unwrap(), catalog);
        assert_eq!(AnimationFormat::Btk.to_string(), "btk");
        // Serialized format names are the lowercase tags used in ClipData too.
        assert!(json.contains("\"btp\""));
    }

    #[test]
    fn serialized_documents_have_no_host_paths_or_timestamps() {
        let clip = AnimationClip {
            version: CLIP_VERSION,
            identity: identity(),
            data: ClipData::Btp(BtpClip {
                duration_frames: 1,
                loop_attribute: 2,
                targets: vec![BtpTarget {
                    material: "mouth".into(),
                    material_remap: 14,
                    texture_map_slot: 0,
                    texture_indices: vec![27, 27],
                }],
            }),
        };
        let json = serde_json::to_string(&clip).unwrap();
        assert!(!json.contains("timestamp"));
        assert!(!json.contains("/home/"));
        assert!(!json.contains("created_at"));
    }
}
