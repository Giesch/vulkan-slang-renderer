//! CPU-only BCK sampling. Preparation validates once; evaluation never publishes state.
use std::rc::Rc;

use anyhow::{Context, Result, ensure};
use glam::{Mat4, Vec3};
use gx::animation_manifest::{BckClip, TrackF32, TrackI16};
use gx::model_manifest::Skeleton;

use crate::animation_validation::{validate_bck, validate_skeleton};

/// Absolute local-scale divisor floor for Maya compensation. Negative scales are
/// supported, but abs(scale) <= 1e-6 is rejected only when used as a divisor.
pub const COMPENSATION_SCALE_MIN: f32 = 1e-6;

pub struct PreparedSkeleton {
    skeleton: Skeleton,
    inverse_bind: Vec<Mat4>,
    bind_model_space: Vec<Mat4>,
}

/// Owns shared, validated, immutable inputs so frame evaluation needs no
/// revalidation and the owner needs no lifetime.
pub struct PreparedClip {
    skeleton: Rc<PreparedSkeleton>,
    clip: Rc<BckClip>,
    /// Indices into `clip.joints`, sorted by joint ordinal.
    joints: Vec<usize>,
}

#[derive(Debug)]
pub struct Pose {
    /// Animated model-space transform per joint. Only the pose tests read it; the
    /// draw paths upload `palette`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub model_space: Vec<Mat4>,
    /// `animated_model_space * inverse_bind`, one entry per skeleton joint.
    pub palette: Vec<Mat4>,
    /// Evaluated local scale per joint, kept for the Maya compensation tests.
    #[cfg_attr(not(test), allow(dead_code))]
    pub local_scales: Vec<Vec3>,
}

impl PreparedSkeleton {
    pub fn new(skeleton: &Skeleton) -> Result<Self> {
        validate_skeleton(skeleton)?;
        let mut bind_model_space: Vec<Mat4> = Vec::with_capacity(skeleton.joints.len());
        let mut inverse_bind = Vec::with_capacity(skeleton.joints.len());
        for (index, joint) in skeleton.joints.iter().enumerate() {
            let local = local_matrix(Vec3::from_array(joint.t), joint.r_s16, Vec3::ONE, Vec3::ONE)
                .with_context(|| format!("bind.joints[{index}].local"))?;
            let model_space = if joint.parent < 0 {
                local
            } else {
                bind_model_space[joint.parent as usize] * local
            };
            finite_matrix(model_space)
                .with_context(|| format!("bind.joints[{index}].model_space"))?;
            let inverse = model_space.inverse();
            finite_matrix(inverse).with_context(|| format!("bind.joints[{index}].inverse"))?;
            bind_model_space.push(model_space);
            inverse_bind.push(inverse);
        }

        Ok(Self {
            skeleton: skeleton.clone(),
            inverse_bind,
            bind_model_space,
        })
    }

    pub fn bind_pose(&self) -> Result<Pose> {
        self.finish(
            self.bind_model_space.clone(),
            vec![Vec3::ONE; self.bind_model_space.len()],
        )
    }

    /// Catalog/document identity validation remains the caller's responsibility
    /// (`animation_validation::validate_clip`); structural BCK validation is here.
    pub fn prepare_clip(self: &Rc<Self>, clip: Rc<BckClip>) -> Result<PreparedClip> {
        validate_bck(&clip, &self.skeleton)?;
        let mut joints: Vec<usize> = (0..clip.joints.len()).collect();
        joints.sort_by_key(|&index| clip.joints[index].ordinal);

        Ok(PreparedClip {
            skeleton: Rc::clone(self),
            clip,
            joints,
        })
    }

    fn finish(&self, model_space: Vec<Mat4>, local_scales: Vec<Vec3>) -> Result<Pose> {
        let palette = model_space
            .iter()
            .zip(&self.inverse_bind)
            .enumerate()
            .map(|(index, (&model_space, &inverse))| {
                let matrix = model_space * inverse;
                finite_matrix(matrix).with_context(|| format!("joints[{index}].palette"))?;

                Ok(matrix)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Pose {
            model_space,
            palette,
            local_scales,
        })
    }
}

impl PreparedClip {
    /// The validated clip this pose sampler was prepared from.
    pub fn clip(&self) -> &BckClip {
        &self.clip
    }

