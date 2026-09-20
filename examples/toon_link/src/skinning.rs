//! Shared GPU skinning inputs for both render modes, plus the CPU reference
//! and the production-shader oracle that check `shaders/source/skinning.slang`.
//!
//! Both modes upload the same two buffers: a per-vertex influence buffer
//! written once at setup, and a per-flight joint palette
//! (`animated_world * inverse_bind_world`, one entry per skeleton joint)
//! written every frame. The shaders index the influence buffer by
//! `SV_VertexID`, so the vertex format is unchanged. The normal policy
//! (conditioning threshold, tolerances, fallback) is documented at the top of
//! `skinning.slang`; [`reference`] restates it on the CPU for the tests.

use std::path::Path;

use anyhow::Context;
use glam::{Mat4, UVec4, Vec4};
use gx::model_manifest::Manifest;
use mltrs::renderer::{
    FrameRenderer, Gpu, ImmutableAddr, ImmutableBufferHandle, Renderer, SingletonBufferHandle,
};

use crate::animation_validation::{VertexSkin, validate_model_skin};
use crate::generated::shader_atlas::skinning::{SkinJoint, VertexSkinning};

/// The GPU buffers behind `params.palette` and `params.skinning`.
pub struct SkinningBuffers {
    /// One [`SkinJoint`] per skeleton joint, rewritten each frame.
    palette: ImmutableBufferHandle<SkinJoint>,
    /// One [`VertexSkinning`] per vertex, written once.
    skinning: SingletonBufferHandle<VertexSkinning>,
    /// Reused per frame so `write_palette` does not allocate.
    staging: Vec<SkinJoint>,
    /// `Some` when the skin data was rejected: the buffers then hold a rigid
    /// rest skin and a one-entry identity palette, and `write_palette` is a
    /// no-op, so the static model still renders (AC5). The host reads it to
    /// disable playback, so the UI never claims to animate a model that
    /// cannot move.
    static_reason: Option<String>,
}

/// Queue-time addresses for one frame. Never retained across frames.
#[derive(Debug, Clone, Copy)]
pub struct SkinningAddrs {
    pub palette: ImmutableAddr<SkinJoint>,
    pub skinning: ImmutableAddr<VertexSkinning>,
}

impl SkinningBuffers {
    /// Validate and upload the model's skin, and allocate the palette with
    /// every flight slot at the identity (bind pose).
    pub fn new(renderer: &mut Renderer, dir: &Path, manifest: &Manifest) -> anyhow::Result<Self> {
        let skins = match load_skin(dir, manifest) {
            Ok(skins) => skins,
            Err(error) => {
                let reason = format!("{error:#}");
                eprintln!("toon_link: animation disabled, rendering the static model: {reason}");
                return Self::with_records(renderer, &rest_records(manifest), 1, Some(reason));
            }
        };

        Self::with_records(
            renderer,
            &skin_records(&skins),
            manifest.skeleton.joints.len(),
            None,
        )
    }

    fn with_records(
        renderer: &mut Renderer,
        records: &[VertexSkinning],
        joint_count: usize,
        static_reason: Option<String>,
    ) -> anyhow::Result<Self> {
        let skinning = renderer.create_singleton_buffer(records)?;
        let identity = vec![
            SkinJoint {
                transform: Mat4::IDENTITY,
            };
            joint_count
        ];
        let mut palette =
            renderer.create_immutable_buffer::<SkinJoint>(u32::try_from(joint_count)?)?;
        renderer.write_immutable_all_frames(&mut palette, &identity);

        Ok(Self {
            palette,
            skinning,
            staging: identity,
            static_reason,
        })
    }

    /// Why the model renders statically, or `None` when it animates.
    pub fn static_reason(&self) -> Option<&str> {
        self.static_reason.as_deref()
    }

    /// The bind pose: one identity per palette entry.
    pub fn identity_palette(&self) -> Vec<Mat4> {
        vec![Mat4::IDENTITY; self.staging.len()]
    }

    /// Upload this frame's palette, one `animated_world * inverse_bind_world`
    /// per skeleton joint in joint order. Call inside `submit_draws`.
    pub fn write_palette(&mut self, gpu: &mut Gpu<'_>, palette: &[Mat4]) {
        let static_model = self.static_reason.is_some();
        if static_model {
            return;
        }

        // Both sides derive from the manifest skeleton; a mismatch is a
        // programming error, and `zip` would otherwise truncate it silently.
        assert_eq!(
            palette.len(),
            self.staging.len(),
            "palette must hold one entry per skeleton joint"
        );
        for (joint, &transform) in self.staging.iter_mut().zip(palette) {
            joint.transform = transform;
        }
        gpu.write_immutable(&mut self.palette, &self.staging);
    }

