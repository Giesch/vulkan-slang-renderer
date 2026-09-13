use mltrs_renderer::renderer::render_graph::DrawIndexedIndirectCommand;
fn main() {
    let _ = DrawIndexedIndirectCommand {
        index_count: 3,
        instance_count: 1,
        first_index: 0,
        vertex_offset: 0,
        first_instance: 0,
    };
}
