//! Runtime BCK compatibility boundary, separate from preservation/schema readers.
//!
//! These checks do not evaluate poses. Every sampled pose must additionally be
//! checked for finite results before publication. Animation-only failures must
//! leave the existing static model path available.

use std::collections::HashSet;
use std::path::{Component, Path};

use anyhow::{Context, Result, ensure};
use gx::animation_manifest::{
    AnimationCatalog, AnimationClip, AnimationFormat, BckClip, CATALOG_VERSION, CLIP_VERSION,
    CatalogClip, ClipData, TrackF32, TrackI16,
};
use gx::model_manifest::{Manifest, ScalingRule, Skeleton};

/// Absolute tolerance on the preserved four-weight sum. We never renormalize.
pub const SKIN_WEIGHT_SUM_TOLERANCE: f64 = 1.0e-4;
pub const PACKED_SKIN_VERTEX_BYTES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkinInfluence {
    pub joint: u8,
    pub weight: f32,
}

pub type VertexSkin = [SkinInfluence; 4];

pub fn catalog_label(entry: &CatalogClip) -> String {
    format!(
        "{}/{} [entry={}, resource={}, sha256={}]",
        entry.archive, entry.member, entry.entry_index, entry.resource_id, entry.sha256
    )
}

/// Require catalog-relative paths; do not permit traversal or host-absolute paths.
pub fn validate_relative_path(file: &str) -> Result<()> {
    ensure!(
        !file.is_empty() && !file.contains('\\') && !file.contains(':'),
        "invalid relative path {file:?}"
    );
    ensure!(
        Path::new(file)
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "invalid relative path {file:?}"
    );

    Ok(())
}

/// Validates metadata only. Does not open any clip documents.
pub fn validate_catalog(catalog: &AnimationCatalog) -> Result<()> {
    ensure!(
        catalog.version == CATALOG_VERSION,
        "catalog.version: unsupported {}",
        catalog.version
    );
    let mut identities = HashSet::new();
    let mut files = HashSet::new();
    for entry in &catalog.clips {
        let label = catalog_label(entry);
        ensure!(
            !entry.archive.is_empty() && !entry.member.is_empty(),
            "{label}: empty identity"
        );
        ensure!(
            entry.sha256.len() == 64 && entry.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "{label}: invalid sha256"
        );
        validate_relative_path(&entry.file).with_context(|| label.clone())?;
        let identity = (
            &entry.archive,
            &entry.member,
            entry.entry_index,
            entry.resource_id,
            &entry.sha256,
        );
        ensure!(
            identities.insert(identity),
            "{label}: duplicate catalog identity"
        );
        ensure!(
            files.insert(&entry.file),
            "{label}: duplicate catalog file {}",
            entry.file
        );
    }

    Ok(())
}

pub fn validate_skeleton(skeleton: &Skeleton) -> Result<()> {
    ensure!(
        skeleton.scaling_rule == ScalingRule::Maya,
        "skeleton.scaling_rule: Maya scaling required, got {:?}; Basic/Softimage playback is unsupported",
        skeleton.scaling_rule
    );
    ensure!(!skeleton.joints.is_empty(), "skeleton.joints: empty");
    ensure!(
        skeleton.joints.len() <= 256,
        "skeleton.joints: exceeds u8 skin index capacity"
    );
    for (index, joint) in skeleton.joints.iter().enumerate() {
        ensure!(
            joint.parent == -1 || (joint.parent >= 0 && (joint.parent as usize) < index),
            "skeleton.joints[{index}].parent: {} must be root or precede child",
            joint.parent
        );
        ensure!(
            joint.t.iter().all(|value| value.is_finite()),
            "skeleton.joints[{index}].t: nonfinite"
        );
        ensure!(
            joint.s == [1.0; 3],
            "skeleton.joints[{index}].s: only exact unit scale supported"
        );
    }

    Ok(())
}

