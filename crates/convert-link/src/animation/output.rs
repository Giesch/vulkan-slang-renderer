//! Deterministic emission of converted animation documents and the
//! `--dump-canonical` semantic dump.
//!
//! # Canonical dump format
//!
//! The canonical dump is a complete, lossless, *normalized* text rendering of
//! every clip, byte-identical to the independent Python oracle's dump of the
//! same source bytes. Rules both sides implement:
//!
//! - clips sort by (archive, member); targets keep source order.
//! - integers print in decimal; **f32 values print as their IEEE-754 bit
//!   pattern in `f32:0x%08X` form** so Python/Rust decimal formatting cannot
//!   diverge; comparisons are exact, never epsilon.
//! - tracks: `default`, `constant=<val>` or `keyed[ty=<u16>]` followed by
//!   `;`-joined keys `(time,value,in,out)`.
//! - one line per structural unit, two-space indents, no trailing whitespace,
//!   LF endings, trailing newline.
//!
//! The same renderer is used for real clips and synthetic fixtures, so the
//! oracle differential tests exercise every branch of it.

use std::fmt::Write as _;

use anyhow::{Context, Result};
use gx::animation_manifest::{
    AnimationCatalog, AnimationClip, AxisSrt, BasMetadata, CATALOG_VERSION, CatalogClip, ClipData,
    TrackF32, TrackI16,
};

/// The catalog document file name inside the output tree.
pub const CATALOG_FILE: &str = "catalog.json";

/// Render one clip's canonical dump (no trailing newline handling here; the
/// driver concatenates whole lines).
pub fn canonical_clip(clip: &AnimationClip) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "clip {}/{} {} sha256={}",
        clip.identity.archive,
        clip.identity.member,
        clip.format(),
        clip.identity.sha256
    );
    match &clip.data {
        ClipData::Bck(b) => {
            let _ = writeln!(
                out,
                "  duration={} loop={} rotation_shift={} bas={}",
                b.duration_frames,
                b.loop_attribute,
                b.rotation_decimal_shift,
                bas_str(&b.bas)
            );
            for j in &b.joints {
                let _ = writeln!(out, "  joint {}", j.ordinal);
                for (name, axis) in ["x", "y", "z"].iter().zip(&j.axes) {
                    let _ = writeln!(out, "    axis {name} {}", axis_str(axis));
                }
            }
        }
        ClipData::Btp(b) => {
            let _ = writeln!(
                out,
                "  duration={} loop={}",
                b.duration_frames, b.loop_attribute
            );
            for (i, t) in b.targets.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "  target {} material={} remap={} texno={} samples={}:{}",
                    i,
                    t.material,
                    t.material_remap,
                    t.texture_map_slot,
                    t.texture_indices.len(),
                    t.texture_indices
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
        }
        ClipData::Btk(b) => {
            let _ = writeln!(
                out,
                "  duration={} loop={} rotation_shift={} matrix_calc={}",
                b.duration_frames, b.loop_attribute, b.rotation_decimal_shift, b.matrix_calc_type
            );
            write_btk_targets(&mut out, &b.targets);
            match &b.post {
                Some(post) => {
                    let _ = writeln!(out, "  post targets={}", post.targets.len());
                    write_btk_targets(&mut out, &post.targets);
                }
                None => {
                    let _ = writeln!(out, "  post targets=0");
                }
            }
        }
    }
    out
}

fn write_btk_targets(out: &mut String, targets: &[gx::animation_manifest::BtkTarget]) {
    for (i, t) in targets.iter().enumerate() {
        let _ = writeln!(
            out,
            "  target {} material={} remap={} texgen={} center={}",
            i,
            t.material,
            t.material_remap,
            t.texgen_selector,
            t.center
                .iter()
                .map(|v| f32hex(*v))
                .collect::<Vec<_>>()
                .join(",")
        );
        for (name, axis) in ["s", "t", "q"].iter().zip(&t.axes) {
            let _ = writeln!(out, "    axis {name} {}", axis_str(axis));
        }
    }
}

fn bas_str(bas: &BasMetadata) -> String {
    if bas.present {
        format!("present off={} len={}", bas.offset, bas.length)
    } else {
        "absent".to_string()
    }
}

fn axis_str(axis: &AxisSrt) -> String {
    format!(
        "scale={} rotation={} translation={}",
        track_f32_str(&axis.scale),
        track_i16_str(&axis.rotation),
        track_f32_str(&axis.translation)
    )
}

