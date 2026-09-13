use mltrs_render_graph::commands::IndirectRequest;
fn main() {
    let _ = IndirectRequest {
        buffer: 0,
        offset: 0,
        draw_count: 1,
        element_size: 20,
        alignment: 4,
    };
}