    pub fn addrs(&self, renderer: &FrameRenderer) -> SkinningAddrs {
        SkinningAddrs {
            palette: renderer.current_immutable_addr(&self.palette),
            skinning: renderer.singleton_addr(&self.skinning),
        }
    }
}

fn load_skin(dir: &Path, manifest: &Manifest) -> anyhow::Result<Vec<VertexSkin>> {
    let path = dir.join(&manifest.buffers.skinning);
    let bytes = std::fs::read(&path).with_context(|| path.display().to_string())?;

    validate_model_skin(manifest, &bytes).with_context(|| path.display().to_string())
}

/// The validated influences as the shader's std430 record.
pub fn skin_records(skins: &[VertexSkin]) -> Vec<VertexSkinning> {
    skins
        .iter()
        .map(|skin| VertexSkinning {
            joints: UVec4::from_array(skin.map(|influence| u32::from(influence.joint))),
            weights: Vec4::from_array(skin.map(|influence| influence.weight)),
        })
        .collect()
}

/// Every vertex rigidly bound to palette entry 0.
fn rest_records(manifest: &Manifest) -> Vec<VertexSkinning> {
    vec![
        VertexSkinning {
            joints: UVec4::ZERO,
            weights: Vec4::X,
        };
        manifest.buffers.vertex_count as usize
    ]
}

/// The CPU statement of the shader's deformation and normal policy. The
/// normal reference is glam's inverse-transpose, not the shader's cofactor
/// form, so agreement inside the well-conditioned domain is independent
/// evidence.
#[cfg(test)]
pub mod reference {
    use glam::{Mat3, Mat4, Vec3};

    use super::VertexSkinning;

    /// Minimum Hadamard ratio `det / (|c0| |c1| |c2|)` for the
    /// inverse-transpose domain. Below it the fallback is authoritative.
    pub const NORMAL_CONDITION_MIN: f32 = 1e-3;
    /// `|v|^2` below this normalizes to exactly zero.
    pub const SAFE_NORMALIZE_MIN_LENGTH_SQ: f32 = 1e-12;

    /// Absolute per-component tolerance on deformed positions.
    pub const POSITION_TOLERANCE: f32 = 1e-4;
    /// Angle between expected and actual unit normals.
    pub const ANGULAR_TOLERANCE_RAD: f32 = 1e-3;
    /// Tolerance on the reported conditioning number.
    pub const CONDITIONING_TOLERANCE: f32 = 1e-4;

    pub fn safe_normalize(vector: Vec3) -> Vec3 {
        let length_sq = vector.dot(vector);
        let near_zero = length_sq < SAFE_NORMALIZE_MIN_LENGTH_SQ;
        if near_zero {
            return Vec3::ZERO;
        }

        vector / length_sq.sqrt()
    }

    /// The weighted sum of the four influences' palette entries.
    pub fn blend(palette: &[Mat4], skin: &VertexSkinning) -> Mat4 {
        let mut blended = Mat4::ZERO;
        for slot in 0..4 {
            blended += palette[skin.joints[slot] as usize] * skin.weights[slot];
        }

        blended
    }

    pub fn linear_part(transform: &Mat4) -> Mat3 {
        Mat3::from_mat4(*transform)
    }

    /// The scale-aware conditioning number in `[-1, 1]`; 0 for a zero column.
    pub fn conditioning(linear: &Mat3) -> f32 {
        let det = linear.x_axis.dot(linear.y_axis.cross(linear.z_axis));
        let scale = linear.x_axis.length() * linear.y_axis.length() * linear.z_axis.length();
        let zero_column = scale.is_nan() || scale <= 0.0;
        if zero_column {
            return 0.0;
        }

        det / scale
    }

    pub fn uses_fallback(linear: &Mat3) -> bool {
        let cond = conditioning(linear);

        cond.is_nan() || cond < NORMAL_CONDITION_MIN
    }

    /// Inverse-transpose inside the domain, `L * n` outside it; both safely
    /// normalized. Translation never enters.
    pub fn expected_normal(linear: &Mat3, normal: Vec3) -> Vec3 {
        if uses_fallback(linear) {
            return safe_normalize(*linear * normal);
        }

        safe_normalize(linear.inverse().transpose() * normal)
    }

    pub fn expected_position(transform: &Mat4, point: Vec3) -> Vec3 {
        transform.transform_point3(point)
    }