fn track_f32_str(track: &TrackF32) -> String {
    match track {
        TrackF32::Default => "default".into(),
        TrackF32::Constant { value } => format!("constant={}", f32hex(*value)),
        TrackF32::Keyed { tangent_type, keys } => format!(
            "keyed[ty={tangent_type}] {}",
            keys.iter()
                .map(|k| format!(
                    "({},{},{},{})",
                    f32hex(k.time),
                    f32hex(k.value),
                    f32hex(k.tangent_in),
                    f32hex(k.tangent_out)
                ))
                .collect::<Vec<_>>()
                .join(";")
        ),
    }
}

fn track_i16_str(track: &TrackI16) -> String {
    match track {
        TrackI16::Default => "default".into(),
        TrackI16::Constant { value } => format!("constant={value}"),
        TrackI16::Keyed { tangent_type, keys } => format!(
            "keyed[ty={tangent_type}] {}",
            keys.iter()
                .map(|k| format!(
                    "({},{},{},{})",
                    k.time, k.value, k.tangent_in, k.tangent_out
                ))
                .collect::<Vec<_>>()
                .join(";")
        ),
    }
}

/// Exact f32 rendering shared with the oracle: the IEEE-754 bit pattern.
pub fn f32hex(v: f32) -> String {
    format!("f32:0x{:08X}", v.to_bits())
}

/// Build the catalog document for a set of parsed clips (sorted by
/// archive/member, stable output file names).
pub fn build_catalog(clips: &[AnimationClip]) -> AnimationCatalog {
    let mut clips: Vec<_> = clips.to_vec();
    clips.sort_by(|a, b| {
        (&a.identity.archive, &a.identity.member).cmp(&(&b.identity.archive, &b.identity.member))
    });
    AnimationCatalog {
        version: CATALOG_VERSION,
        clips: clips
            .iter()
            .map(|c| CatalogClip {
                archive: c.identity.archive.clone(),
                member: c.identity.member.clone(),
                entry_index: c.identity.entry_index,
                resource_id: c.identity.resource_id,
                format: c.format(),
                sha256: c.identity.sha256.clone(),
                file: clip_output_file(&c.identity.archive, &c.identity.member),
            })
            .collect(),
    }
}

/// Output path of one clip document, e.g. `clips/LkAnm/bcks/x.bck.json`.
pub fn clip_output_file(archive: &str, member: &str) -> String {
    format!("clips/{archive}/{member}.json")
}

/// Serialize a document deterministically: pretty JSON, fixed field order,
/// one trailing newline.
pub fn json_bytes<T: serde::Serialize>(doc: &T) -> Result<Vec<u8>> {
    let mut text = serde_json::to_string_pretty(doc).context("serializing animation document")?;
    text.push('\n');
    Ok(text.into_bytes())
}

