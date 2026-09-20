use crate::generated::readback_compute::{ReadbackOutput, ReadbackTag};
use crate::renderer::gpu_read::GPURead;

fn output_bytes() -> Vec<u8> {
    let mut bytes = vec![0xab; 160];
    bytes[0..4].copy_from_slice(&42u32.to_le_bytes());
    for (offset, values) in [(16, [1.0f32, 2.0, 3.0, 4.0]), (32, [5.0, 6.0, 0.0, 0.0])] {
        for (index, value) in values.into_iter().enumerate() {
            bytes[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[40..44].copy_from_slice(&99u32.to_le_bytes());
    bytes[48..52].copy_from_slice(&7u32.to_le_bytes());
    for index in 0..16 {
        bytes[64 + index * 4..68 + index * 4].copy_from_slice(&(index as f32 + 1.0).to_le_bytes());
    }
    for index in 0..8 {
        bytes[128 + index * 4..132 + index * 4]
            .copy_from_slice(&(100u32 + index as u32).to_le_bytes());
    }

    bytes
}

#[test]
fn reflected_readback_decodes_fields_and_initializes_padding() {
    assert_eq!(ReadbackOutput::GPU_SIZE, 160);
    let output = ReadbackOutput::read_gpu(&output_bytes()).unwrap();
    assert_eq!(output.marker, 42);
    assert_eq!(output.vector, glam::vec4(1.0, 2.0, 3.0, 4.0));
    assert_eq!(output.nested.xy, glam::vec2(5.0, 6.0));
    assert_eq!(output.nested.id, 99);
    assert_eq!(output.tag, ReadbackTag::Done);
    assert_eq!(output.transform.x_axis, glam::vec4(1.0, 2.0, 3.0, 4.0));
    assert_eq!(output.transform.y_axis, glam::vec4(5.0, 6.0, 7.0, 8.0));
    assert_eq!(output.transform.w_axis, glam::vec4(13.0, 14.0, 15.0, 16.0));
    assert_eq!(output.values[0], glam::uvec4(100, 101, 102, 103));
    assert_eq!(output.values[1], glam::uvec4(104, 105, 106, 107));
    assert_eq!(output._padding_0, [0; 12]);
    assert_eq!(output._padding_1, [0; 12]);
    assert_eq!(output.nested._padding_0, [0; 4]);
}

#[test]
fn reflected_readback_rejects_wrong_lengths_and_unknown_tags() {
    let mut bytes = output_bytes();
    for length in 0..ReadbackOutput::GPU_SIZE {
        assert!(ReadbackOutput::read_gpu(&bytes[..length]).is_err());
    }
    bytes.push(0);
    assert!(ReadbackOutput::read_gpu(&bytes).is_err());
    bytes.pop();
    bytes[48..52].copy_from_slice(&999u32.to_le_bytes());
    let error = ReadbackOutput::read_gpu(&bytes).unwrap_err().to_string();
    assert!(error.contains("ReadbackTag"), "{error}");
    assert!(error.contains("999"), "{error}");
}
