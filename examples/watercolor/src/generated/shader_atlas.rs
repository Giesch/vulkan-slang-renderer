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

// a convenience aggregate; consumers may instead construct the per-shader
// `Shader::init()` types directly, so an unused atlas is not a defect
#[allow(dead_code)]
pub struct ShaderAtlas {
    pub paint_display: paint_display::Shader,
    pub wc_update_velocity_compute: wc_update_velocity_compute::Shader,
    pub wc_capillary_flow_compute: wc_capillary_flow_compute::Shader,
    pub wc_flow_outward_compute: wc_flow_outward_compute::Shader,
    pub wc_advect_and_transfer_pigment_compute: wc_advect_and_transfer_pigment_compute::Shader,
    pub wc_pressure_jacobi_compute: wc_pressure_jacobi_compute::Shader,
    pub wc_project_velocity_compute: wc_project_velocity_compute::Shader,
    pub wc_gaussian_blur_compute: wc_gaussian_blur_compute::Shader,
    pub paint_brush_compute: paint_brush_compute::Shader,
    pub wc_divergence_compute: wc_divergence_compute::Shader,
}

#[allow(dead_code)]
impl ShaderAtlas {
    pub fn init() -> Self {
        Self {
            paint_display: paint_display::Shader::init(),
            wc_update_velocity_compute: wc_update_velocity_compute::Shader::init(),
            wc_capillary_flow_compute: wc_capillary_flow_compute::Shader::init(),
            wc_flow_outward_compute: wc_flow_outward_compute::Shader::init(),
            wc_advect_and_transfer_pigment_compute:
                wc_advect_and_transfer_pigment_compute::Shader::init(),
            wc_pressure_jacobi_compute: wc_pressure_jacobi_compute::Shader::init(),
            wc_project_velocity_compute: wc_project_velocity_compute::Shader::init(),
            wc_gaussian_blur_compute: wc_gaussian_blur_compute::Shader::init(),
            paint_brush_compute: paint_brush_compute::Shader::init(),
            wc_divergence_compute: wc_divergence_compute::Shader::init(),
        }
    }
}
