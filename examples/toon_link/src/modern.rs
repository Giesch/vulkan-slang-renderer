//! Independent linear-light renderer. No TEV packing or classic raster policy.
use std::time::Instant;

use anyhow::Context;
use facet::Facet;
use glam::{Mat4, Vec2, Vec3};
use gx::model_manifest::{self as mm, Manifest, MaterialEntry};
use mltrs::editor::{Checkbox, RGBPicker, Slider};
use mltrs::game::Game;
use mltrs::renderer::render_graph::DrawIndexedIndirectCommand;
use mltrs::renderer::{
    BlendMode, CullMode, DepthCompare, DrawError, DrawIndexedIndirect, FrameRenderer,
    ImmutableBufferHandle, PipelineHandle, PushBlock, RasterState, Renderer, RgbaPixels,
    SingletonBufferHandle, StencilMode, TextureColorSpace, TextureHandle, UniformBufferHandle,
};

use crate::generated::shader_atlas::ShaderAtlas;
use crate::generated::shader_atlas::toon_link_modern::*;

#[derive(Facet, Clone)]
pub struct ModernEditState {
    pub ramp: ModernRamp,
    pub diagnostic: ModernDiagnostic,
    pub band_center: Slider,
    pub band_softness: Slider,
    pub lut_ambient: Slider,
    pub shadow: RGBPicker,
    pub lit: RGBPicker,
    pub secondary: Checkbox,
    pub secondary_tint: RGBPicker,
    pub secondary_intensity: Slider,
    /// Radians in model space; rotates continuously with Link.
    pub secondary_azimuth: Slider,
    /// Radians above the model XZ plane.
    pub secondary_elevation: Slider,
}

impl Default for ModernEditState {
    fn default() -> Self {
        Self {
            ramp: ModernRamp::Analytic,
            diagnostic: ModernDiagnostic::Final,
            band_center: Slider::new(0.304, 0.0, 1.0),
            band_softness: Slider::new(0.05, 0.0, 0.5),
            lut_ambient: Slider::new(50.0 / 255.0, 0.0, 1.0),
            shadow: RGBPicker::from_vec3(Vec3::new(156.0, 140.0, 134.0) / 255.0),
            lit: RGBPicker::from_vec3(Vec3::ONE),
            secondary: Checkbox::new(false),
            secondary_tint: RGBPicker::from_vec3(Vec3::new(1.0, 1.0, 100.0 / 255.0)),
            secondary_intensity: Slider::new(0.25, 0.0, 1.0),
            secondary_azimuth: Slider::new(0.0, -std::f32::consts::PI, std::f32::consts::PI),
            secondary_elevation: Slider::new(-0.35, -1.57, 1.57),
        }
    }
}

