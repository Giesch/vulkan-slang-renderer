#[path = "../support.rs"]
mod support;
use mltrs_render_graph::*;
struct ExternalNode;
impl GraphNode for ExternalNode {
    type Frame = ();
    fn lower(&self, _: &mut LowerCtx) {}
    fn plan(&self, _: &(), _: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        Ok(())
    }
}
impl CompatibleWith<support::OtherBackend> for ExternalNode {}
fn main() {}