    /// Samples raw frame time and clamps to track endpoints. Playback handles loop wrapping.
    /// All failures include the requested frame; the caller retains its last pose.
    pub fn evaluate(&self, frame: f32) -> Result<Pose> {
        self.evaluate_inner(frame)
            .with_context(|| format!("pose.frame[{frame}]"))
    }

    fn evaluate_inner(&self, frame: f32) -> Result<Pose> {
        ensure!(frame.is_finite(), "frame: nonfinite");

        let mut model_space: Vec<Mat4> = Vec::with_capacity(self.joints.len());
        let mut local_scales: Vec<Vec3> = Vec::with_capacity(self.joints.len());
        for (index, (joint, bind)) in self
            .joints
            .iter()
            .map(|&joint_index| &self.clip.joints[joint_index])
            .zip(&self.skeleton.skeleton.joints)
            .enumerate()
        {
            let sample = || -> Result<(Mat4, Vec3)> {
                let mut scale = Vec3::ONE;
                let mut translation = Vec3::ZERO;
                let mut rotation = [0; 3];
                for (axis_index, axis) in joint.axes.iter().enumerate() {
                    scale[axis_index] = sample_f32(&axis.scale, frame, 1.0)
                        .with_context(|| format!("axes[{axis_index}].scale"))?;
                    translation[axis_index] = sample_f32(&axis.translation, frame, 0.0)
                        .with_context(|| format!("axes[{axis_index}].translation"))?;
                    rotation[axis_index] =
                        sample_rotation(&axis.rotation, frame, self.clip.rotation_decimal_shift)
                            .with_context(|| format!("axes[{axis_index}].rotation"))?;
                }

                let compensation = if bind.parent >= 0 && bind.scale_compensate {
                    let parent_scale = local_scales[bind.parent as usize];
                    ensure!(
                        parent_scale.abs().min_element() > COMPENSATION_SCALE_MIN,
                        "compensation: parent local scale {parent_scale:?} has divisor abs <= {COMPENSATION_SCALE_MIN}"
                    );

                    parent_scale.recip()
                } else {
                    Vec3::ONE
                };

                let local = local_matrix(translation, rotation, scale, compensation)?;
                let matrix = if bind.parent < 0 {
                    local
                } else {
                    model_space[bind.parent as usize] * local
                };
                finite_matrix(matrix).context("model_space: nonfinite hierarchy product")?;

                Ok((matrix, scale))
            };

            let (matrix, scale) = sample().with_context(|| format!("joints[{index}]"))?;
            model_space.push(matrix);
            local_scales.push(scale);
        }

        self.skeleton.finish(model_space, local_scales)
    }
}

fn finite_matrix(matrix: Mat4) -> Result<()> {
    ensure!(matrix.is_finite(), "nonfinite matrix");

    Ok(())
}

fn local_matrix(
    translation: Vec3,
    rotation: [i16; 3],
    scale: Vec3,
    compensation: Vec3,
) -> Result<Mat4> {
    let radians = rotation.map(|angle| f32::from(angle) * (std::f32::consts::PI / 32768.0));
    let rotation = Mat4::from_rotation_z(radians[2])
        * Mat4::from_rotation_y(radians[1])
        * Mat4::from_rotation_x(radians[0]);

    // J3D Maya compensation scales ROWS of R*S, not columns. Translation stays
    // untouched: T * inverse(parent LOCAL S) * Rz * Ry * Rx * S.
    let compensated = Mat4::from_scale(compensation) * rotation;
    finite_matrix(compensated)?;

    let linear = compensated * Mat4::from_scale(scale);
    finite_matrix(linear)?;

    let local = Mat4::from_translation(translation) * linear;
    finite_matrix(local)?;

    Ok(local)
}

fn hermite(frame: f32, left: [f32; 3], right: [f32; 3]) -> Result<f32> {
    // `outgoing` and `incoming` refer to the keyframes,
    // not this curve between them
    let [start_time, start_value, outgoing] = left;
    let [end_time, end_value, incoming] = right;

    let interval = end_time - start_time;
    ensure!(
        interval.is_finite() && interval > 0.0,
        "Hermite interval: {interval} invalid"
    );

    let fraction = (frame - start_time) / interval;
    ensure!(fraction.is_finite(), "Hermite fraction: nonfinite");

    let squared = fraction * fraction;
    let cubed = squared * fraction;
    let start_slope = interval * outgoing;
    let end_slope = interval * incoming;
    ensure!(
        start_slope.is_finite() && end_slope.is_finite(),
        "Hermite interval-scaled tangent: nonfinite"
    );

    let terms = [
        (2.0 * cubed - 3.0 * squared + 1.0) * start_value,
        (cubed - 2.0 * squared + fraction) * start_slope,
        (-2.0 * cubed + 3.0 * squared) * end_value,
        (cubed - squared) * end_slope,
    ];

    let mut value = 0.0;
    for term in terms {
        ensure!(term.is_finite(), "Hermite term: nonfinite");
        value += term;
        ensure!(value.is_finite(), "Hermite sum: nonfinite");
    }

    Ok(value)
}

// Private samplers accept only tracks validated by prepare_clip.
fn sample_f32(track: &TrackF32, frame: f32, default: f32) -> Result<f32> {
    let value = match track {
        TrackF32::Default => default,
        TrackF32::Constant { value } => *value,
        TrackF32::Keyed { keys, .. } => {
            if frame <= keys[0].time {
                keys[0].value
            } else if frame >= keys[keys.len() - 1].time {
                keys[keys.len() - 1].value
            } else {
                let index = keys.partition_point(|key| key.time <= frame);
                let a = keys[index - 1];
                let b = keys[index];
                if frame == a.time {
                    a.value
                } else {
                    hermite(
                        frame,
                        [a.time, a.value, a.tangent_out],
                        [b.time, b.value, b.tangent_in],
                    )?
                }
            }
        }
    };

    ensure!(value.is_finite(), "sample: nonfinite");

    Ok(value)
}

fn sample_rotation(track: &TrackI16, frame: f32, shift: u8) -> Result<i16> {
    let raw = match track {
        TrackI16::Default => 0.0,
        TrackI16::Constant { value } => f32::from(*value),
        TrackI16::Keyed { keys, .. } => {
            if frame <= f32::from(keys[0].time) {
                f32::from(keys[0].value)
            } else if frame >= f32::from(keys[keys.len() - 1].time) {
                f32::from(keys[keys.len() - 1].value)
            } else {
                let index = keys.partition_point(|key| f32::from(key.time) <= frame);
                let a = keys[index - 1];
                let b = keys[index];

                hermite(
                    frame,
                    [a.time, a.value, a.tangent_out].map(f32::from),
                    [b.time, b.value, b.tangent_in].map(f32::from),
                )?
            }
        }
    };

    ensure!(raw.is_finite(), "rotation sample: nonfinite");

    // Reduce before integer conversion: Rust's saturating float casts must not
    // replace the source's signed-16 wrap for overshooting Hermite curves.
    let wrapped = raw.trunc().rem_euclid(65536.0) as u32;

    Ok((wrapped << shift) as u16 as i16)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gx::animation_manifest::{AxisSrt, BasMetadata, BckJoint, KeyF32, KeyI16};
    use gx::model_manifest::{ScalingRule, SkeletonJoint};

    // f32 matrix products and trig tolerate 2e-5 absolute error in these small fixtures.
    const TOLERANCE: f32 = 2e-5;

    fn skeleton(parents: &[i32]) -> Skeleton {
        Skeleton {
            scaling_rule: ScalingRule::Maya,
            joints: parents
                .iter()
                .enumerate()
                .map(|(index, &parent)| SkeletonJoint {
                    name: format!("joint{index}"),
                    parent,
                    t: [0.0; 3],
                    r_s16: [0; 3],
                    s: [1.0; 3],
                    scale_compensate: false,
                })
                .collect(),
        }
    }

    fn clip(count: usize) -> BckClip {
        BckClip {
            duration_frames: 20,
            loop_attribute: 2,
            rotation_decimal_shift: 0,
            joints: (0..count)
                .map(|index| BckJoint {
                    ordinal: index as u16,
                    axes: std::array::from_fn(|_| AxisSrt {
                        scale: TrackF32::Default,
                        rotation: TrackI16::Default,
                        translation: TrackF32::Default,
                    }),
                })
                .collect(),
            bas: BasMetadata {
                present: false,
                offset: 0,
                length: 0,
            },
        }
    }

    fn close(actual: Vec3, expected: Vec3) {
        assert!(
            (actual - expected).abs().max_element() <= TOLERANCE,
            "{actual:?} != {expected:?}"
        );
    }

    fn rotation_keys(start_value: i16, end_value: i16) -> TrackI16 {
        TrackI16::Keyed {
            tangent_type: 7,
            keys: vec![
                KeyI16 {
                    time: 10,
                    value: start_value,
                    tangent_in: 0,
                    tangent_out: 2,
                },
                KeyI16 {
                    time: 20,
                    value: end_value,
                    tangent_in: 4,
                    tangent_out: 0,
                },
            ],
        }
    }

    #[test]
    fn bck_hermite_interval_oracle() {
        assert_eq!(
            hermite(15.0, [10.0, 100.0, 2.0], [20.0, 300.0, 4.0]).unwrap(),
            197.5
        );
    }

    #[test]
    fn bck_f32_interpolation_uses_interval_scaled_outgoing_and_incoming_tangents() {
        let track = TrackF32::Keyed {
            tangent_type: 9,
            keys: vec![
                KeyF32 {
                    time: 3.0,
                    value: 10.0,
                    tangent_in: 99.0,
                    tangent_out: 2.0,
                },
                KeyF32 {
                    time: 10.0,
                    value: 20.0,
                    tangent_in: -4.0,
                    tangent_out: 99.0,
                },
            ],
        };

        assert_eq!(sample_f32(&track, 6.5, 0.0).unwrap(), 20.25);
    }

    #[test]
    fn bck_rotation_truncates_toward_zero_before_shifting() {
        assert_eq!(
            sample_rotation(&rotation_keys(100, 300), 15.0, 3).unwrap(),
            1576
        );
        assert_eq!(
            sample_rotation(&rotation_keys(-300, -100), 15.0, 3).unwrap(),
            -1616
        );
    }

    #[test]
    fn bck_constant_rotation_wraps_after_shifting() {
        assert_eq!(
            sample_rotation(&TrackI16::Constant { value: 32767 }, 0.0, 3).unwrap(),
            -8
        );
        assert_eq!(
            sample_rotation(&TrackI16::Constant { value: -32768 }, 0.0, 3).unwrap(),
            0
        );
    }

    #[test]
    fn bck_rotation_interpolates_across_signed_boundary_without_shortest_arc() {
        let track = TrackI16::Keyed {
            tangent_type: 0,
            keys: vec![
                KeyI16 {
                    time: 0,
                    value: -32760,
                    tangent_in: 0,
                    tangent_out: 0,
                },
                KeyI16 {
                    time: 2,
                    value: 32760,
                    tangent_in: 0,
                    tangent_out: 0,
                },
            ],
        };

        assert_eq!(sample_rotation(&track, 1.0, 0).unwrap(), 0); // not shortest arc
    }

    #[test]
    fn bck_rotation_wraps_hermite_overshoot() {
        let overshoot = TrackI16::Keyed {
            tangent_type: 1,
            keys: vec![
                KeyI16 {
                    time: 0,
                    value: 0,
                    tangent_in: 0,
                    tangent_out: 32767,
                },
                KeyI16 {
                    time: 100,
                    value: 0,
                    tangent_in: -32767,
                    tangent_out: 0,
                },
            ],
        };

        assert_eq!(sample_rotation(&overshoot, 50.0, 0).unwrap(), 32743); // 819175 modulo 65536
    }

    #[test]
    fn bck_track_defaults_and_constants() {
        assert_eq!(sample_f32(&TrackF32::Default, 4.0, 7.0).unwrap(), 7.0);
        assert_eq!(
            sample_f32(&TrackF32::Constant { value: -2.0 }, -8.0, 0.0).unwrap(),
            -2.0
        );
        assert_eq!(sample_rotation(&TrackI16::Default, 4.0, 3).unwrap(), 0);
    }

    #[test]
    fn bck_rotation_clamps_to_track_endpoints() {
        for (frame, expected) in [(-10.0, 100), (10.0, 100), (20.0, 300), (100.0, 300)] {
            assert_eq!(
                sample_rotation(&rotation_keys(100, 300), frame, 0).unwrap(),
                expected,
                "frame {frame}"
            );
        }
    }

    #[test]
    fn bck_default_tracks_evaluate_to_unit_scale_and_zero_translation() {
        let mut bind = skeleton(&[-1]);
        bind.joints[0].t = [3.0, 4.0, 5.0];
        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());
        let data = clip(1);
        let pose = prepared
            .prepare_clip(Rc::new(data))
            .unwrap()
            .evaluate(0.0)
            .unwrap();

        close(pose.model_space[0].transform_point3(Vec3::ZERO), Vec3::ZERO);
        assert_eq!(pose.local_scales, [Vec3::ONE]);
    }

