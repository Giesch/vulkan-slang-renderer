pub mod paint_brush_compute;
pub mod paint_display;
pub mod wc_advect_and_transfer_pigment_compute;
pub mod wc_capillary_flow_compute;
pub mod wc_divergence_compute;
pub mod wc_flow_outward_compute;
pub mod wc_gaussian_blur_compute;
pub mod wc_pressure_jacobi_compute;
pub mod wc_project_velocity_compute;
pub mod wc_update_velocity_compute;

use ::mltrs::shaders::atlas::ShaderAtlasRoot;

pub struct ShaderAtlas {
    pub paint_display: paint_display::Shader,
    pub paint_brush_compute: paint_brush_compute::Shader,
    pub wc_advect_and_transfer_pigment_compute: wc_advect_and_transfer_pigment_compute::Shader,
    pub wc_capillary_flow_compute: wc_capillary_flow_compute::Shader,
    pub wc_divergence_compute: wc_divergence_compute::Shader,
    pub wc_flow_outward_compute: wc_flow_outward_compute::Shader,
    pub wc_gaussian_blur_compute: wc_gaussian_blur_compute::Shader,
    pub wc_pressure_jacobi_compute: wc_pressure_jacobi_compute::Shader,
    pub wc_project_velocity_compute: wc_project_velocity_compute::Shader,
    pub wc_update_velocity_compute: wc_update_velocity_compute::Shader,
}

impl ShaderAtlasRoot for ShaderAtlas {
    const SHADERS_SOURCE_DIR: &'static str = concat!(env!("CARGO_MANIFEST_DIR"), "/shaders/source");

    fn init() -> Self {
        Self {
            paint_display: paint_display::Shader::init(),
            paint_brush_compute: paint_brush_compute::Shader::init(),
            wc_advect_and_transfer_pigment_compute:
                wc_advect_and_transfer_pigment_compute::Shader::init(),
            wc_capillary_flow_compute: wc_capillary_flow_compute::Shader::init(),
            wc_divergence_compute: wc_divergence_compute::Shader::init(),
            wc_flow_outward_compute: wc_flow_outward_compute::Shader::init(),
            wc_gaussian_blur_compute: wc_gaussian_blur_compute::Shader::init(),
            wc_pressure_jacobi_compute: wc_pressure_jacobi_compute::Shader::init(),
            wc_project_velocity_compute: wc_project_velocity_compute::Shader::init(),
            wc_update_velocity_compute: wc_update_velocity_compute::Shader::init(),
        }
    }
}