/// Decodes the asset's interleaved u8/f32-LE slots, not a shader struct.
/// All indices (including padding) must be safe; positive weights are live.
pub fn validate_model_skin(model: &Manifest, bytes: &[u8]) -> Result<Vec<VertexSkin>> {
    ensure!(
        model.version == 1,
        "model.version: unsupported {}",
        model.version
    );
    validate_skeleton(&model.skeleton)?;
    let expected = usize::try_from(model.buffers.vertex_count)?
        .checked_mul(PACKED_SKIN_VERTEX_BYTES)
        .context("skin byte count overflow")?;
    ensure!(
        bytes.len() == expected,
        "skin.length: expected {expected}, got {}",
        bytes.len()
    );
    let mut vertices = Vec::with_capacity(model.buffers.vertex_count as usize);
    for (vertex, record) in bytes
        .as_chunks::<PACKED_SKIN_VERTEX_BYTES>()
        .0
        .iter()
        .enumerate()
    {
        let mut influences = [SkinInfluence {
            joint: 0,
            weight: 0.0,
        }; 4];
        let mut sum = 0.0_f64;
        for (slot, packed) in record.as_chunks::<5>().0.iter().enumerate() {
            let joint = packed[0];
            let weight = f32::from_le_bytes(packed[1..5].try_into().expect("five-byte slot"));
            ensure!(
                (joint as usize) < model.skeleton.joints.len(),
                "skin[{vertex}][{slot}].joint: {joint} out of range"
            );
            ensure!(
                weight.is_finite() && weight >= 0.0,
                "skin[{vertex}][{slot}].weight: nonfinite or negative {weight}"
            );
            sum += f64::from(weight);
            influences[slot] = SkinInfluence { joint, weight };
        }
        ensure!(
            (sum - 1.0).abs() <= SKIN_WEIGHT_SUM_TOLERANCE,
            "skin[{vertex}].weight_sum: {sum}, expected 1 +/- {SKIN_WEIGHT_SUM_TOLERANCE}"
        );
        vertices.push(influences);
    }

    Ok(vertices)
}

pub fn validate_clip<'a>(
    entry: &CatalogClip,
    document: &'a AnimationClip,
    skeleton: &Skeleton,
) -> Result<&'a BckClip> {
    let label = catalog_label(entry);
    let validate = || -> Result<&'a BckClip> {
        ensure!(
            document.version == CLIP_VERSION,
            "clip.version: unsupported {}",
            document.version
        );
        let identity = &document.identity;
        ensure!(
            identity.archive == entry.archive
                && identity.member == entry.member
                && identity.entry_index == entry.entry_index
                && identity.resource_id == entry.resource_id
                && identity.sha256 == entry.sha256,
            "clip.identity: differs from catalog"
        );
        ensure!(
            entry.format == AnimationFormat::Bck && document.format() == entry.format,
            "clip.format: expected catalog BCK"
        );
        let ClipData::Bck(clip) = &document.data else {
            unreachable!("format checked")
        };
        validate_bck(clip, skeleton)?;

        Ok(clip)
    };

    validate().with_context(|| label)
}

pub fn validate_bck(clip: &BckClip, skeleton: &Skeleton) -> Result<()> {
    validate_skeleton(skeleton)?;
    ensure!(
        matches!(clip.loop_attribute, 0 | 2),
        "loop_attribute: unsupported {}",
        clip.loop_attribute
    );
    ensure!(
        clip.rotation_decimal_shift <= 3,
        "rotation_decimal_shift: unsupported {}",
        clip.rotation_decimal_shift
    );
    ensure!(
        clip.joints.len() == skeleton.joints.len(),
        "joints: expected {}, got {}",
        skeleton.joints.len(),
        clip.joints.len()
    );
    let mut seen = vec![false; skeleton.joints.len()];
    for joint in &clip.joints {
        let ordinal = usize::from(joint.ordinal);
        ensure!(
            ordinal < seen.len(),
            "joints.ordinal: {ordinal} out of range"
        );
        ensure!(!seen[ordinal], "joints.ordinal: duplicate {ordinal}");
        seen[ordinal] = true;
        for (axis_index, axis) in joint.axes.iter().enumerate() {
            let field = format!("joints[{ordinal}].axes[{axis_index}]");
            // Scale defaults to 1 in the sampler; all explicit values and keys
            // obey the same finite/structural contract as translation.
            validate_translation(&axis.scale).with_context(|| format!("{field}.scale"))?;
            validate_translation(&axis.translation)
                .with_context(|| format!("{field}.translation"))?;
            validate_rotation(&axis.rotation).with_context(|| format!("{field}.rotation"))?;
        }
    }

    Ok(())
}

