use mltrs_renderer::renderer::render_graph::DrawIndexedIndirectCommand;
fn main() {
    let command = DrawIndexedIndirectCommand::new(3, 2, 1, -4, 5);
    assert_eq!(command.index_count(), 3);
    assert_eq!(command.instance_count(), 2);
    assert_eq!(command.first_index(), 1);
    assert_eq!(command.vertex_offset(), -4);
    assert_eq!(command.first_instance(), 5);
}