    fn nontrivial_bind_skeleton() -> Skeleton {
        let mut bind = skeleton(&[-1, 0, 1]);
        bind.joints[0].t = [3.0, -4.0, 1.0];
        bind.joints[0].r_s16 = [4096, 8192, -2048];
        bind.joints[1].t = [-2.0, 3.0, 7.0];
        bind.joints[1].r_s16 = [-8192, 1024, 16384];
        bind.joints[2].t = [1.0, 2.0, -3.0];

        bind
    }

    fn clip_with_bind_translation_and_rotation(bind: &Skeleton) -> BckClip {
        let mut data = clip(bind.joints.len());
        for (animated, joint) in data.joints.iter_mut().zip(&bind.joints) {
            for axis in 0..3 {
                animated.axes[axis].translation = TrackF32::Constant {
                    value: joint.t[axis],
                };
                animated.axes[axis].rotation = TrackI16::Constant {
                    value: joint.r_s16[axis],
                };
            }
        }

        data
    }

    #[test]
    fn bck_bind_identity_nontrivial_hierarchy() {
        let bind = nontrivial_bind_skeleton();
        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());
        let pose = prepared.bind_pose().unwrap();
        for matrix in pose.palette {
            for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z] {
                close(matrix.transform_point3(point), point);
            }
        }
    }

    #[test]
    fn bck_clip_matching_bind_has_identity_palette() {
        let bind = nontrivial_bind_skeleton();
        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());
        let data = clip_with_bind_translation_and_rotation(&bind);
        let pose = prepared
            .prepare_clip(Rc::new(data))
            .unwrap()
            .evaluate(0.0)
            .unwrap();

        for matrix in pose.palette {
            close(
                matrix.transform_point3(Vec3::new(2.0, -1.0, 3.0)),
                Vec3::new(2.0, -1.0, 3.0),
            );
        }
    }

    /// Pins `palette = animated_model_space * inverse_bind` (not the reverse product):
    /// the palette must carry a bind-space point to its animated-space image.
    #[test]
    fn bck_palette_maps_bind_space_to_animated_space() {
        let mut bind = skeleton(&[-1, 0]);
        bind.joints[0].t = [3.0, -4.0, 1.0];
        bind.joints[0].r_s16 = [4096, 8192, -2048];
        bind.joints[1].t = [-2.0, 3.0, 7.0];
        bind.joints[1].r_s16 = [-8192, 1024, 16384];
        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());
        // Start from bind, then move the root +1 on x and re-rotate the child.
        let mut data = clip_with_bind_translation_and_rotation(&bind);
        data.joints[0].axes[0].translation = TrackF32::Constant {
            value: bind.joints[0].t[0] + 1.0,
        };
        data.joints[1].axes[1].rotation = TrackI16::Constant { value: -12288 };
        let pose = prepared
            .prepare_clip(Rc::new(data))
            .unwrap()
            .evaluate(0.0)
            .unwrap();
        let point = Vec3::new(0.3, -0.7, 1.1);
        for (joint, (palette, model_space)) in
            pose.palette.iter().zip(&pose.model_space).enumerate()
        {
            let bound = prepared.bind_model_space[joint].transform_point3(point);
            let animated = model_space.transform_point3(point);
            assert!(
                (animated - bound).abs().max_element() > 0.5,
                "joint {joint}: pose equals bind, so the product order would not matter"
            );
            close(palette.transform_point3(bound), animated);
        }
    }

    /// Pins the Euler composition order `Rz * Ry * Rx` in `local_matrix` against
    /// a hand-derived image, independent of the bind path.
    #[test]
    fn bck_euler_order_is_z_y_x() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1])).unwrap());
        let mut data = clip(1);
        data.joints[0].axes[0].rotation = TrackI16::Constant { value: 16384 }; // x = 90 deg
        data.joints[0].axes[1].rotation = TrackI16::Constant { value: 16384 }; // y = 90 deg
        data.joints[0].axes[2].rotation = TrackI16::Constant { value: 8192 }; // z = 45 deg
        let pose = prepared
            .prepare_clip(Rc::new(data))
            .unwrap()
            .evaluate(0.0)
            .unwrap();
        // Independent scalar rotations of (1,2,3):
        // Rx90 -> (1,-3,2), Ry90 -> (2,-3,-1),
        // Rz45 -> ((2+3)/sqrt(2), (2-3)/sqrt(2), -1).
        let actual = pose.model_space[0].transform_point3(Vec3::new(1.0, 2.0, 3.0));
        let half_root = std::f32::consts::FRAC_1_SQRT_2;
        let expected = Vec3::new(5.0 * half_root, -half_root, -1.0);

        close(actual, expected);
    }

    #[test]
    fn bck_maya_nonuniform_rotated_parent_child_and_sibling() {
        let mut bind = skeleton(&[-1, 0, 0]);
        bind.joints[1].scale_compensate = true;
        bind.joints[2].scale_compensate = true;
        let mut data = clip(3);
        for (axis, value) in [2.0, 3.0, 4.0].into_iter().enumerate() {
            data.joints[0].axes[axis].scale = TrackF32::Constant { value };
        }

        data.joints[0].axes[2].rotation = TrackI16::Constant { value: 16384 };
        data.joints[0].axes[0].translation = TrackF32::Constant { value: 10.0 };
        data.joints[1].axes[2].rotation = TrackI16::Constant { value: 16384 };
        for (axis, value) in [5.0, 7.0, 11.0].into_iter().enumerate() {
            data.joints[1].axes[axis].scale = TrackF32::Constant { value };
            data.joints[1].axes[axis].translation = TrackF32::Constant {
                value: (axis + 1) as f32,
            };
        }

        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());
        let pose = prepared
            .prepare_clip(Rc::new(data))
            .unwrap()
            .evaluate(0.0)
            .unwrap();
        // Independent scalar oracle: child S=(5x,7y,11z), Rz90=(-7y,5x,11z),
        // divide by (2,3,4), add (1,2,3); parent scales (2,3,4),
        // Rz90 maps (x,y,z)->(-y,x,z), then adds (10,0,0).
        close(
            pose.model_space[1].transform_point3(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(-1.0, -12.0, 45.0),
        );
        // Sibling must use root (2,3,4), NOT preceding child's (5,7,11).
        close(
            pose.model_space[2].transform_point3(Vec3::new(1.0, 2.0, 3.0)),
            Vec3::new(8.0, 1.0, 3.0),
        );
        assert_eq!(pose.local_scales[0], Vec3::new(2.0, 3.0, 4.0));
    }

    #[test]
    fn bck_compensation_accepts_divisors_above_threshold() {
        let mut bind = skeleton(&[-1, 0]);
        bind.joints[1].scale_compensate = true;
        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());
        for value in [
            -2.0,
            -2.0 * COMPENSATION_SCALE_MIN,
            2.0 * COMPENSATION_SCALE_MIN,
        ] {
            let mut data = clip(2);
            data.joints[0].axes[0].scale = TrackF32::Constant { value };
            let pose = prepared
                .prepare_clip(Rc::new(data))
                .unwrap()
                .evaluate(0.0)
                .unwrap_or_else(|error| panic!("scale {value}: {error:#}"));
            let actual = pose.model_space[1].transform_point3(Vec3::X);
            assert!(
                (actual - Vec3::X).abs().max_element() <= TOLERANCE,
                "scale {value}: {actual:?} != {:?}",
                Vec3::X
            );
        }
    }

    #[test]
    fn bck_compensation_rejects_divisors_at_or_below_threshold() {
        let mut bind = skeleton(&[-1, 0]);
        bind.joints[1].scale_compensate = true;
        let prepared = Rc::new(PreparedSkeleton::new(&bind).unwrap());

        for value in [0.0, COMPENSATION_SCALE_MIN, -COMPENSATION_SCALE_MIN] {
            let mut data = clip(2);
            data.joints[0].axes[0].scale = TrackF32::Constant { value };
            assert!(
                prepared
                    .prepare_clip(Rc::new(data))
                    .unwrap()
                    .evaluate(0.0)
                    .is_err(),
                "scale {value} must be rejected as a compensation divisor"
            );
        }
    }

    #[test]
    fn bck_zero_scale_is_valid_without_compensation() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1, 0])).unwrap());
        let mut data = clip(2);
        data.joints[0].axes[0].scale = TrackF32::Constant { value: 0.0 };
        let pose = prepared
            .prepare_clip(Rc::new(data))
            .unwrap()
            .evaluate(0.0)
            .unwrap();

        close(pose.model_space[1].transform_point3(Vec3::X), Vec3::ZERO);
    }

    #[test]
    fn bck_hermite_overflow_fails_between_valid_endpoints() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1, 0])).unwrap());
        let mut data = clip(2);
        data.joints[0].axes[0].scale = TrackF32::Keyed {
            tangent_type: 1,
            keys: vec![
                KeyF32 {
                    time: 0.0,
                    value: 1.0,
                    tangent_in: 0.0,
                    tangent_out: f32::MAX,
                },
                KeyF32 {
                    time: 10.0,
                    value: 1.0,
                    tangent_in: -f32::MAX,
                    tangent_out: 0.0,
                },
            ],
        };
        let candidate = prepared.prepare_clip(Rc::new(data)).unwrap();

        assert!(candidate.evaluate(0.0).is_ok());
        let error = candidate.evaluate(5.0).unwrap_err();
        assert!(format!("{error:#}").contains("Hermite"), "{error:#}");
        assert!(candidate.evaluate(10.0).is_ok());
    }

    #[test]
    fn bck_evaluation_rejects_nonfinite_frame() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1])).unwrap());
        let candidate = prepared.prepare_clip(Rc::new(clip(1))).unwrap();

        assert!(candidate.evaluate(f32::NAN).is_err());
    }

    #[test]
    fn bck_evaluation_rejects_hierarchy_overflow() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1, 0])).unwrap());
        let mut data = clip(2);
        for joint in &mut data.joints {
            joint.axes[0].scale = TrackF32::Constant { value: 1e30 };
        }

        let candidate = prepared.prepare_clip(Rc::new(data)).unwrap();
        let error = candidate.evaluate(0.0).unwrap_err();

        assert!(format!("{error:#}").contains("hierarchy"), "{error:#}");
    }

    #[test]
    fn bck_keyed_scale_and_translation_with_reordered_joints() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1, 0])).unwrap());
        let mut data = clip(2);
        let track = TrackF32::Keyed {
            tangent_type: 5,
            keys: vec![
                KeyF32 {
                    time: 2.0,
                    value: -2.0,
                    tangent_in: 99.0,
                    tangent_out: 0.0,
                },
                KeyF32 {
                    time: 8.0,
                    value: 4.0,
                    tangent_in: 0.0,
                    tangent_out: 99.0,
                },
            ],
        };
        data.joints[0].axes[0].scale = track.clone();
        data.joints[1].axes[1].translation = track;
        data.joints.reverse();
        let candidate = prepared.prepare_clip(Rc::new(data)).unwrap();
        for (frame, expected) in [
            (-1.0, -2.0),
            (2.0, -2.0),
            (5.0, 1.0),
            (8.0, 4.0),
            (20.0, 4.0),
        ] {
            let pose = candidate.evaluate(frame).unwrap();
            close(
                pose.model_space[1].transform_point3(Vec3::X),
                Vec3::new(expected, expected, 0.0),
            );
            assert_eq!(pose.local_scales[0].x, expected);
        }
    }

    #[test]
    fn bck_skeleton_preparation_rejects_invalid_hierarchy() {
        assert!(PreparedSkeleton::new(&skeleton(&[-1, 1])).is_err());
    }

    #[test]
    fn bck_clip_preparation_rejects_invalid_rotation_shift() {
        let prepared = Rc::new(PreparedSkeleton::new(&skeleton(&[-1])).unwrap());
        let mut data = clip(1);
        data.rotation_decimal_shift = 4;
        assert!(prepared.prepare_clip(Rc::new(data)).is_err());
    }
}
