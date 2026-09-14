use mltrs_render_graph::{
    ResourcePlanner, SampledTexBinding,
    bindless::{BindlessHandle, Sampler2D},
};
fn main() {
    let _ = ResourcePlanner::new();
    let sampled = BindlessHandle::<Sampler2D>::from_raw(1);
    let _: SampledTexBinding = sampled.into();
}
