#[path = "../support.rs"]
mod support;
use mltrs_render_graph::*;
struct ExternalNode;
impl GraphNode for ExternalNode {
    type FrameInput = ();
    fn add_to_graph(&self, _: &mut GraphBuildCtx) {}
    fn plan_frame(&self, _: &(), _: &mut FramePlanCtx<'_>) -> anyhow::Result<()> {
        Ok(())
    }
}
impl CompatibleWith<support::OtherBackend> for ExternalNode {}
fn main() {}