fn validate_translation(track: &TrackF32) -> Result<()> {
    match track {
        TrackF32::Default => {}
        TrackF32::Constant { value } => ensure!(value.is_finite(), "constant.value: nonfinite"),
        TrackF32::Keyed { tangent_type, keys } => {
            ensure!(
                keys.len() >= 2,
                "keys: keyed track requires at least two keys"
            );
            for (index, key) in keys.iter().enumerate() {
                ensure!(
                    [key.time, key.value, key.tangent_in, key.tangent_out]
                        .iter()
                        .all(|value| value.is_finite()),
                    "keys[{index}]: nonfinite field"
                );
                ensure!(
                    index == 0 || keys[index - 1].time < key.time,
                    "keys[{index}].time: must strictly increase"
                );
                ensure!(
                    *tangent_type != 0 || key.tangent_in == key.tangent_out,
                    "keys[{index}]: shared tangents differ"
                );
            }
        }
    }

    Ok(())
}

fn validate_rotation(track: &TrackI16) -> Result<()> {
    if let TrackI16::Keyed { tangent_type, keys } = track {
        ensure!(
            keys.len() >= 2,
            "keys: keyed track requires at least two keys"
        );
        for (index, key) in keys.iter().enumerate() {
            ensure!(
                index == 0 || keys[index - 1].time < key.time,
                "keys[{index}].time: must strictly increase"
            );
            ensure!(
                *tangent_type != 0 || key.tangent_in == key.tangent_out,
                "keys[{index}]: shared tangents differ"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gx::animation_manifest::{
        self as am, AxisSrt, BasMetadata, BckJoint, ClipIdentity, KeyF32, KeyI16,
    };
    use gx::model_manifest::{Buffers, SkeletonJoint};

    fn model() -> Manifest {
        Manifest {
            version: 1,
            buffers: Buffers {
                vertices: "v.bin".into(),
                indices: "i.bin".into(),
                skinning: "s.bin".into(),
                vertex_layout: vec![],
                vertex_count: 1,
                index_count: 0,
            },
            textures: vec![],
            materials: vec![],
            batches: vec![],
            skeleton: Skeleton {
                scaling_rule: ScalingRule::Maya,
                joints: vec![SkeletonJoint {
                    scale_compensate: false,
                    name: "root".into(),
                    parent: -1,
                    t: [0.0; 3],
                    r_s16: [0; 3],
                    s: [1.0; 3],
                }],
            },
        }
    }

    fn clip() -> BckClip {
        BckClip {
            duration_frames: 0,
            loop_attribute: 2,
            rotation_decimal_shift: 3,
            joints: vec![BckJoint {
                ordinal: 0,
                axes: std::array::from_fn(|_| AxisSrt {
                    scale: TrackF32::Default,
                    rotation: TrackI16::Default,
                    translation: TrackF32::Default,
                }),
            }],
            bas: BasMetadata {
                present: false,
                offset: 0,
                length: 0,
            },
        }
    }

    fn entry() -> CatalogClip {
        CatalogClip {
            archive: "LkAnm".into(),
            member: "bcks/test.bck".into(),
            entry_index: 1,
            resource_id: 2,
            format: AnimationFormat::Bck,
            sha256: "a".repeat(64),
            file: "clips/test.json".into(),
        }
    }

    fn packed(weights: [f32; 4]) -> Vec<u8> {
        weights
            .into_iter()
            .flat_map(|weight| std::iter::once(0).chain(weight.to_le_bytes()))
            .collect()
    }

    #[test]
    fn bck_validation_static_defaults_and_metadata() {
        let skeleton = model().skeleton;
        let mut candidate = clip();
        validate_bck(&candidate, &skeleton).unwrap();
        for duration in [0, 1, 400] {
            candidate.duration_frames = duration;
            for loop_attribute in [0, 2] {
                candidate.loop_attribute = loop_attribute;
                for shift in 0..=3 {
                    candidate.rotation_decimal_shift = shift;
                    validate_bck(&candidate, &skeleton).unwrap();
                }
            }
        }
        for shift in [4, 255] {
            candidate.rotation_decimal_shift = shift;
            assert!(validate_bck(&candidate, &skeleton).is_err());
        }
        candidate.rotation_decimal_shift = 0;
        for loop_attribute in [1, 3, 4, 255] {
            candidate.loop_attribute = loop_attribute;
            assert!(validate_bck(&candidate, &skeleton).is_err());
        }
    }

    #[test]
    fn bck_validation_scale_and_joint_coverage() {
        let skeleton = model().skeleton;
        for scale in [
            TrackF32::Constant {
                value: f32::INFINITY,
            },
            TrackF32::Constant { value: f32::NAN },
            TrackF32::Keyed {
                tangent_type: 0,
                keys: vec![],
            },
        ] {
            let mut candidate = clip();
            candidate.joints[0].axes[1].scale = scale;
            assert!(validate_bck(&candidate, &skeleton).is_err());
        }
        let mut candidate = clip();
        candidate.joints[0].axes[0].scale = TrackF32::Constant { value: 1.0 };
        validate_bck(&candidate, &skeleton).unwrap();
        candidate.joints[0].ordinal = 1;
        assert!(validate_bck(&candidate, &skeleton).is_err());
        candidate.joints.clear();
        assert!(validate_bck(&candidate, &skeleton).is_err());
        let mut two = skeleton.clone();
        two.joints.push(two.joints[0].clone());
        candidate.joints = vec![clip().joints[0].clone(); 2];
        assert!(validate_bck(&candidate, &two).is_err());
    }

    #[test]
    fn bck_validation_requires_maya_scaling() {
        for rule in [ScalingRule::Basic, ScalingRule::Softimage] {
            let mut skeleton = model().skeleton;
            skeleton.scaling_rule = rule;
            let error = validate_skeleton(&skeleton).unwrap_err().to_string();
            assert!(error.contains("scaling_rule") && error.contains("unsupported"));
        }
        let mut skeleton = model().skeleton;
        for flag in [false, true] {
            skeleton.joints[0].scale_compensate = flag;
            validate_skeleton(&skeleton).unwrap();
        }
    }

    #[test]
    fn bck_validation_accepts_animated_scale() {
        let key = KeyF32 {
            time: 0.0,
            value: 0.9,
            tangent_in: 0.0,
            tangent_out: 0.25,
        };
        for scale in [
            TrackF32::Default,
            TrackF32::Constant { value: 0.99999 },
            TrackF32::Constant { value: 0.0 },
            TrackF32::Constant { value: -2.0 },
            TrackF32::Keyed {
                tangent_type: 7,
                keys: vec![
                    key,
                    KeyF32 {
                        time: 10.0,
                        value: 1.2,
                        ..key
                    },
                ],
            },
        ] {
            let mut candidate = clip();
            candidate.joints[0].axes[0].scale = scale;
            validate_bck(&candidate, &model().skeleton).unwrap();
        }
    }

    #[test]
    fn bck_validation_track_structure_and_finiteness() {
        let key = KeyF32 {
            time: 0.0,
            value: 1.0,
            tangent_in: 2.0,
            tangent_out: 3.0,
        };
        let last = KeyF32 { time: 10.0, ..key };
        validate_translation(&TrackF32::Keyed {
            tangent_type: 7,
            keys: vec![key, last],
        })
        .unwrap();
        for keys in [
            vec![],
            vec![key],
            vec![key, key],
            vec![last, key],
            vec![
                key,
                KeyF32 {
                    value: f32::INFINITY,
                    ..last
                },
            ],
            vec![
                key,
                KeyF32 {
                    time: f32::NAN,
                    ..last
                },
            ],
            vec![
                key,
                KeyF32 {
                    tangent_out: f32::NAN,
                    ..last
                },
            ],
        ] {
            assert!(
                validate_translation(&TrackF32::Keyed {
                    tangent_type: 7,
                    keys
                })
                .is_err()
            );
        }
        assert!(validate_translation(&TrackF32::Constant { value: f32::NAN }).is_err());
        assert!(
            validate_translation(&TrackF32::Keyed {
                tangent_type: 0,
                keys: vec![key, last]
            })
            .is_err()
        );
        let rotation = KeyI16 {
            time: 0,
            value: 100,
            tangent_in: 1,
            tangent_out: 2,
        };
        validate_rotation(&TrackI16::Keyed {
            tangent_type: 65535,
            keys: vec![
                rotation,
                KeyI16 {
                    time: 10,
                    ..rotation
                },
            ],
        })
        .unwrap();
        for keys in [vec![], vec![rotation], vec![rotation, rotation]] {
            assert!(
                validate_rotation(&TrackI16::Keyed {
                    tangent_type: 2,
                    keys
                })
                .is_err()
            );
        }
        assert!(
            validate_rotation(&TrackI16::Keyed {
                tangent_type: 0,
                keys: vec![
                    rotation,
                    KeyI16 {
                        time: 1,
                        ..rotation
                    }
                ]
            })
            .is_err()
        );
    }

    #[test]
    fn bck_validation_model_skin() {
        let valid = model();
        for weights in [
            [1.0, 0.0, 0.0, 0.0],
            [0.1, 0.2, 0.3, 0.4],
            [1.00005, 0.0, 0.0, 0.0],
        ] {
            assert_eq!(
                validate_model_skin(&valid, &packed(weights)).unwrap().len(),
                1
            );
        }
        for weights in [
            [0.0; 4],
            [1.001, 0.0, 0.0, 0.0],
            [-0.1, 1.1, 0.0, 0.0],
            [f32::NAN, 0.0, 0.0, 0.0],
            [f32::INFINITY, 0.0, 0.0, 0.0],
        ] {
            assert!(validate_model_skin(&valid, &packed(weights)).is_err());
        }
        assert!(validate_model_skin(&valid, &[0; 19]).is_err());
        assert!(validate_model_skin(&valid, &[0; 21]).is_err());
        let mut bad_index = packed([1.0, 0.0, 0.0, 0.0]);
        bad_index[5] = 1;
        assert!(validate_model_skin(&valid, &bad_index).is_err());
        for parent in [-2, 0, 1] {
            let mut invalid = model();
            invalid.skeleton.joints[0].parent = parent;
            assert!(validate_skeleton(&invalid.skeleton).is_err());
        }
        let mut invalid = model();
        invalid.skeleton.joints[0].t[0] = f32::INFINITY;
        assert!(validate_skeleton(&invalid.skeleton).is_err());
        invalid = model();
        invalid.skeleton.joints[0].s[0] = 1.001;
        assert!(validate_skeleton(&invalid.skeleton).is_err());
        invalid = model();
        invalid.version = 2;
        assert!(validate_model_skin(&invalid, &packed([1.0, 0.0, 0.0, 0.0])).is_err());
        assert!(
            validate_skeleton(&Skeleton {
                scaling_rule: ScalingRule::Maya,
                joints: vec![]
            })
            .is_err()
        );
    }

    #[test]
    fn bck_validation_catalog_and_document_identity() {
        let row = entry();
        let mut catalog = AnimationCatalog {
            version: CATALOG_VERSION,
            clips: vec![row.clone()],
        };
        validate_catalog(&catalog).unwrap();
        catalog.clips.push(row.clone());
        assert!(validate_catalog(&catalog).is_err());
        catalog.clips.pop();
        catalog.version += 1;
        assert!(validate_catalog(&catalog).is_err());
        for path in [
            "",
            "../clip.json",
            "/clip.json",
            "clips/../../clip.json",
            "C:\\clip.json",
        ] {
            assert!(validate_relative_path(path).is_err());
        }
        let document = AnimationClip {
            version: CLIP_VERSION,
            identity: ClipIdentity {
                archive: row.archive.clone(),
                member: row.member.clone(),
                entry_index: row.entry_index,
                resource_id: row.resource_id,
                sha256: row.sha256.clone(),
            },
            data: ClipData::Bck(clip()),
        };
        validate_clip(&row, &document, &model().skeleton).unwrap();
        for field in 0..7 {
            let mut bad = document.clone();
            match field {
                0 => bad.version += 1,
                1 => bad.identity.archive.push('x'),
                2 => bad.identity.member.push('x'),
                3 => bad.identity.entry_index += 1,
                4 => bad.identity.resource_id += 1,
                5 => bad.identity.sha256.push('a'),
                _ => {
                    bad.data = ClipData::Btp(am::BtpClip {
                        duration_frames: 1,
                        loop_attribute: 0,
                        targets: vec![],
                    })
                }
            }
            let error = validate_clip(&row, &bad, &model().skeleton).unwrap_err();
            assert!(format!("{error:#}").contains(&catalog_label(&row)));
        }
    }

    /// Opt-in asset gate; missing assets are failures, never skips.
    #[test]
    #[ignore = "requires local converted Link model and animation assets"]
    fn bck_catalog_runtime_audit() -> Result<()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/link");
        let model_dir = root.join("converted");
        let model_path = model_dir.join("link.manifest.json");
        let model: Manifest = serde_json::from_slice(
            &std::fs::read(&model_path).with_context(|| model_path.display().to_string())?,
        )?;
        validate_relative_path(&model.buffers.skinning)?;
        let skin = std::fs::read(model_dir.join(&model.buffers.skinning))?;
        let vertices = validate_model_skin(&model, &skin)?;
        println!(
            "MODEL accepted: {} joints, {} skin vertices, weight-sum tolerance {}",
            model.skeleton.joints.len(),
            vertices.len(),
            SKIN_WEIGHT_SUM_TOLERANCE
        );
        let catalog_dir = root.join("animations/converted");
        let catalog =
            am::read_catalog(&std::fs::read_to_string(catalog_dir.join("catalog.json"))?)?;
        validate_catalog(&catalog)?;
        let mut accepted = 0;
        let mut rejected = Vec::new();
        let mut static_members = Vec::new();
        let mut nonunit_constant_clips = 0;
        let mut varying_keyed_clips = 0;
        let mut overlap_clips = 0;
        for entry in catalog
            .clips
            .iter()
            .filter(|entry| entry.format == AnimationFormat::Bck)
        {
            let label = catalog_label(entry);
            let result = (|| -> Result<u16> {
                let json = std::fs::read_to_string(catalog_dir.join(&entry.file))?;
                let document = am::read_clip(&json)?;
                let clip = validate_clip(entry, &document, &model.skeleton)?;
                let scales = || {
                    clip.joints
                        .iter()
                        .flat_map(|joint| joint.axes.iter())
                        .map(|axis| &axis.scale)
                };
                let nonunit_constant = scales()
                    .any(|track| matches!(track, TrackF32::Constant { value } if *value != 1.0));
                // Equal key values can still vary between keys when tangents
                // are nonzero. Do not mistake those Hermite tracks for constants.
                let varying_keyed = scales().any(|track| match track {
                    TrackF32::Keyed { keys, .. } => keys.iter().any(|key| {
                        key.value != keys[0].value
                            || key.tangent_in != 0.0
                            || key.tangent_out != 0.0
                    }),
                    _ => false,
                });
                nonunit_constant_clips += usize::from(nonunit_constant);
                varying_keyed_clips += usize::from(varying_keyed);
                overlap_clips += usize::from(nonunit_constant && varying_keyed);

                Ok(clip.duration_frames)
            })();
            match result {
                Ok(duration) => {
                    accepted += 1;
                    println!("ACCEPT {label}: duration={duration}");
                    if duration == 0 {
                        static_members.push(format!("{}/{}", entry.archive, entry.member));
                    }
                }
                Err(error) => {
                    println!("REJECT {label}: {error:#}");
                    rejected.push((label, format!("{error:#}")));
                }
            }
        }
        println!(
            "BCK AUDIT total={} accepted={accepted} rejected={}",
            accepted + rejected.len(),
            rejected.len()
        );
        println!(
            "ZERO-DURATION accepted={}: {static_members:#?}",
            static_members.len()
        );
        ensure!(
            rejected.is_empty(),
            "scope gate: incompatible real entries: {rejected:#?}"
        );
        ensure!(
            accepted == 594,
            "inventory changed: expected 594 BCK, got {accepted}"
        );
        println!(
            "SCALE FINGERPRINT nonunit_constant={nonunit_constant_clips} varying_keyed={varying_keyed_clips} overlap={overlap_clips}"
        );
        ensure!(
            (nonunit_constant_clips, varying_keyed_clips, overlap_clips) == (77, 116, 35),
            "scale fingerprint changed: expected (77, 116, 35), got ({nonunit_constant_clips}, {varying_keyed_clips}, {overlap_clips})"
        );
        static_members.sort();
        let expected = ["hookshotjmp", "rise", "ship_jump1", "usefanb2", "vomitjmp"]
            .map(|name| format!("LkAnm/bcks/{name}.bck"));
        ensure!(
            static_members == expected,
            "zero-duration inventory changed: {static_members:?}"
        );

        Ok(())
    }
}