    /// Angle in radians between two unit vectors.
    pub fn angle_between(a: Vec3, b: Vec3) -> f32 {
        a.dot(b).clamp(-1.0, 1.0).acos()
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use glam::{Mat3, Mat4, Quat, UVec4, Vec3, Vec4};

    use super::reference::*;
    use super::*;

    fn skin(joints: [u32; 4], weights: [f32; 4]) -> VertexSkinning {
        VertexSkinning {
            joints: UVec4::from_array(joints),
            weights: Vec4::from_array(weights),
        }
    }

    /// Columns `(1,0,0)`, `(cos, sin, 0)`, `(0,0,1)`: a shear whose
    /// conditioning number is exactly `sin`.
    fn skew(sin: f32) -> Mat4 {
        let cos = (1.0 - sin * sin).sqrt();

        Mat4::from_cols(Vec4::X, Vec4::new(cos, sin, 0.0, 0.0), Vec4::Z, Vec4::W)
    }

    fn assert_close(actual: Vec3, expected: Vec3, tolerance: f32, what: &str) {
        let error = (actual - expected).abs().max_element();
        assert!(
            error <= tolerance,
            "{what}: expected {expected:?}, got {actual:?} (max error {error:e} > {tolerance:e})"
        );
    }

    #[test]
    fn bck_normal_conditioning_policy() {
        // Well-conditioned side: sin = 2e-3 is twice the threshold.
        let above = linear_part(&skew(2e-3));
        let cond = conditioning(&above);
        assert!((cond - 2e-3).abs() < 1e-6, "conditioning {cond}");
        assert!(!uses_fallback(&above));
        let n = Vec3::X;
        let expected = expected_normal(&above, n);
        // Hand value: the y-z plane maps to span(c1, c2), whose normal is
        // c1 x c2 = (sin, -cos, 0).
        let cos = (1.0f32 - 4e-6).sqrt();
        assert_close(
            expected,
            Vec3::new(2e-3, -cos, 0.0),
            1e-6,
            "inverse-transpose",
        );
        // The shader's cofactor form gives the same direction.
        let cofactor = safe_normalize(above.y_axis.cross(above.z_axis) * n.x);
        assert!(angle_between(expected, cofactor) < 1e-5);

        // Fallback side: sin = 5e-4 is half the threshold.
        let below = linear_part(&skew(5e-4));
        let cond = conditioning(&below);
        assert!((cond - 5e-4).abs() < 1e-6, "conditioning {cond}");
        assert!(uses_fallback(&below));
        let fallback = expected_normal(&below, n);
        assert_eq!(fallback, Vec3::X, "fallback is normalize(L * n)");
        // The branches disagree by ~90 degrees here, so the choice is observable.
        let inverse_transpose = safe_normalize(below.inverse().transpose() * n);
        assert!(angle_between(fallback, inverse_transpose) > 1.5);

        // Exactly singular: two equal columns.
        let singular = Mat3::from_cols(Vec3::X, Vec3::X, Vec3::Z);
        assert_eq!(conditioning(&singular), 0.0);
        assert!(uses_fallback(&singular));
        assert_eq!(expected_normal(&singular, Vec3::Y), Vec3::X);

        // A zero column conditions to 0, never NaN.
        let flattened = Mat3::from_cols(Vec3::X, Vec3::Y, Vec3::ZERO);
        assert_eq!(conditioning(&flattened), 0.0);
        assert_eq!(expected_normal(&flattened, Vec3::Z), Vec3::ZERO);

        // Mirrored blends are outside the domain.
        let mirrored = Mat3::from_diagonal(Vec3::new(-1.0, 1.0, 1.0));
        assert_eq!(conditioning(&mirrored), -1.0);
        assert!(uses_fallback(&mirrored));
        assert_eq!(expected_normal(&mirrored, Vec3::X), Vec3::NEG_X);

        // Uniform scale does not change the conditioning number.
        let scaled = Mat3::from_quat(Quat::from_rotation_z(0.7)) * 3.0;
        assert!((conditioning(&scaled) - 1.0).abs() < 1e-5);
        assert!(!uses_fallback(&scaled));
        let rotated = Quat::from_rotation_z(0.7) * Vec3::X;
        assert_close(expected_normal(&scaled, Vec3::X), rotated, 1e-6, "rotation");
    }

    #[test]
    fn bck_normal_zero_and_cancellation() {
        assert_eq!(safe_normalize(Vec3::ZERO), Vec3::ZERO);
        // Below the 1e-6 length floor.
        assert_eq!(safe_normalize(Vec3::new(1e-7, 0.0, 0.0)), Vec3::ZERO);
        // Above it: |v|^2 = 4e-12 >= 1e-12.
        assert_eq!(safe_normalize(Vec3::new(2e-6, 0.0, 0.0)), Vec3::X);
        assert_eq!(
            safe_normalize(Vec3::new(3.0, 4.0, 0.0)),
            Vec3::new(0.6, 0.8, 0.0)
        );

        // Fragment interpolation halfway between opposite vertex normals.
        let cancelled = Vec3::X * 0.5 + Vec3::NEG_X * 0.5;
        assert_eq!(cancelled, Vec3::ZERO);
        assert_eq!(safe_normalize(cancelled), Vec3::ZERO);

        // A zero input normal stays zero through both branches.
        let well = linear_part(&skew(0.5));
        assert_eq!(expected_normal(&well, Vec3::ZERO), Vec3::ZERO);
        let singular = Mat3::from_cols(Vec3::X, Vec3::X, Vec3::Z);
        assert_eq!(expected_normal(&singular, Vec3::ZERO), Vec3::ZERO);

        // Translation moves positions and leaves normals alone.
        let translated = Mat4::from_translation(Vec3::new(5.0, -3.0, 2.0));
        let oblique = Vec3::new(1.0, -2.0, 2.0) / 3.0;
        assert_eq!(expected_normal(&linear_part(&translated), oblique), oblique);
        assert_eq!(
            expected_position(&translated, Vec3::ONE),
            Vec3::new(6.0, -2.0, 3.0)
        );
    }

    #[test]
    fn bck_weighted_position_reference() {
        let palette = [
            Mat4::IDENTITY,
            Mat4::from_translation(Vec3::X),
            Mat4::from_rotation_z(FRAC_PI_2),
            Mat4::from_scale(Vec3::splat(2.0)),
        ];
        let p = Vec3::X;
        let four = skin([0, 1, 2, 3], [0.4, 0.3, 0.2, 0.1]);
        // Per joint: (1,0,0), (2,0,0), (0,1,0), (2,0,0); weighted by hand.
        let hand = Vec3::new(0.4 + 0.6 + 0.2, 0.2, 0.0);
        let per_joint: Vec3 = (0..4)
            .map(|slot| {
                palette[four.joints[slot] as usize].transform_point3(p) * four.weights[slot]
            })
            .sum();
        assert_close(per_joint, hand, 1e-6, "per-joint weighted sum");
        let blended = blend(&palette, &four);
        assert_close(expected_position(&blended, p), hand, 1e-6, "blended matrix");

        // A rigid vertex is the same path with one weight of 1.
        let rigid = skin([2, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        assert_close(
            expected_position(&blend(&palette, &rigid), p),
            Vec3::Y,
            1e-6,
            "rigid",
        );

        // Slot order does not matter.
        let permuted = skin([3, 2, 1, 0], [0.1, 0.2, 0.3, 0.4]);
        assert_close(
            expected_position(&blend(&palette, &permuted), p),
            hand,
            1e-6,
            "permuted slots",
        );
    }

    #[test]
    fn skin_records_widen_packed_influences() {
        use crate::animation_validation::SkinInfluence;

        let records = skin_records(&[[
            SkinInfluence {
                joint: 41,
                weight: 0.5,
            },
            SkinInfluence {
                joint: 3,
                weight: 0.25,
            },
            SkinInfluence {
                joint: 0,
                weight: 0.25,
            },
            SkinInfluence {
                joint: 0,
                weight: 0.0,
            },
        ]]);
        assert_eq!(records[0].joints, UVec4::new(41, 3, 0, 0));
        assert_eq!(records[0].weights, Vec4::new(0.5, 0.25, 0.25, 0.0));
    }

    // -------------------------------------------------------- shader sources

    const SKINNING_SOURCE: &str = include_str!("../shaders/source/skinning.slang");

    /// The three shaders with a fragment-stage normal consumer. The oracle
    /// covers only `skinSafeNormalize` itself, so this ties each call site to
    /// it by source inspection.
    const FRAGMENT_SOURCES: [(&str, &str); 3] = [
        (
            "toon_link.shader.slang",
            include_str!("../shaders/source/toon_link.shader.slang"),
        ),
        (
            "toon_link_modern.shader.slang",
            include_str!("../shaders/source/toon_link_modern.shader.slang"),
        ),
        ("tev.slang", include_str!("../shaders/source/tev.slang")),
    ];

    /// The `(line number, argument text)` of every bare `normalize(` call in
    /// `source`. `skinSafeNormalize(` never matches: its `N` is upper case,
    /// and the identifier check rejects any other suffix match.
    fn bare_normalize_calls(source: &str) -> Vec<(usize, &str)> {
        const CALL: &str = "normalize(";
        let mut calls = Vec::new();
        for (index, line) in source.lines().enumerate() {
            let mut from = 0;
            while let Some(found) = line[from..].find(CALL) {
                let start = from + found;
                let open = start + CALL.len();
                let argument = &line[open..];
                let argument = argument
                    .find(')')
                    .map_or(argument, |close| &argument[..close]);
                let is_suffix = line[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|previous| previous.is_alphanumeric() || previous == '_');
                if !is_suffix {
                    calls.push((index + 1, argument));
                }
                from = open;
            }
        }

        calls
    }

    /// The parsed value of `static const float NAME = VALUE;` in `source`.
    fn shader_float_constant(source: &str, name: &str) -> f32 {
        let declaration = format!("static const float {name} = ");
        let line = source
            .lines()
            .find(|line| line.contains(&declaration))
            .unwrap_or_else(|| panic!("skinning.slang declares no `{declaration}...;`"));
        let (_, value) = line
            .split_once(&declaration)
            .expect("the line contains the declaration");
        let value = value
            .strip_suffix(';')
            .unwrap_or_else(|| panic!("`{line}` does not end its declaration with `;`"));

        value
            .trim()
            .parse()
            .unwrap_or_else(|error| panic!("`{line}`: {value:?} is not a float: {error}"))
    }

    #[test]
    fn fragment_shaders_use_safe_normalize() {
        for (file, source) in FRAGMENT_SOURCES {
            assert!(
                source.lines().any(|line| line.trim() == "import skinning;"),
                "{file}: missing `import skinning;`"
            );
            for (line, argument) in bare_normalize_calls(source) {
                let lowered = argument.to_ascii_lowercase();
                assert!(
                    !lowered.contains("normal"),
                    "{file}:{line}: bare `normalize({argument})` on a normal; use `skinSafeNormalize`"
                );
                assert!(
                    argument.contains("lights.dir"),
                    "{file}:{line}: bare `normalize({argument})` is not a light direction"
                );
            }
            assert!(
                source.contains("skinSafeNormalize("),
                "{file}: no `skinSafeNormalize(` call site"
            );
        }
    }

    #[test]
    fn shader_policy_constants_match_reference() {
        assert_eq!(
            shader_float_constant(SKINNING_SOURCE, "SKIN_NORMAL_CONDITION_MIN"),
            NORMAL_CONDITION_MIN,
            "SKIN_NORMAL_CONDITION_MIN drifted from reference::NORMAL_CONDITION_MIN"
        );
        assert_eq!(
            shader_float_constant(SKINNING_SOURCE, "SKIN_SAFE_NORMALIZE_MIN_LENGTH_SQ"),
            SAFE_NORMALIZE_MIN_LENGTH_SQ,
            "SKIN_SAFE_NORMALIZE_MIN_LENGTH_SQ drifted from reference::SAFE_NORMALIZE_MIN_LENGTH_SQ"
        );
    }

    // ---------------------------------------------------------------- oracle

    /// One synthetic vertex for the GPU oracle. `literal_normal` is a hand
    /// value that the CPU reference must also reproduce, where one is simple
    /// enough to state.
    struct OracleFixture {
        name: &'static str,
        joints: [u32; 4],
        weights: [f32; 4],
        position: Vec3,
        normal: Vec3,
        /// Fed straight to `skinSafeNormalize`, like an interpolated normal.
        raw: Vec3,
        expect_fallback: bool,
        literal_normal: Option<Vec3>,
    }

    fn oracle_palette() -> Vec<Mat4> {
        vec![
            // 0: identity
            Mat4::IDENTITY,
            // 1: nonsymmetric affine, det 1.0375, translation (1, 2, 3)
            Mat4::from_cols(
                Vec4::new(1.0, 0.0, 0.3, 0.0),
                Vec4::new(0.5, 1.0, 0.0, 0.0),
                Vec4::new(0.0, 0.25, 1.0, 0.0),
                Vec4::new(1.0, 2.0, 3.0, 1.0),
            ),
            // 2: rotation about Y by 90 degrees, translation (0, 1, 0)
            Mat4::from_rotation_translation(Quat::from_rotation_y(FRAC_PI_2), Vec3::Y),
            // 3: translation only
            Mat4::from_translation(Vec3::new(5.0, -3.0, 2.0)),
            // 4: rotation about Z by 30 degrees, translation (0.5, 0, 0)
            Mat4::from_rotation_translation(
                Quat::from_rotation_z(30f32.to_radians()),
                Vec3::new(0.5, 0.0, 0.0),
            ),
            // 5: nonuniform scale
            Mat4::from_scale(Vec3::new(2.0, 1.0, 1.0)),
            // 6, 7: shears whose 50/50 blend conditions to ~(3e-3 + 1e-3) / 2
            // = 2e-3, above the threshold
            skew(3e-3),
            skew(1e-3),
            // 8, 9: opposite shears whose 50/50 blend is exactly singular
            skew(0.3f32.sin()),
            skew(-(0.3f32.sin())),
            // 10: mirror
            Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
            // 11, 12: shears whose 50/50 blend conditions to ~(7e-4 + 3e-4) / 2
            // = 5e-4, below the threshold but still invertible
            skew(7e-4),
            skew(3e-4),
        ]
    }

    fn oracle_fixtures() -> Vec<OracleFixture> {
        let rigid = |joint: u32| ([joint, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]);
        let half = |a: u32, b: u32| ([a, b, 0, 0], [0.5, 0.5, 0.0, 0.0]);
        let (identity_joints, identity_weights) = rigid(0);
        let (nonsym_joints, nonsym_weights) = rigid(1);
        let (rigid_joints, rigid_weights) = rigid(2);
        let (translation_joints, translation_weights) = rigid(3);
        let (mirror_joints, mirror_weights) = rigid(10);
        let (oblique_joints, oblique_weights) = half(4, 5);
        let (above_joints, above_weights) = half(6, 7);
        let (below_joints, below_weights) = half(11, 12);
        let (singular_joints, singular_weights) = half(8, 9);
        let oblique = Vec3::new(1.0, 2.0, 3.0).normalize();
        let cancelled = Vec3::X * 0.5 + Vec3::NEG_X * 0.5;

        vec![
            OracleFixture {
                name: "identity_bind",
                joints: identity_joints,
                weights: identity_weights,
                position: Vec3::new(0.25, -0.5, 0.75),
                normal: Vec3::new(0.6, 0.0, 0.8),
                raw: Vec3::new(3.0, 4.0, 0.0),
                expect_fallback: false,
                literal_normal: Some(Vec3::new(0.6, 0.0, 0.8)),
            },
            OracleFixture {
                name: "nonsymmetric_orientation",
                joints: nonsym_joints,
                weights: nonsym_weights,
                position: Vec3::new(1.0, 2.0, 3.0),
                normal: Vec3::Z,
                raw: cancelled,
                expect_fallback: false,
                // c0 x c1 = (-0.3, 0.15, 1), normalized.
                literal_normal: Some(Vec3::new(-0.3, 0.15, 1.0).normalize()),
            },
            OracleFixture {
                name: "rigid_position",
                joints: rigid_joints,
                weights: rigid_weights,
                position: Vec3::X,
                normal: Vec3::X,
                raw: Vec3::new(1e-7, 0.0, 0.0),
                expect_fallback: false,
                literal_normal: Some(Vec3::NEG_Z),
            },
            OracleFixture {
                name: "four_influence_position",
                joints: [0, 1, 2, 3],
                weights: [0.4, 0.3, 0.2, 0.1],
                position: Vec3::ONE,
                normal: Vec3::Y,
                raw: Vec3::new(0.0, -2.0, 0.0),
                expect_fallback: false,
                literal_normal: None,
            },
            OracleFixture {
                name: "oblique_normal_nonorthogonal_blend",
                joints: oblique_joints,
                weights: oblique_weights,
                position: Vec3::new(0.2, 0.3, 0.4),
                normal: oblique,
                raw: oblique * 2.5,
                expect_fallback: false,
                literal_normal: None,
            },
            OracleFixture {
                name: "near_singular_above_threshold",
                joints: above_joints,
                weights: above_weights,
                position: Vec3::ONE,
                normal: Vec3::X,
                raw: Vec3::new(2e-6, 0.0, 0.0),
                expect_fallback: false,
                literal_normal: None,
            },
            OracleFixture {
                name: "near_singular_below_threshold",
                joints: below_joints,
                weights: below_weights,
                position: Vec3::ONE,
                normal: Vec3::X,
                raw: Vec3::ZERO,
                expect_fallback: true,
                literal_normal: Some(Vec3::X),
            },
            OracleFixture {
                name: "singular_blend",
                joints: singular_joints,
                weights: singular_weights,
                position: Vec3::ONE,
                normal: Vec3::Y,
                raw: Vec3::new(0.0, 0.0, -0.5),
                expect_fallback: true,
                // L * (0,1,0) = c1 = (cos 0.3, 0, 0), normalized.
                literal_normal: Some(Vec3::X),
            },
            OracleFixture {
                name: "zero_input_normal",
                joints: nonsym_joints,
                weights: nonsym_weights,
                position: Vec3::new(-1.0, 0.5, 2.0),
                normal: Vec3::ZERO,
                raw: Vec3::new(0.0, 1e-9, 0.0),
                expect_fallback: false,
                literal_normal: Some(Vec3::ZERO),
            },
            OracleFixture {
                name: "translation_only_palette",
                joints: translation_joints,
                weights: translation_weights,
                position: Vec3::ONE,
                normal: Vec3::new(1.0, -2.0, 2.0) / 3.0,
                raw: Vec3::new(0.0, 0.0, 7.0),
                expect_fallback: false,
                literal_normal: Some(Vec3::new(1.0, -2.0, 2.0) / 3.0),
            },
            OracleFixture {
                name: "mirrored_blend",
                joints: mirror_joints,
                weights: mirror_weights,
                position: Vec3::new(1.0, 2.0, 3.0),
                normal: Vec3::X,
                raw: Vec3::new(-1.0, -1.0, 0.0),
                expect_fallback: true,
                literal_normal: Some(Vec3::NEG_X),
            },
        ]
    }

    #[test]
    fn oracle_fixtures_match_their_literals() {
        let palette = oracle_palette();
        for fixture in oracle_fixtures() {
            let blended = blend(&palette, &skin(fixture.joints, fixture.weights));
            let linear = linear_part(&blended);
            assert_eq!(
                uses_fallback(&linear),
                fixture.expect_fallback,
                "{}: conditioning {}",
                fixture.name,
                conditioning(&linear)
            );
            if let Some(literal) = fixture.literal_normal {
                let expected = expected_normal(&linear, fixture.normal);
                assert_close(expected, literal, 1e-5, fixture.name);
            }
        }
        // The nonsymmetric matrix pins the orientation convention.
        let nonsym = blend(&palette, &skin([1, 0, 0, 0], [1.0, 0.0, 0.0, 0.0]));
        assert_close(
            expected_position(&nonsym, Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(3.0, 4.75, 6.3),
            1e-6,
            "nonsymmetric position",
        );
    }

    /// AC10: run the production `skinning.slang` helpers on Vulkan and compare
    /// with the CPU reference. Fails, never skips, without a GPU.
    #[test]
    #[ignore = "needs Vulkan (lavapipe) and SDL offscreen; run `just toon_link test-skinning-gpu`"]
    fn skinning_gpu_oracle() {
        use mltrs::env_config::EnvConfig;
        use mltrs::renderer::{MaxMSAASamples, debug};
        use mltrs::shaders::atlas::ShaderAtlasRoot;

        use crate::generated::shader_atlas::ShaderAtlas;
        use crate::generated::shader_atlas::skinning_oracle_compute::{
            OracleCase, OracleOutput, Resources, SkinningOracleParams, SkinningOraclePush,
        };

        // Validation layers exist only in debug builds; `Renderer::init`'s
        // `#[cfg(debug_assertions)]` argument below does not compile otherwise.
        let sdl = sdl3::init().expect("SDL init (set SDL_VIDEODRIVER=offscreen)");
        let video = sdl.video().expect("SDL video subsystem");
        let window = video
            .window("toon_link skinning oracle", 64, 64)
            .vulkan()
            .hidden()
            .build()
            .expect("SDL Vulkan window");
        let mut renderer = Renderer::init(
            window,
            EnvConfig::from_env(),
            false,
            1.0,
            MaxMSAASamples::Off,
            false,
            #[cfg(debug_assertions)]
            ShaderAtlas::SHADERS_SOURCE_DIR,
        )
        .expect("Vulkan renderer (set VK_ICD_FILENAMES to lavapipe)");
        let shaders = ShaderAtlas::init();

        let palette = oracle_palette();
        let fixtures = oracle_fixtures();
        let joints: Vec<SkinJoint> = palette
            .iter()
            .map(|&transform| SkinJoint { transform })
            .collect();
        let records: Vec<VertexSkinning> =
            fixtures.iter().map(|f| skin(f.joints, f.weights)).collect();
        let cases: Vec<OracleCase> = fixtures
            .iter()
            .map(|f| OracleCase {
                position: f.position.extend(1.0),
                normal: f.normal.extend(0.0),
                raw_vector: f.raw.extend(0.0),
            })
            .collect();
        let case_count = u32::try_from(cases.len()).unwrap();

        let mut params = renderer
            .create_uniform_buffer::<SkinningOracleParams>()
            .expect("params buffer");
        let pipeline = renderer
            .create_compute_pipeline(shaders.skinning_oracle_compute.pipeline_config(Resources {
                params_buffer: &params,
            }))
            .expect("oracle compute pipeline");
        let palette_buffer = renderer.create_singleton_buffer(&joints).expect("palette");
        let skin_buffer = renderer.create_singleton_buffer(&records).expect("skin");
        let case_buffer = renderer.create_singleton_buffer(&cases).expect("cases");
        let output = renderer
            .create_gpu_only_buffer::<OracleOutput>(case_count)
            .expect("output");
        let params_data = SkinningOracleParams {
            palette: renderer.singleton_addr(&palette_buffer),
            skinning: renderer.singleton_addr(&skin_buffer),
            cases: renderer.singleton_addr(&case_buffer),
            case_count,
            _padding_0: Default::default(),
        };
        let values = renderer
            .dispatch_readback(&pipeline, &output, [case_count, 1, 1], |gpu, output| {
                gpu.write_uniform(&mut params, params_data);
                SkinningOraclePush { output }
            })
            .expect("dispatch and read back");
        assert_eq!(values.len(), fixtures.len());

        let mut failures = Vec::new();
        for (index, (fixture, actual)) in fixtures.iter().zip(&values).enumerate() {
            let mut fail = |message: String| {
                println!("  FAIL {message}");
                failures.push(format!("{}: {message}", fixture.name));
            };
            let blended = blend(&palette, &records[index]);
            let linear = linear_part(&blended);
            let expected_position = expected_position(&blended, fixture.position);
            let expected_normal = expected_normal(&linear, fixture.normal);
            let expected_cond = conditioning(&linear);
            let expected_fallback = uses_fallback(&linear);
            let expected_safe = safe_normalize(fixture.raw);
            let actual_position = actual.position.truncate();
            let actual_normal = actual.normal.truncate();
            let actual_safe = actual.safe_normalized.truncate();
            let actual_fallback = actual.flags.x != 0;

            println!("case {index} {}", fixture.name);
            println!(
                "  position expected={expected_position:?} actual={actual_position:?} tol={POSITION_TOLERANCE:e}"
            );
            println!(
                "  normal   expected={expected_normal:?} actual={actual_normal:?} tol={ANGULAR_TOLERANCE_RAD:e} rad"
            );
            println!(
                "  cond     expected={expected_cond:e} actual={:e} det={:e} fallback expected={expected_fallback} actual={actual_fallback}",
                actual.normal.w, actual.position.w
            );
            println!(
                "  safe     input={:?} expected={expected_safe:?} actual={actual_safe:?} |input|={}",
                fixture.raw, actual.safe_normalized.w
            );

            let finite = [actual.position, actual.normal, actual.safe_normalized]
                .iter()
                .all(|v| v.is_finite());
            if !finite {
                fail("nonfinite output".to_owned());
            }
            if actual.flags.y != index as u32 {
                fail(format!(
                    "case index {} written as {}",
                    index, actual.flags.y
                ));
            }
            let position_error = (actual_position - expected_position).abs().max_element();
            if exceeds(position_error, POSITION_TOLERANCE) {
                fail(format!("position error {position_error:e}"));
            }
            if actual_fallback != expected_fallback {
                fail(format!(
                    "fallback flag {actual_fallback}, expected {expected_fallback}"
                ));
            }
            if fixture.expect_fallback != expected_fallback {
                fail(format!(
                    "fixture expects fallback {}",
                    fixture.expect_fallback
                ));
            }
            let cond_error = (actual.normal.w - expected_cond).abs();
            if exceeds(cond_error, CONDITIONING_TOLERANCE) {
                fail(format!("conditioning error {cond_error:e}"));
            }
            check_direction(&mut fail, "normal", expected_normal, actual_normal);
            check_direction(&mut fail, "safe-normalized", expected_safe, actual_safe);
            if let Some(literal) = fixture.literal_normal {
                let literal_error = (expected_normal - literal).abs().max_element();
                if exceeds(literal_error, 1e-5) {
                    fail(format!(
                        "reference {expected_normal:?} disagrees with hand value {literal:?}"
                    ));
                }
            }
        }

        drop(renderer);
        let validation = debug::validation_message_count();
        if validation != 0 {
            failures.push(format!("Vulkan validation reported {validation} messages"));
        }
        assert!(
            failures.is_empty(),
            "skinning GPU oracle failed:\n{}",
            failures.join("\n")
        );
        println!(
            "SKINNING_GPU_ORACLE_OK: {} cases; validation=0; teardown complete",
            fixtures.len()
        );
    }

    /// True when `error` is outside `tolerance`, counting NaN as a failure
    /// (a plain `error > tolerance` would let NaN through).
    fn exceeds(error: f32, tolerance: f32) -> bool {
        error.is_nan() || error > tolerance
    }

    /// A zero expectation must be exactly zero; otherwise both must be unit
    /// and agree within the angular tolerance.
    fn check_direction(fail: &mut impl FnMut(String), what: &str, expected: Vec3, actual: Vec3) {
        if expected == Vec3::ZERO {
            if actual != Vec3::ZERO {
                fail(format!("{what} expected exactly zero, got {actual:?}"));
            }
            return;
        }
        let length_error = (actual.length() - 1.0).abs();
        if exceeds(length_error, 1e-4) {
            fail(format!("{what} length error {length_error:e}"));
        }
        let angle = angle_between(expected, safe_normalize(actual));
        if exceeds(angle, ANGULAR_TOLERANCE_RAD) {
            fail(format!("{what} angle {angle:e} rad"));
        }
    }
}
