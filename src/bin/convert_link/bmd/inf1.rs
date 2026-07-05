//! INF1 chunk: scene-graph / draw order. Structure (J3DModelLoader.h
//! J3DModelInfoBlock, readInformation): u16 load flags at +8 (low nibble =
//! matrix scaling rule), u32 packet count at +0x0C, u32 vertex count at +0x10,
//! u32 hierarchy offset at +0x14. The hierarchy is a stream of 4-byte nodes
//! `{u16 type, u16 value}`: FINISH=0x00, OPEN=0x01, CLOSE=0x02, JOINT=0x10,
//! MATERIAL=0x11, SHAPE=0x12. OPEN nests subsequent nodes under the enclosing
//! joint; the stream defines both joint parentage and draw order (each SHAPE
//! inherits the nearest preceding MATERIAL).

use crate::be::BeReader;
use crate::bmd::BmdError;
use crate::gx::types::{InfNodeType, MatrixScalingRule};

pub struct Inf1 {
    pub flags: u16,
    pub scaling_rule: MatrixScalingRule,
    pub packet_count: u32,
    pub vertex_count: u32,
    /// The flat node stream (for the canonical dump).
    pub nodes: Vec<(InfNodeType, u16)>,
    /// Per joint index: parent joint, or `None` for the root.
    pub parents: Vec<Option<u16>>,
    /// Joint indices in a parent-before-child order (drives FK).
    pub hierarchy_order: Vec<u16>,
    /// (material index, shape index) in INF1 draw order.
    pub draw: Vec<(u16, u16)>,
}

pub fn parse(chunk: &[u8], joint_count: usize) -> Result<Inf1, BmdError> {
    let r = BeReader::new(chunk);
    let mut h = r.at(8);
    let flags = h.u16()?;
    h.skip(2)?;
    let packet_count = h.u32()?;
    let vertex_count = h.u32()?;
    let hierarchy_off = h.u32()? as usize;

    let scaling_rule =
        MatrixScalingRule::try_from((flags & 0x0F) as u8).map_err(|source| BmdError::Gx {
            context: "INF1 scaling rule".to_string(),
            source,
        })?;

    let mut nodes = Vec::new();
    let mut parents = vec![None; joint_count];
    let mut hierarchy_order = Vec::new();
    let mut draw = Vec::new();

    // joint_stack holds the current-joint value saved at each OPEN; the parent
    // of a JOINT node is whatever the enclosing OPEN pushed.
    let mut joint_stack: Vec<Option<u16>> = Vec::new();
    let mut cur_joint: Option<u16> = None;
    let mut cur_material: Option<u16> = None;
    let mut seen_root = false;

    let mut n = r.at(hierarchy_off);
    loop {
        let ty_raw = n.u16()?;
        let value = n.u16()?;
        let ty = InfNodeType::try_from(ty_raw as u8).map_err(|source| BmdError::Gx {
            context: "INF1 hierarchy node".to_string(),
            source,
        })?;
        nodes.push((ty, value));
        match ty {
            InfNodeType::Finish => break,
            InfNodeType::OpenChild => joint_stack.push(cur_joint),
            InfNodeType::CloseChild => {
                cur_joint = joint_stack.pop().ok_or_else(|| {
                    BmdError::Invariant("INF1 CLOSE with no matching OPEN".to_string())
                })?;
            }
            InfNodeType::Joint => {
                if value as usize >= joint_count {
                    return Err(BmdError::Invariant(format!(
                        "INF1 JOINT index {value} of {joint_count}"
                    )));
                }
                let parent = joint_stack.last().copied().flatten();
                if parent.is_none() {
                    if seen_root {
                        return Err(BmdError::Invariant(
                            "INF1 has more than one root joint".to_string(),
                        ));
                    }
                    seen_root = true;
                }
                parents[value as usize] = parent;
                hierarchy_order.push(value);
                cur_joint = Some(value);
            }
            InfNodeType::Material => cur_material = Some(value),
            InfNodeType::Shape => {
                let material = cur_material.ok_or_else(|| {
                    BmdError::Invariant("INF1 SHAPE with no preceding MATERIAL".to_string())
                })?;
                draw.push((material, value));
            }
        }
    }

    if !joint_stack.is_empty() {
        return Err(BmdError::Invariant(
            "INF1 unbalanced OPEN/CLOSE (stack not empty at FINISH)".to_string(),
        ));
    }
    if hierarchy_order.len() != joint_count {
        return Err(BmdError::Invariant(format!(
            "INF1 visited {} joints, expected {joint_count}",
            hierarchy_order.len()
        )));
    }

    Ok(Inf1 {
        flags,
        scaling_rule,
        packet_count,
        vertex_count,
        nodes,
        parents,
        hierarchy_order,
        draw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an INF1 chunk from a node list `(type, value)`.
    fn synth(nodes: &[(u16, u16)]) -> Vec<u8> {
        let hier_off = 0x18usize;
        let mut d = vec![0u8; hier_off];
        d[8..10].copy_from_slice(&2u16.to_be_bytes()); // flags: MAYA
        d[0x0C..0x10].copy_from_slice(&1u32.to_be_bytes()); // packet count
        d[0x10..0x14].copy_from_slice(&1591u32.to_be_bytes()); // vertex count
        d[0x14..0x18].copy_from_slice(&(hier_off as u32).to_be_bytes());
        for (ty, val) in nodes {
            d.extend_from_slice(&ty.to_be_bytes());
            d.extend_from_slice(&val.to_be_bytes());
        }
        d
    }

    // Types: JOINT=0x10, MATERIAL=0x11, SHAPE=0x12, OPEN=0x01, CLOSE=0x02, END=0
    #[test]
    fn builds_parents_and_draw_order() {
        // root(0) { child1(1) { grandchild(2) } child2(3) }, with a material
        // and shape under the root.
        let nodes = [
            (0x10, 0), // JOINT root
            (0x11, 5), // MATERIAL 5
            (0x12, 9), // SHAPE 9 -> (5,9)
            (0x01, 0), // OPEN
            (0x10, 1), // JOINT child1 (parent 0)
            (0x01, 0), // OPEN
            (0x10, 2), // JOINT grandchild (parent 1)
            (0x02, 0), // CLOSE
            (0x10, 3), // JOINT child2 (parent 0)
            (0x02, 0), // CLOSE
            (0x00, 0), // END
        ];
        let inf1 = parse(&synth(&nodes), 4).unwrap();
        assert_eq!(inf1.scaling_rule, MatrixScalingRule::Maya);
        assert_eq!(inf1.vertex_count, 1591);
        assert_eq!(inf1.parents[0], None);
        assert_eq!(inf1.parents[1], Some(0));
        assert_eq!(inf1.parents[2], Some(1));
        assert_eq!(inf1.parents[3], Some(0));
        assert_eq!(inf1.hierarchy_order, vec![0, 1, 2, 3]);
        assert_eq!(inf1.draw, vec![(5, 9)]);
    }

    #[test]
    fn unbalanced_open_is_error() {
        let nodes = [(0x10, 0), (0x01, 0), (0x00, 0)]; // OPEN without CLOSE
        assert!(matches!(
            parse(&synth(&nodes), 1),
            Err(BmdError::Invariant(_))
        ));
    }

    #[test]
    fn two_roots_is_error() {
        let nodes = [(0x10, 0), (0x10, 1), (0x00, 0)];
        assert!(matches!(
            parse(&synth(&nodes), 2),
            Err(BmdError::Invariant(_))
        ));
    }
}