/// Render a whole raw tree's canonical dump (sorted clips).
pub fn canonical_dump(clips: &[AnimationClip]) -> String {
    let mut clips: Vec<_> = clips.to_vec();
    clips.sort_by(|a, b| {
        (&a.identity.archive, &a.identity.member).cmp(&(&b.identity.archive, &b.identity.member))
    });
    let mut out = String::new();
    for clip in &clips {
        out.push_str(&canonical_clip(clip));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gx::animation_manifest::{
        BckClip, BckJoint, BtpClip, BtpTarget as MbtpTarget, CLIP_VERSION, ClipIdentity, KeyF32,
        KeyI16,
    };

    fn ident(member: &str) -> ClipIdentity {
        ClipIdentity {
            archive: "LkAnm".into(),
            member: member.into(),
            entry_index: 3,
            resource_id: 4,
            sha256: "aa".repeat(32),
        }
    }

    fn one_axis() -> AxisSrt {
        AxisSrt {
            scale: TrackF32::Default,
            rotation: TrackI16::Keyed {
                tangent_type: 0,
                keys: vec![KeyI16 {
                    time: 0,
                    value: -1,
                    tangent_in: 2,
                    tangent_out: 2,
                }],
            },
            translation: TrackF32::Constant { value: 1.0 },
        }
    }

    fn btk_axis() -> AxisSrt {
        AxisSrt {
            scale: TrackF32::Keyed {
                tangent_type: 1,
                keys: vec![KeyF32 {
                    time: 0.0,
                    value: 1.0,
                    tangent_in: 0.0,
                    tangent_out: 0.5,
                }],
            },
            rotation: TrackI16::Default,
            translation: TrackF32::Default,
        }
    }

    #[test]
    fn canonical_dump_is_stable_and_lossless() {
        let clip = AnimationClip {
            version: CLIP_VERSION,
            identity: ident("bcks/x.bck"),
            data: ClipData::Bck(BckClip {
                duration_frames: 6,
                loop_attribute: 2,
                rotation_decimal_shift: 0,
                joints: vec![BckJoint {
                    ordinal: 0,
                    axes: std::array::from_fn(|_| one_axis()),
                }],
                bas: BasMetadata {
                    present: false,
                    offset: 0,
                    length: 0,
                },
            }),
        };
        let dump = canonical_clip(&clip);
        assert!(dump.contains("clip LkAnm/bcks/x.bck bck sha256="), "{dump}");
        assert!(dump.contains("bas=absent"), "{dump}");
        assert!(
            dump.contains("axis x scale=default rotation=keyed[ty=0] (0,-1,2,2)"),
            "{dump}"
        );
        assert!(
            dump.contains(&format!("translation=constant={}", f32hex(1.0))),
            "{dump}"
        );
        // f32 values render as bit patterns, not decimals
        assert!(f32hex(1.0).contains("3F800000"));
        assert_eq!(f32hex(-2.0), "f32:0xC0000000");
    }

    #[test]
    fn canonical_dump_btp_samples_and_btk_axes() {
        let btp = AnimationClip {
            version: CLIP_VERSION,
            identity: ident("btp/y.btp"),
            data: ClipData::Btp(BtpClip {
                duration_frames: 1,
                loop_attribute: 2,
                targets: vec![MbtpTarget {
                    material: "mouth".into(),
                    material_remap: 14,
                    texture_map_slot: 0,
                    texture_indices: vec![27, 7, 7],
                }],
            }),
        };
        let dump = canonical_clip(&btp);
        assert!(
            dump.contains("target 0 material=mouth remap=14 texno=0 samples=3:27,7,7"),
            "{dump}"
        );

        let btk = AnimationClip {
            version: CLIP_VERSION,
            identity: ident("btk/z.btk"),
            data: ClipData::Btk(gx::animation_manifest::BtkClip {
                duration_frames: 20,
                loop_attribute: 2,
                rotation_decimal_shift: 1,
                matrix_calc_type: 0,
                targets: vec![gx::animation_manifest::BtkTarget {
                    material: "eyeL".into(),
                    material_remap: 7,
                    texgen_selector: 2,
                    center: [0.5, -0.5, 0.25],
                    axes: std::array::from_fn(|_| btk_axis()),
                }],
                post: None,
            }),
        };
        let dump = canonical_clip(&btk);
        assert!(dump.contains("matrix_calc=0"), "{dump}");
        assert!(
            dump.contains(&format!(
                "center={},{},{}",
                f32hex(0.5),
                f32hex(-0.5),
                f32hex(0.25)
            )),
            "{dump}"
        );
        assert!(dump.contains("axis s scale=keyed[ty=1]"), "{dump}");
        assert!(dump.contains("post targets=0"), "{dump}");
    }

    #[test]
    fn catalog_and_json_determinism() {
        let clip = AnimationClip {
            version: CLIP_VERSION,
            identity: ident("bcks/x.bck"),
            data: ClipData::Bck(BckClip {
                duration_frames: 1,
                loop_attribute: 0,
                rotation_decimal_shift: 0,
                joints: vec![],
                bas: BasMetadata {
                    present: false,
                    offset: 0,
                    length: 0,
                },
            }),
        };
        let catalog = build_catalog(&[clip]);
        assert_eq!(catalog.version, CATALOG_VERSION);
        assert_eq!(catalog.clips[0].file, "clips/LkAnm/bcks/x.bck.json");
        let a = json_bytes(&catalog).unwrap();
        let b = json_bytes(&build_catalog(&[AnimationClip {
            version: CLIP_VERSION,
            identity: ident("bcks/x.bck"),
            data: ClipData::Bck(BckClip {
                duration_frames: 1,
                loop_attribute: 0,
                rotation_decimal_shift: 0,
                joints: vec![],
                bas: BasMetadata {
                    present: false,
                    offset: 0,
                    length: 0,
                },
            }),
        }]))
        .unwrap();
        assert_eq!(a, b);
        assert_eq!(a.last(), Some(&b'\n'));
    }
}