fn linear_rgb(srgb: Vec3) -> Vec3 {
    let decode = |channel: f32| {
        let low_channel = channel <= 0.04045;

        if low_channel {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };

    Vec3::new(decode(srgb.x), decode(srgb.y), decode(srgb.z))
}

fn direction(azimuth: f32, elevation: f32) -> Vec3 {
    Vec3::new(
        azimuth.sin() * elevation.cos(),
        elevation.sin(),
        azimuth.cos() * elevation.cos(),
    )
}

impl ModernEditState {
    pub(crate) fn secondary_parameters(&self, spin: f32) -> (Vec3, Vec3, f32) {
        let intensity = self.secondary_intensity.value;
        let inactive = !self.secondary.checked || intensity <= 0.0;
        if inactive {
            return (Vec3::Z, Vec3::ZERO, 0.0);
        }

        let light_direction = Mat4::from_rotation_y(spin).transform_vector3(direction(
            self.secondary_azimuth.value,
            self.secondary_elevation.value,
        ));

        (
            light_direction,
            linear_rgb(self.secondary_tint.to_vec3()),
            intensity,
        )
    }

    fn params_data(&self, spin: f32) -> ModernParamsData {
        let (secondary_direction, secondary_color, secondary_intensity) =
            self.secondary_parameters(spin);

        ModernParamsData {
            // Camera matrices and the live ramp handle are supplied by draw.
            mvp: MVPMatrices {
                model: Mat4::IDENTITY,
                view: Mat4::IDENTITY,
                proj: Mat4::IDENTITY,
            },
            main_direction: direction(0.6, 0.7).extend(0.0),
            shadow_color: linear_rgb(self.shadow.to_vec3()).extend(0.0),
            lit_color: linear_rgb(self.lit.to_vec3()).extend(0.0),
            secondary_direction: secondary_direction.extend(0.0),
            secondary_color: secondary_color.extend(0.0),
            controls: glam::Vec4::new(
                self.band_center.value,
                self.band_softness.value,
                self.lut_ambient.value,
                secondary_intensity,
            ),
            ramp: self.ramp,
            diagnostic: self.diagnostic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Opaque,
    Bangs,
    Mask(u8),
    Composite(u8),
    Erase,
}

fn lighting_mix(material_name: &str, role: Role) -> f32 {
    if matches!(role, Role::Composite(_)) {
        1.0
    } else if matches!(material_name, "face" | "mouth") {
        // The mouth texture includes skin that must match the surrounding face.
        0.1
    } else {
        0.0
    }
}

fn role(material: &MaterialEntry) -> anyhow::Result<Role> {
    let fail = || {
        anyhow::anyhow!(
            "Modern material {:?}: unsupported or ambiguous role/raster state",
            material.name
        )
    };
    if material.pe_mode == mm::PixelEngineMode::Opaque {
        if !material.z_test
            || !material.z_write
            || material.z_func != mm::CompareType::LessEqual
            || material
                .blend
                .as_ref()
                .is_some_and(|b| b.mode != mm::BlendMode::None)
        {
            return Err(fail());
        }
        return Ok(if material.name == "ear(2)" {
            Role::Bangs
        } else {
            Role::Opaque
        });
    }
    if material.pe_mode != mm::PixelEngineMode::Translucent || material.z_write {
        return Err(fail());
    }
    let blend = material.blend.as_ref();
    for (index, name) in ["eyeL", "eyeR", "mayuL", "mayuR"].iter().enumerate() {
        let reference = index as u8 + 1;
        if material.name == format!("{name}damA")
            && material.z_test
            && material.z_func == mm::CompareType::LessEqual
            && blend.is_some_and(|b| {
                b.mode == mm::BlendMode::Blend
                    && b.src == mm::BlendFactor::SourceAlpha
                    && b.dst == mm::BlendFactor::InverseSourceAlpha
            })
        {
            return Ok(Role::Mask(reference));
        }
        if material.name == *name
            && !material.z_test
            && blend.is_some_and(|b| {
                b.mode == mm::BlendMode::Blend
                    && b.src == mm::BlendFactor::DestinationAlpha
                    && b.dst == mm::BlendFactor::InverseDestinationAlpha
            })
        {
            return Ok(Role::Composite(reference));
        }
        // Disabled blending takes precedence over ignored stored factors.
        if material.name == format!("{name}damB")
            && !material.z_test
            && blend.is_none_or(|b| b.mode == mm::BlendMode::None)
        {
            return Ok(Role::Erase);
        }
    }
    Err(fail())
}

fn required_texture(
    manifest: &Manifest,
    material: &MaterialEntry,
    slot: usize,
) -> anyhow::Result<usize> {
    let index = material
        .texmaps
        .get(slot)
        .copied()
        .flatten()
        .with_context(|| {
            format!(
                "Modern material {:?}: required texture slot {slot} is missing",
                material.name
            )
        })? as usize;
    anyhow::ensure!(
        index < manifest.textures.len(),
        "Modern material {:?}: texture slot {slot} references resource {index}, but only {} exist",
        material.name,
        manifest.textures.len()
    );
    Ok(index)
}

fn pupil_offset(material: &MaterialEntry) -> anyhow::Result<Vec2> {
    let error = || {
        anyhow::anyhow!(
            "Modern material {:?}: unsupported required pupil UV transform",
            material.name
        )
    };
    let texgen = material.texgens.get(1).ok_or_else(error)?;
    if texgen.ty != 1 || texgen.src != 4 {
        return Err(error());
    }
    if texgen.matrix == 60 {
        return Ok(Vec2::ZERO);
    }
    if texgen.matrix < 30 || !(texgen.matrix - 30).is_multiple_of(3) {
        return Err(error());
    }
    let slot = (texgen.matrix - 30) / 3;
    let matrices: Vec<_> = material
        .tex_matrices
        .iter()
        .filter(|m| m.slot == slot)
        .collect();
    let [matrix] = matrices.as_slice() else {
        return Err(error());
    };
    if matrix.scale != [1.0, 1.0]
        || matrix.rotation != 0
        || matrix.effect_matrix != Mat4::IDENTITY.to_cols_array()
        || !matrix
            .center
            .iter()
            .chain(matrix.translation.iter())
            .all(|v| v.is_finite())
    {
        return Err(error());
    }
    Ok(Vec2::from_array(matrix.translation))
}

const FEATURE_POSITION_TOLERANCE: f32 = 2.0e-5;

fn feature_vertices_match(mask: &crate::ModelVertex, composite: &crate::ModelVertex) -> bool {
    mask.uv0 == composite.uv0
        && mask
            .position
            .abs_diff_eq(composite.position, FEATURE_POSITION_TOLERANCE)
}

struct PreparedMaterial {
    role: Role,
    albedo: usize,
    pupil: Option<usize>,
    pupil_offset: Vec2,
}
struct Prepared {
    materials: Vec<PreparedMaterial>,
    ramp: usize,
    order: Vec<usize>,
}

fn prepare(manifest: &Manifest) -> anyhow::Result<Prepared> {
    let ramps: Vec<_> = manifest
        .textures
        .iter()
        .enumerate()
        .filter(|(_, t)| t.runtime_substitution.as_deref() == Some("toonex"))
        .map(|(i, _)| i)
        .collect();
    let [ramp] = ramps.as_slice() else {
        anyhow::bail!(
            "Modern resource toonex: expected exactly one runtime substitution, found {}",
            ramps.len()
        );
    };
    let mut materials = Vec::new();
    for material in &manifest.materials {
        let role = role(material)?;
        for index in material.texmaps.iter().flatten() {
            anyhow::ensure!(
                (*index as usize) < manifest.textures.len(),
                "Modern material {:?}: invalid texture resource {index}",
                material.name
            );
        }
        anyhow::ensure!(
            material.texmaps.iter().skip(2).all(Option::is_none),
            "Modern material {:?}: unsupported texture slot >= 2",
            material.name
        );
        let albedo = required_texture(manifest, material, 0)?;
        anyhow::ensure!(
            albedo != *ramp,
            "Modern material {:?}: toonex data cannot be an albedo",
            material.name
        );
        let base = material
            .texgens
            .first()
            .with_context(|| format!("Modern material {:?}: missing primary UV0", material.name))?;
        anyhow::ensure!(
            base.ty == 1 && base.src == 4 && base.matrix == 60,
            "Modern material {:?}: unsupported primary UV0 transform",
            material.name
        );
        let has_pupil = matches!(role, Role::Composite(1 | 2));
        let pupil = if has_pupil {
            let pupil = required_texture(manifest, material, 1)?;
            anyhow::ensure!(
                pupil != *ramp && manifest.textures[pupil].name == "hitomi",
                "Modern material {:?}: required pupil resource must be hitomi, found {:?}",
                material.name,
                manifest.textures[pupil].name
            );
            Some(pupil)
        } else {
            None
        };
        match role {
            Role::Opaque | Role::Bangs => anyhow::ensure!(
                required_texture(manifest, material, 1)? == *ramp,
                "Modern material {:?}: required slot 1 must reference toonex",
                material.name
            ),
            Role::Mask(_) | Role::Erase | Role::Composite(3 | 4) => anyhow::ensure!(
                material.texmaps.get(1).copied().flatten().is_none(),
                "Modern material {:?}: unexpected secondary texture",
                material.name
            ),
            _ => {}
        }
        materials.push(PreparedMaterial {
            role,
            albedo,
            pupil,
            pupil_offset: if has_pupil {
                pupil_offset(material)?
            } else {
                Vec2::ZERO
            },
        });
    }
    let mut order = Vec::new();
    for (index, batch) in manifest.batches.iter().enumerate() {
        anyhow::ensure!(
            (batch.material as usize) < materials.len(),
            "Modern batch {index}: invalid material {}",
            batch.material
        );
    }
    let batches_for = |role| {
        manifest
            .batches
            .iter()
            .enumerate()
            .filter(|(_, b)| materials[b.material as usize].role == role)
            .map(|(i, _)| i)
            .collect::<Vec<_>>()
    };
    anyhow::ensure!(
        batches_for(Role::Bangs).len() == 1,
        "Modern material ear(2): expected exactly one bangs batch"
    );
    anyhow::ensure!(
        manifest
            .batches
            .iter()
            .filter(|b| manifest.materials[b.material as usize].name == "face")
            .count()
            == 1,
        "Modern material face: expected exactly one occluding face batch"
    );
    order.extend(batches_for(Role::Opaque));
    for reference in 1..=4 {
        let masks = batches_for(Role::Mask(reference));
        let composites = batches_for(Role::Composite(reference));
        anyhow::ensure!(
            masks.len() == 1 && composites.len() == 1,
            "Modern feature {reference}: expected one paired mask/composite batch"
        );
        let mask = &manifest.batches[masks[0]];
        let composite = &manifest.batches[composites[0]];
        anyhow::ensure!(
            mask.index_count == composite.index_count
                && materials[mask.material as usize].albedo
                    == materials[composite.material as usize].albedo,
            "Modern feature {reference}: mask/composite coverage mismatch"
        );
        order.extend(masks);
    }
    order.extend(batches_for(Role::Bangs));
    for reference in 1..=4 {
        order.extend(batches_for(Role::Composite(reference)));
    }
    Ok(Prepared {
        materials,
        ramp: *ramp,
        order,
    })
}

fn prepare_geometry(
    manifest: &Manifest,
    vertices: &[crate::ModelVertex],
    indices: &[u32],
) -> anyhow::Result<Prepared> {
    let prepared = prepare(manifest)?;
    crate::validate_manifest(manifest, vertices, indices)?;
    // Validate duplicated geometry before any GPU upload.
    for reference in 1..=4 {
        let find_batch = |role| {
            manifest
                .batches
                .iter()
                .find(|batch| prepared.materials[batch.material as usize].role == role)
                .unwrap()
        };
        let mask = find_batch(Role::Mask(reference));
        let composite = find_batch(Role::Composite(reference));
        let range = |batch: &mm::Batch| {
            batch.first_index as usize..(batch.first_index + batch.index_count) as usize
        };
        for (&mask_index, &composite_index) in
            indices[range(mask)].iter().zip(&indices[range(composite)])
        {
            anyhow::ensure!(
                feature_vertices_match(
                    &vertices[mask_index as usize],
                    &vertices[composite_index as usize]
                ),
                "Modern feature {reference}: mask/composite coverage differs beyond position tolerance {FEATURE_POSITION_TOLERANCE} or has different UV0"
            );
        }
    }

    Ok(prepared)
}

fn modern_texture_options(
    entry: &mm::TextureEntry,
    data: bool,
) -> anyhow::Result<mltrs::renderer::TextureOptions> {
    let mut options = crate::texture_options(entry)?;
    options.color_space = if data {
        TextureColorSpace::Unorm
    } else {
        TextureColorSpace::Srgb
    };
    Ok(options)
}

type ModernPipeline = PipelineHandle<DrawIndexedIndirect, PushBlock<ModernMultiDraw>>;

pub struct ToonLinkModern {
    start_time: Instant,
    host_spin: Option<f32>,
    pub edit_state: ModernEditState,
    pipelines: Vec<ModernPipeline>,
    params_buffer: UniformBufferHandle<ModernParams>,
    args_buffer: ImmutableBufferHandle<DrawIndexedIndirectCommand>,
    draws: SingletonBufferHandle<ModernIndividualDraw>,
    pipeline_order: Vec<usize>,
    ramp: TextureHandle,
}

impl ToonLinkModern {
    pub fn set_spin(&mut self, spin: f32) {
        self.host_spin = Some(spin);
    }
    pub fn edit_state(&self) -> &ModernEditState {
        &self.edit_state
    }
    pub fn edit_state_mut(&mut self) -> &mut ModernEditState {
        &mut self.edit_state
    }
}

impl Game for ToonLinkModern {
    type EditState = ModernEditState;
    type Atlas = ShaderAtlas;
    fn window_title() -> &'static str {
        "Toon Link Modern"
    }
    fn needs_stencil() -> bool {
        true
    }
    fn editor_ui(&mut self) -> Option<(&str, &mut Self::EditState)> {
        Some(("Modern", &mut self.edit_state))
    }
    fn setup(renderer: &mut Renderer, shaders: ShaderAtlas) -> anyhow::Result<Self> {
        let dir = crate::converted_dir();
        let manifest = crate::load_manifest(&dir)?;
        let vertices = crate::load_vertices(
            &dir.join(&manifest.buffers.vertices),
            manifest.buffers.vertex_count,
        )?;
        let indices = crate::load_indices(
            &dir.join(&manifest.buffers.indices),
            manifest.buffers.index_count,
        )?;
        let prepared = prepare_geometry(&manifest, &vertices, &indices)?;
        let vertices: Vec<_> = vertices
            .iter()
            .map(|v| ModernVertex {
                position: v.position,
                normal: v.normal,
                uv0: v.uv0,
            })
            .collect();
        let mesh = renderer.create_mesh(&vertices, &indices)?;
        let mut textures = Vec::new();
        for (i, entry) in manifest.textures.iter().enumerate() {
            let needed = i == prepared.ramp
                || prepared
                    .materials
                    .iter()
                    .any(|m| m.albedo == i || m.pupil == Some(i));
            if !needed {
                textures.push(None);
                continue;
            }
            let image = image::ImageReader::open(dir.join(&entry.file))
                .with_context(|| {
                    format!("Modern texture {:?}: opening {}", entry.name, entry.file)
                })?
                .decode()
                .with_context(|| {
                    format!("Modern texture {:?}: decoding {}", entry.name, entry.file)
                })?
                .to_rgba8();
            textures.push(Some(renderer.create_texture_with_options(
                format!("modern_{}", entry.name),
                RgbaPixels::new(image.width(), image.height(), &image)?,
                modern_texture_options(entry, i == prepared.ramp)?,
            )?));
        }
        let params_buffer = renderer.create_uniform_buffer::<ModernParams>()?;
        let stencil = renderer
            .stencil_support()
            .context("Modern requires host Game::needs_stencil() = true")?;
        let mut pipelines = Vec::new();
        let mut gpu_materials = Vec::new();
        for (material, prepared) in manifest.materials.iter().zip(&prepared.materials) {
            let cull = match material.cull {
                mm::CullMode::Back => CullMode::Back,
                mm::CullMode::Front => CullMode::Front,
                mm::CullMode::None => CullMode::None,
                mm::CullMode::All => {
                    anyhow::bail!("Modern material {:?}: unsupported Cull_All", material.name)
                }
            };
            let (blend, depth_test, depth_write, color_write, stencil_mode) = match prepared.role {
                Role::Mask(reference) => (
                    BlendMode::Opaque,
                    DepthCompare::LessEqual,
                    false,
                    [false; 4],
                    stencil.write(reference),
                ),
                Role::Composite(reference) => (
                    BlendMode::Alpha,
                    DepthCompare::Always,
                    false,
                    [true, true, true, false],
                    stencil.test_equal(reference),
                ),
                _ => (
                    BlendMode::Opaque,
                    DepthCompare::LessEqual,
                    true,
                    [true, true, true, false],
                    StencilMode::DISABLED,
                ),
            };
            pipelines.push(
                renderer.create_pipeline(
                    shaders
                        .toon_link_modern
                        .pipeline_config(Resources {
                            params_buffer: &params_buffer,
                        })
                        .with_shared_mesh(&mesh)
                        .with_raster_state(RasterState {
                            blend,
                            cull,
                            depth_test,
                            depth_write,
                            color_write,
                            stencil: stencil_mode,
                        })
                        .indirect(),
                )?,
            );
            let albedo = textures[prepared.albedo]
                .as_ref()
                .unwrap()
                .bindless_handle();
            gpu_materials.push(ModernMaterial {
                albedo,
                pupil: textures[prepared.pupil.unwrap_or(prepared.albedo)]
                    .as_ref()
                    .unwrap()
                    .bindless_handle(),
                pupil_offset: prepared.pupil_offset,
                has_pupil: u32::from(prepared.pupil.is_some()),
                lighting_mix: lighting_mix(&material.name, prepared.role),
            });
        }
        let materials_buffer = renderer.create_singleton_buffer(&gpu_materials)?;
        let mut commands = Vec::new();
        let mut draws = Vec::new();
        let mut pipeline_order = Vec::new();
        for index in prepared.order {
            let batch = &manifest.batches[index];
            commands.push(DrawIndexedIndirectCommand {
                index_count: batch.index_count,
                instance_count: 1,
                first_index: batch.first_index,
                vertex_offset: 0,
                first_instance: 0,
            });
            draws.push(ModernIndividualDraw {
                material: renderer.singleton_addr_at(&materials_buffer, batch.material as u32),
            });
            pipeline_order.push(batch.material as usize);
        }
        let args_buffer = renderer.create_indirect_buffer(&commands)?;
        let draws = renderer.create_singleton_buffer(&draws)?;
        Ok(Self {
            start_time: Instant::now(),
            host_spin: None,
            edit_state: ModernEditState::default(),
            pipelines,
            params_buffer,
            args_buffer,
            draws,
            pipeline_order,
            ramp: textures[prepared.ramp].take().unwrap(),
        })
    }
    fn draw(&mut self, mut renderer: FrameRenderer) -> Result<(), DrawError> {
        let spin = self
            .host_spin
            .unwrap_or_else(|| self.start_time.elapsed().as_secs_f32() * crate::MODEL_SPIN);
        let model = Mat4::from_rotation_y(spin) * Mat4::from_scale(Vec3::splat(crate::MODEL_SCALE));
        let target = Vec3::new(0.0, 0.62, 0.0);
        let view = glam::camera::rh::view::look_at_mat4(
            target + Vec3::new(0.0, 0.25, 2.8),
            target,
            Vec3::Y,
        );
        let proj = glam::camera::rh::proj::directx::perspective(
            45f32.to_radians(),
            renderer.aspect_ratio(),
            0.1,
            20.0,
        );
        let data = self.edit_state.params_data(spin);
        let params = ModernParams {
            mvp: MVPMatrices { model, view, proj },
            main_direction: data.main_direction,
            secondary_direction: data.secondary_direction,
            shadow_color: data.shadow_color,
            lit_color: data.lit_color,
            secondary_color: data.secondary_color,
            controls: data.controls,
            ramp_texture: self.ramp.bindless_handle(),
            ramp: data.ramp,
            diagnostic: data.diagnostic,
        };
        for (index, &pipeline) in self.pipeline_order.iter().enumerate() {
            let first = index as u32;
            renderer.queue_draw_indexed_indirect_with_push_constants(
                &self.pipelines[pipeline],
                &self.args_buffer,
                first,
                1,
                &ModernMultiDraw {
                    individual_draws: renderer.singleton_addr_at(&self.draws, first),
                },
            );
        }
        renderer.submit_draws(|gpu| gpu.write_uniform(&mut self.params_buffer, params))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_defaults_and_secondary_off_zero_parameter_policy() {
        let mut state = ModernEditState::default();
        assert_eq!(state.ramp, ModernRamp::Analytic);
        assert_eq!(state.diagnostic, ModernDiagnostic::Final);
        assert_eq!(state.band_softness.value, 0.05);
        assert_eq!(lighting_mix("sleeve", Role::Opaque), 0.0);
        assert_eq!(
            lighting_mix("face", Role::Opaque),
            lighting_mix("mouth", Role::Opaque),
        );
        assert_eq!(lighting_mix("eyeL", Role::Composite(1)), 1.0);
        assert_eq!(lighting_mix("mayuR", Role::Composite(4)), 1.0);
        assert_eq!(lighting_mix("eyeLdamA", Role::Mask(1)), 0.0);
        let disabled = state.secondary_parameters(1.0);
        state.secondary.checked = true;
        state.secondary_intensity.value = 0.0;
        state.secondary_azimuth.value = 2.0;
        state.secondary_elevation.value = 1.0;
        assert_eq!(disabled, state.secondary_parameters(3.0));
        state.secondary_intensity.value = 1.0;
        assert!(state.secondary_parameters(3.0).0.is_normalized());
    }

    #[test]
    fn modern_rgb_conversion_boundaries() {
        assert_eq!(linear_rgb(Vec3::ZERO), Vec3::ZERO);
        assert_eq!(linear_rgb(Vec3::ONE), Vec3::ONE);
        assert!((linear_rgb(Vec3::splat(0.5)).x - 0.21404114).abs() < 1e-6);
    }

    #[test]
    fn packed_cpu_parameters_route_exact_values() {
        let mut state = ModernEditState {
            shadow: RGBPicker::from_vec3(Vec3::new(0.0, 0.5, 1.0)),
            lit: RGBPicker::from_vec3(Vec3::new(1.0, 0.0, 0.5)),
            secondary_tint: RGBPicker::from_vec3(Vec3::new(0.5, 1.0, 0.0)),
            secondary_elevation: Slider::new(0.0, -1.57, 1.57),
            ..Default::default()
        };
        // RGBPicker stores authored sRGB bytes, so from_vec3(0.5) rounds to 128.
        let half_linear = ((128.0_f32 / 255.0 + 0.055) / 1.055).powf(2.4);
        for ramp in [ModernRamp::Analytic, ModernRamp::Texture] {
            state.ramp = ramp;
            for diagnostic in [
                ModernDiagnostic::Final,
                ModernDiagnostic::WorldNormals,
                ModernDiagnostic::Uv0,
                ModernDiagnostic::NDotL,
                ModernDiagnostic::BandOnly,
                ModernDiagnostic::AlbedoOnly,
            ] {
                state.diagnostic = diagnostic;
                for (center, softness, ambient) in
                    [(0.0, 0.0, 0.0), (1.0, 0.5, 1.0), (0.23, 0.07, 0.81)]
                {
                    state.band_center.value = center;
                    state.band_softness.value = softness;
                    state.lut_ambient.value = ambient;
                    for (enabled, intensity) in [(false, 0.37), (true, 0.37), (true, 0.0)] {
                        state.secondary.checked = enabled;
                        state.secondary_intensity.value = intensity;
                        let data = state.params_data(std::f32::consts::FRAC_PI_2);
                        let active = enabled && intensity > 0.0;
                        assert_eq!(data.ramp, ramp);
                        assert_eq!(data.diagnostic, diagnostic);
                        assert_eq!(
                            data.controls,
                            glam::Vec4::new(
                                center,
                                softness,
                                ambient,
                                if active { intensity } else { 0.0 }
                            )
                        );
                        assert!(
                            data.shadow_color
                                .abs_diff_eq(Vec3::new(0.0, half_linear, 1.0).extend(0.0), 1e-6)
                        );
                        assert!(
                            data.lit_color
                                .abs_diff_eq(Vec3::new(1.0, 0.0, half_linear).extend(0.0), 1e-6)
                        );
                        let expected_direction = if active { Vec3::X } else { Vec3::Z };
                        let expected_color = if active {
                            Vec3::new(half_linear, 1.0, 0.0)
                        } else {
                            Vec3::ZERO
                        };
                        assert!(
                            data.secondary_direction
                                .abs_diff_eq(expected_direction.extend(0.0), 1e-6)
                        );
                        assert!(
                            data.secondary_color
                                .abs_diff_eq(expected_color.extend(0.0), 1e-6)
                        );
                    }
                }
            }
        }
    }

    fn material() -> MaterialEntry {
        MaterialEntry {
            name: "eyeL".into(),
            record: 0,
            pe_mode: mm::PixelEngineMode::Translucent,
            cull: mm::CullMode::Back,
            z_test: false,
            z_func: mm::CompareType::LessEqual,
            z_write: false,
            z_compare_early: false,
            blend: Some(mm::BlendState {
                mode: mm::BlendMode::Blend,
                src: mm::BlendFactor::DestinationAlpha,
                dst: mm::BlendFactor::InverseDestinationAlpha,
                logic: mm::LogicOp::Copy,
            }),
            alpha_compare: None,
            dither: false,
            num_tev_stages: 0,
            num_tex_gens: 2,
            num_color_chans: 0,
            texmaps: vec![Some(0), Some(1)],
            tev: mm::TevConfig {
                stages: vec![],
                orders: vec![],
                konst_colors: vec![],
                reg_colors: vec![],
                kcsels: vec![],
                kasels: vec![],
                swap_modes: vec![],
                swap_tables: vec![],
            },
            texgens: vec![
                mm::TexGenState {
                    ty: 1,
                    src: 4,
                    matrix: 60,
                },
                mm::TexGenState {
                    ty: 1,
                    src: 4,
                    matrix: 33,
                },
            ],
            tex_matrices: vec![mm::TexMatrixState {
                slot: 1,
                center: [0.5; 3],
                scale: [1.0; 2],
                rotation: 0,
                translation: [-0.05, 0.0],
                effect_matrix: Mat4::IDENTITY.to_cols_array(),
            }],
            channels: vec![],
            material_colors: vec![],
            ambient_colors: vec![],
            light_colors: vec![],
        }
    }

    #[test]
    fn modern_roles_accept_supported_and_reject_ambiguous() {
        let mut material = material();
        assert_eq!(role(&material).unwrap(), Role::Composite(1));
        material.name = "unknown-eye".into();
        assert!(
            role(&material)
                .unwrap_err()
                .to_string()
                .contains("unknown-eye")
        );
        material.name = "eyeLdamB".into();
        material.blend.as_mut().unwrap().mode = mm::BlendMode::None;
        assert_eq!(role(&material).unwrap(), Role::Erase);
        material.z_write = true;
        assert!(role(&material).is_err());
    }

    fn fixture() -> Manifest {
        let texture = |name: &str, ramp: bool| mm::TextureEntry {
            name: name.into(),
            file: format!("{name}.png"),
            wrap_u: mm::WrapMode::Clamp,
            wrap_v: mm::WrapMode::Clamp,
            filter: mm::FilterMode::Linear,
            runtime_substitution: ramp.then(|| "toonex".into()),
        };
        let mut materials = Vec::new();
        for name in ["face", "ear(2)"] {
            let mut m = material();
            m.name = name.into();
            m.pe_mode = mm::PixelEngineMode::Opaque;
            m.z_test = true;
            m.z_write = true;
            m.blend.as_mut().unwrap().mode = mm::BlendMode::None;
            m.texmaps[1] = Some(2);
            materials.push(m);
        }
        for name in ["eyeL", "eyeR", "mayuL", "mayuR"] {
            let mut m = material();
            m.name = name.into();
            if name.starts_with("mayu") {
                m.texmaps[1] = None;
            }
            materials.push(m.clone());
            m.name = format!("{name}damA");
            m.z_test = true;
            m.texmaps[1] = None;
            let blend = m.blend.as_mut().unwrap();
            blend.src = mm::BlendFactor::SourceAlpha;
            blend.dst = mm::BlendFactor::InverseSourceAlpha;
            materials.push(m.clone());
            m.name = format!("{name}damB");
            m.z_test = false;
            m.blend.as_mut().unwrap().mode = mm::BlendMode::None;
            materials.push(m);
        }
        let batches = (0..materials.len())
            .rev()
            .enumerate()
            .map(|(i, material)| mm::Batch {
                material: material as u16,
                shape: 0,
                first_index: i as u32 * 3,
                index_count: 3,
            })
            .collect();
        Manifest {
            version: 1,
            buffers: mm::Buffers {
                vertices: String::new(),
                indices: String::new(),
                skinning: String::new(),
                vertex_layout: vec![],
                vertex_count: 3,
                index_count: materials.len() as u32 * 3,
            },
            textures: vec![
                texture("base", false),
                texture("hitomi", false),
                texture("ramp", true),
            ],
            materials,
            batches,
            skeleton: mm::Skeleton { joints: vec![] },
        }
    }

    #[test]
    fn batch_material_mapping_preserved() {
        let manifest = fixture();
        let prepared = prepare(&manifest).unwrap();
        let slots: Vec<_> = prepared
            .order
            .iter()
            .map(|&i| manifest.batches[i].material)
            .collect();
        assert_eq!(slots, [0, 3, 6, 9, 12, 1, 2, 5, 8, 11]);
        assert_eq!(prepared.order.len(), manifest.batches.len() - 4);
    }

    #[test]
    fn required_references_fail_contextually() {
        let mut manifest = fixture();
        manifest.materials[2].texmaps[1] = Some(99);
        assert!(
            prepare(&manifest)
                .err()
                .unwrap()
                .to_string()
                .contains("eyeL")
        );
        manifest.materials[2].texmaps[1] = None;
        assert!(
            prepare(&manifest)
                .err()
                .unwrap()
                .to_string()
                .contains("slot 1")
        );
        manifest.materials[2].texmaps[1] = Some(1);
        manifest.batches[0].material = 99;
        assert!(
            prepare(&manifest)
                .err()
                .unwrap()
                .to_string()
                .contains("batch 0")
        );
    }

    #[test]
    fn prepare_rejects_unsupported_manifest_contracts() {
        type Mutation = fn(&mut Manifest);
        let cases: &[(&str, Mutation, &str)] = &[
            (
                "missing toonex",
                |manifest| manifest.textures[2].runtime_substitution = None,
                "expected exactly one",
            ),
            (
                "duplicate toonex",
                |manifest| manifest.textures[1].runtime_substitution = Some("toonex".into()),
                "expected exactly one",
            ),
            (
                "toonex albedo",
                |manifest| manifest.materials[0].texmaps[0] = Some(2),
                "cannot be an albedo",
            ),
            (
                "third slot",
                |manifest| manifest.materials[0].texmaps.push(Some(0)),
                "slot >= 2",
            ),
            (
                "missing primary UV",
                |manifest| manifest.materials[0].texgens.clear(),
                "missing primary UV0",
            ),
            (
                "primary type",
                |manifest| manifest.materials[0].texgens[0].ty = 0,
                "primary UV0 transform",
            ),
            (
                "primary source",
                |manifest| manifest.materials[0].texgens[0].src = 19,
                "primary UV0 transform",
            ),
            (
                "primary matrix",
                |manifest| manifest.materials[0].texgens[0].matrix = 30,
                "primary UV0 transform",
            ),
            (
                "wrong pupil",
                |manifest| manifest.textures[1].name = "other".into(),
                "must be hitomi",
            ),
            (
                "opaque wrong ramp",
                |manifest| manifest.materials[0].texmaps[1] = Some(1),
                "must reference toonex",
            ),
            (
                "mask secondary",
                |manifest| manifest.materials[3].texmaps[1] = Some(1),
                "unexpected secondary",
            ),
            (
                "erase secondary",
                |manifest| manifest.materials[4].texmaps[1] = Some(1),
                "unexpected secondary",
            ),
            (
                "brow secondary",
                |manifest| manifest.materials[8].texmaps[1] = Some(1),
                "unexpected secondary",
            ),
            (
                "pair albedo",
                |manifest| manifest.materials[3].texmaps[0] = Some(1),
                "coverage mismatch",
            ),
            (
                "opaque depth test",
                |manifest| manifest.materials[0].z_test = false,
                "role/raster",
            ),
            (
                "opaque depth write",
                |manifest| manifest.materials[0].z_write = false,
                "role/raster",
            ),
            (
                "opaque depth function",
                |manifest| manifest.materials[0].z_func = mm::CompareType::Always,
                "role/raster",
            ),
            (
                "opaque blend",
                |manifest| {
                    manifest.materials[0].blend.as_mut().unwrap().mode = mm::BlendMode::Blend
                },
                "role/raster",
            ),
            (
                "translucent depth write",
                |manifest| manifest.materials[2].z_write = true,
                "role/raster",
            ),
            (
                "composite depth test",
                |manifest| manifest.materials[2].z_test = true,
                "role/raster",
            ),
            (
                "composite blend",
                |manifest| {
                    manifest.materials[2].blend.as_mut().unwrap().src = mm::BlendFactor::SourceAlpha
                },
                "role/raster",
            ),
            (
                "mask depth test",
                |manifest| manifest.materials[3].z_test = false,
                "role/raster",
            ),
            (
                "mask depth function",
                |manifest| manifest.materials[3].z_func = mm::CompareType::Always,
                "role/raster",
            ),
            (
                "mask blend",
                |manifest| manifest.materials[3].blend = None,
                "role/raster",
            ),
            (
                "erase depth test",
                |manifest| manifest.materials[4].z_test = true,
                "role/raster",
            ),
            (
                "erase blend",
                |manifest| {
                    manifest.materials[4].blend.as_mut().unwrap().mode = mm::BlendMode::Blend
                },
                "role/raster",
            ),
            (
                "missing pupil texgen",
                |manifest| {
                    manifest.materials[2].texgens.pop();
                },
                "pupil UV transform",
            ),
            (
                "missing pupil matrix",
                |manifest| manifest.materials[2].tex_matrices.clear(),
                "pupil UV transform",
            ),
            (
                "duplicate pupil matrix",
                |manifest| {
                    let matrix = manifest.materials[2].tex_matrices[0].clone();
                    manifest.materials[2].tex_matrices.push(matrix);
                },
                "pupil UV transform",
            ),
            (
                "pupil selector below range",
                |manifest| manifest.materials[2].texgens[1].matrix = 29,
                "pupil UV transform",
            ),
            (
                "pupil selector stride",
                |manifest| manifest.materials[2].texgens[1].matrix = 31,
                "pupil UV transform",
            ),
            (
                "pupil rotation",
                |manifest| manifest.materials[2].tex_matrices[0].rotation = 1,
                "pupil UV transform",
            ),
            (
                "pupil effect",
                |manifest| manifest.materials[2].tex_matrices[0].effect_matrix[0] = 2.0,
                "pupil UV transform",
            ),
            (
                "pupil nonfinite center",
                |manifest| manifest.materials[2].tex_matrices[0].center[0] = f32::NAN,
                "pupil UV transform",
            ),
            (
                "pupil nonfinite translation",
                |manifest| manifest.materials[2].tex_matrices[0].translation[1] = f32::INFINITY,
                "pupil UV transform",
            ),
        ];
        for &(name, mutate, expected) in cases {
            let mut manifest = fixture();
            mutate(&mut manifest);
            let error = prepare(&manifest)
                .err()
                .unwrap_or_else(|| panic!("accepted {name}"));
            assert!(error.to_string().contains(expected), "{name}: {error:#}");
        }

        for (material_index, expected) in [
            (0, "face"),
            (1, "bangs"),
            (2, "paired"),
            (3, "paired"),
            (5, "paired"),
            (6, "paired"),
            (8, "paired"),
            (9, "paired"),
            (11, "paired"),
            (12, "paired"),
        ] {
            for duplicate in [false, true] {
                let mut manifest = fixture();
                let batch_index = manifest
                    .batches
                    .iter()
                    .position(|batch| batch.material == material_index)
                    .unwrap();
                if duplicate {
                    manifest.batches.push(manifest.batches[batch_index].clone());
                } else {
                    manifest.batches.remove(batch_index);
                }
                let error = prepare(&manifest).err().unwrap_or_else(|| {
                    panic!("accepted material {material_index}, duplicate={duplicate}")
                });
                assert!(error.to_string().contains(expected), "{error:#}");
            }
        }
    }

    #[test]
    fn modern_texture_roles_preserve_classic_policy() {
        let manifest = fixture();
        for entry in &manifest.textures {
            assert_eq!(
                crate::texture_options(entry).unwrap().color_space,
                TextureColorSpace::Unorm
            );
            assert_eq!(
                modern_texture_options(entry, false).unwrap().color_space,
                TextureColorSpace::Srgb
            );
            assert_eq!(
                modern_texture_options(entry, true).unwrap().color_space,
                TextureColorSpace::Unorm
            );
        }
    }

    #[test]
    fn feature_coverage_tolerates_converter_position_noise_but_not_uv_or_shape_drift() {
        let base = crate::ModelVertex {
            position: Vec3::new(1.0, 2.0, 3.0),
            normal: Vec3::Y,
            uv0: Vec2::new(0.25, 0.75),
        };
        let mut other = crate::ModelVertex {
            position: base.position + Vec3::splat(FEATURE_POSITION_TOLERANCE * 0.5),
            normal: -Vec3::Y,
            uv0: base.uv0,
        };
        assert!(feature_vertices_match(&base, &other));
        other.position.x += FEATURE_POSITION_TOLERANCE;
        assert!(!feature_vertices_match(&base, &other));
        other.position = base.position;
        other.uv0.x += f32::EPSILON;
        assert!(!feature_vertices_match(&base, &other));
    }

    #[test]
    fn pre_upload_preparation_rejects_unequal_pair_lengths() {
        for material_name in ["eyeLdamA", "eyeL"] {
            let mut manifest = fixture();
            let batch = manifest
                .batches
                .iter_mut()
                .find(|batch| manifest.materials[batch.material as usize].name == material_name)
                .unwrap();
            batch.index_count = 6;
            let mut next_first_index = 0;
            for batch in &mut manifest.batches {
                batch.first_index = next_first_index;
                next_first_index += batch.index_count;
            }
            manifest.buffers.index_count = next_first_index;
            let vertices = vec![
                crate::ModelVertex {
                    position: Vec3::ZERO,
                    normal: Vec3::Y,
                    uv0: Vec2::ZERO,
                };
                3
            ];
            let indices = vec![0; next_first_index as usize];

            let error = prepare_geometry(&manifest, &vertices, &indices)
                .err()
                .unwrap_or_else(|| panic!("accepted unequal pair length for {material_name}"));
            assert!(error.to_string().contains("Modern feature 1"));
            assert!(error.to_string().contains("coverage mismatch"));
        }
    }

    #[test]
    fn pre_upload_preparation_checks_paired_geometry() {
        let manifest = fixture();
        let base = crate::ModelVertex {
            position: Vec3::ONE,
            normal: Vec3::Y,
            uv0: Vec2::splat(0.5),
        };
        let mut vertices = vec![base; 3];
        let mut indices = vec![0; manifest.buffers.index_count as usize];
        let mask = manifest
            .batches
            .iter()
            .find(|batch| batch.material == 3)
            .unwrap();
        indices[mask.first_index as usize] = 1;
        assert!(prepare_geometry(&manifest, &vertices, &indices).is_ok());
        vertices[1].position.x += FEATURE_POSITION_TOLERANCE * 0.5;
        assert!(prepare_geometry(&manifest, &vertices, &indices).is_ok());
        for uv_drift in [false, true] {
            vertices[1] = base;
            if uv_drift {
                vertices[1].uv0.x += f32::EPSILON;
            } else {
                vertices[1].position.x += FEATURE_POSITION_TOLERANCE * 2.0;
            }
            let error = prepare_geometry(&manifest, &vertices, &indices)
                .err()
                .expect("accepted coverage drift");
            assert!(error.to_string().contains("Modern feature 1"), "{error:#}");
        }
    }

    #[test]
    fn pupil_transform_and_slot_validation() {
        let mut material = material();
        assert_eq!(pupil_offset(&material).unwrap(), Vec2::new(-0.05, 0.0));
        material.tex_matrices[0].scale[0] = 2.0;
        assert!(
            pupil_offset(&material)
                .unwrap_err()
                .to_string()
                .contains("eyeL")
        );
        material.texgens[1].matrix = 60;
        assert_eq!(pupil_offset(&material).unwrap(), Vec2::ZERO);
        material.texgens[1].src = 19;
        assert!(pupil_offset(&material).is_err());
    }
}
