//! Bind-pose evaluation and CPU skinning. Consumes the parsed geometry chunks
//! (see `bmd::*`) and produces baked, model-space vertices ready for a static
//! render — no runtime skinning in v1.
//!
//! Pipeline: FK world matrices from JNT1 + INF1 parentage → per-DRW1-slot
//! skinning matrices (rigid = joint world; weighted = `Σ wᵢ·worldᵢ·invBindᵢ`)
//! → per-shape matrix-slot state machine resolving PNMTXIDX → baked vertices
//! (deduped by GX index tuple + resolved matrix) → triangle lists.
//!
//! Two numeric gates run here, both hard errors (the file is its own oracle):
//! invBind identity (`world(j)·invBind(j) = I` at bind pose) and
//! weighted-identity (EVP1-weighted vertices, stored in model space, must bake
//! back to ≈ their stored positions).

use std::collections::HashMap;

use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::bmd::BmdError;
use crate::bmd::Model;
use crate::bmd::drw1::{Drw1, DrwSlot};
use crate::bmd::evp1::{Evp1, Mtx3x4};
use crate::bmd::jnt1::{Jnt1, Joint};
use crate::bmd::shp1::VertexIndices;
use crate::bmd::vtx1::Vtx1;
use crate::gx::types::PrimitiveType;

/// Max acceptable residual for `world(j)·invBind(j) = I`. The stored inverse
/// binds are f32 and Link's joints sit up to ~30 units from the origin chained
/// ~6 deep, so the identity holds only to f32 precision at that scale; the
/// wrong rotation order fails by ~10^4, not ~10^-2, so this still catches it.
const INVBIND_EPS: f32 = 0.02;
/// Max acceptable baked-vs-stored distance for weighted vertices, in model
/// units. Tighter than INVBIND because it is a direct positional error.
const WEIGHTED_EPS: f32 = 0.05;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BakedVertex {
    pub pos: [f32; 3],
    pub nrm: [f32; 3],
    pub uv: [f32; 2],
}

pub struct BakedModel {
    pub vertices: Vec<BakedVertex>,
    /// Per vertex: up to 4 (joint, weight) influences, zero-padded. Unused in
    /// v1 (the pose is baked); emitted for future runtime skinning.
    pub skin: Vec<[(u8, f32); 4]>,
    /// One triangle-list per SHP1 shape (GX-native winding).
    pub indices_per_shape: Vec<Vec<u32>>,
    pub invbind_max_residual: f32,
    pub weighted_max_distance: f32,
}

pub fn bake(model: &Model) -> Result<BakedModel, BmdError> {
    let world = joint_world_matrices(
        &model.jnt1,
        &model.inf1.parents,
        &model.inf1.hierarchy_order,
    );

    // invBind identity: converts a silent FK/transpose bug into a loud failure
    // across all joints at once.
    let inv_bind: Vec<Mat4> = model.evp1.inv_bind.iter().map(mat4_from_rows_3x4).collect();
    let mut invbind_max_residual = 0.0f32;
    for j in 0..world.len() {
        let residual = world[j] * inv_bind[j] - Mat4::IDENTITY;
        invbind_max_residual = invbind_max_residual.max(mat4_max_abs(&residual));
    }
    if invbind_max_residual > INVBIND_EPS {
        return Err(BmdError::Invariant(format!(
            "FK invBind identity failed: max residual {invbind_max_residual:.6} > {INVBIND_EPS} \
             (rotation composition order or invBind transpose wrong)"
        )));
    }

    // One skinning + normal matrix per DRW1 slot, reused across all vertices.
    let skin_mtx: Vec<Mat4> = model
        .drw1
        .slots
        .iter()
        .map(|slot| skinning_matrix(slot, &world, &inv_bind, &model.evp1))
        .collect();
    let norm_mtx: Vec<Mat3> = skin_mtx
        .iter()
        .map(|m| Mat3::from_mat4(*m).inverse().transpose())
        .collect();

    let mut baker = Baker {
        vtx1: &model.vtx1,
        drw1: &model.drw1,
        evp1: &model.evp1,
        skin_mtx: &skin_mtx,
        norm_mtx: &norm_mtx,
        verts: Vec::new(),
        skin: Vec::new(),
        dedup: HashMap::new(),
        weighted_max: 0.0,
    };

    let mut indices_per_shape = Vec::with_capacity(model.shp1.shapes.len());
    for shape in &model.shp1.shapes {
        // 10-slot matrix table, reset per shape, persisting across groups.
        let mut slots: [Option<u16>; 10] = [None; 10];
        let mut indices = Vec::new();
        for group in &shape.groups {
            for (i, &e) in group.use_mtx.iter().enumerate() {
                if e != 0xFFFF {
                    slots[i] = Some(e);
                }
            }
            for prim in &group.primitives {
                let mut global = Vec::with_capacity(prim.verts.len());
                for vi in &prim.verts {
                    let slot_idx = vi.pnmtx_slot.unwrap_or(0) as usize;
                    let drw_slot = slots[slot_idx].ok_or_else(|| {
                        BmdError::Invariant(format!(
                            "SHP1 matrix slot {slot_idx} read before ever being set \
                             (0xFFFF inherit with no prior packet)"
                        ))
                    })?;
                    global.push(baker.emit(vi, drw_slot)?);
                }
                expand(prim.prim_type, &global, &mut indices);
            }
        }
        indices_per_shape.push(indices);
    }

    if baker.weighted_max > WEIGHTED_EPS {
        return Err(BmdError::Invariant(format!(
            "weighted-identity failed: max baked-vs-stored distance {:.6} > {WEIGHTED_EPS} \
             (envelope skinning or SHP1 matrix resolution wrong)",
            baker.weighted_max
        )));
    }

    Ok(BakedModel {
        vertices: baker.verts,
        skin: baker.skin,
        indices_per_shape,
        invbind_max_residual,
        weighted_max_distance: baker.weighted_max,
    })
}

// --- FK ---------------------------------------------------------------------

/// s16 angle → radians (0x8000 = -π; the full u16 range spans 0..2π).
fn rotation_radians(s16: [i16; 3]) -> [f32; 3] {
    const K: f32 = std::f32::consts::PI / 32768.0;
    [s16[0] as f32 * K, s16[1] as f32 * K, s16[2] as f32 * K]
}

/// Local T·R (scales are all unit in cl.bdl, asserted at parse). Rotation is
/// composed Z·Y·X (the J3D Basic/Maya convention); the invBind identity check
/// catches the wrong order.
fn joint_local(j: &Joint) -> Mat4 {
    let [rx, ry, rz] = rotation_radians(j.rotation_s16);
    let r = Mat4::from_rotation_z(rz) * Mat4::from_rotation_y(ry) * Mat4::from_rotation_x(rx);
    Mat4::from_translation(Vec3::from_array(j.translation)) * r
}

/// World matrix per joint. `order` must list joints parent-before-child
/// (INF1 hierarchy order guarantees it — joint indices are not assumed sorted).
pub fn joint_world_matrices(jnt1: &Jnt1, parents: &[Option<u16>], order: &[u16]) -> Vec<Mat4> {
    let mut world = vec![Mat4::IDENTITY; jnt1.joints.len()];
    for &j in order {
        let local = joint_local(&jnt1.joints[j as usize]);
        world[j as usize] = match parents[j as usize] {
            Some(p) => world[p as usize] * local,
            None => local,
        };
    }
    world
}

/// 3×4 row-major (translation in the 4th column) → glam column-major affine.
fn mat4_from_rows_3x4(rows: &Mtx3x4) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(rows[0][0], rows[1][0], rows[2][0], 0.0),
        Vec4::new(rows[0][1], rows[1][1], rows[2][1], 0.0),
        Vec4::new(rows[0][2], rows[1][2], rows[2][2], 0.0),
        Vec4::new(rows[0][3], rows[1][3], rows[2][3], 1.0),
    )
}

fn mat4_max_abs(m: &Mat4) -> f32 {
    m.to_cols_array()
        .iter()
        .fold(0.0f32, |acc, &v| acc.max(v.abs()))
}

fn skinning_matrix(slot: &DrwSlot, world: &[Mat4], inv_bind: &[Mat4], evp1: &Evp1) -> Mat4 {
    match slot {
        DrwSlot::Joint(j) => world[*j as usize],
        DrwSlot::Envelope(e) => {
            let mut m = Mat4::ZERO;
            for &(joint, w) in &evp1.envelopes[*e as usize] {
                m += (world[joint as usize] * inv_bind[joint as usize]) * w;
            }
            m
        }
    }
}

fn skin_entry(slot: &DrwSlot, evp1: &Evp1) -> [(u8, f32); 4] {
    let mut out = [(0u8, 0.0f32); 4];
    match slot {
        DrwSlot::Joint(j) => out[0] = (*j as u8, 1.0),
        DrwSlot::Envelope(e) => {
            for (k, &(joint, w)) in evp1.envelopes[*e as usize].iter().take(4).enumerate() {
                out[k] = (joint as u8, w);
            }
        }
    }
    out
}

// --- baking -----------------------------------------------------------------

struct Baker<'a> {
    vtx1: &'a Vtx1,
    drw1: &'a Drw1,
    evp1: &'a Evp1,
    skin_mtx: &'a [Mat4],
    norm_mtx: &'a [Mat3],
    verts: Vec<BakedVertex>,
    skin: Vec<[(u8, f32); 4]>,
    dedup: HashMap<(u16, u16, u16, u16), u32>,
    weighted_max: f32,
}

impl Baker<'_> {
    fn emit(&mut self, vi: &VertexIndices, drw_slot: u16) -> Result<u32, BmdError> {
        let key = (
            vi.pos,
            vi.nrm.unwrap_or(u16::MAX),
            vi.uv.unwrap_or(u16::MAX),
            drw_slot,
        );
        if let Some(&i) = self.dedup.get(&key) {
            return Ok(i);
        }

        let m = self.skin_mtx[drw_slot as usize];
        let stored = Vec3::from_array(self.vtx1.pos(vi.pos)?);
        let pos = m.transform_point3(stored);
        let nrm = match vi.nrm {
            Some(n) => (self.norm_mtx[drw_slot as usize] * Vec3::from_array(self.vtx1.nrm(n)?))
                .normalize_or_zero(),
            None => Vec3::ZERO,
        };
        let uv = match vi.uv {
            Some(u) => self.vtx1.uv0(u)?,
            None => [0.0, 0.0],
        };

        // weighted vertices are stored in model space → baked ≈ stored.
        if let DrwSlot::Envelope(_) = self.drw1.slots[drw_slot as usize] {
            self.weighted_max = self.weighted_max.max((pos - stored).length());
        }

        let idx = self.verts.len() as u32;
        self.verts.push(BakedVertex {
            pos: pos.to_array(),
            nrm: nrm.to_array(),
            uv,
        });
        self.skin
            .push(skin_entry(&self.drw1.slots[drw_slot as usize], self.evp1));
        self.dedup.insert(key, idx);
        Ok(idx)
    }
}

/// Strip → list (odd triangles swap the first two indices), fan → `(0,i,i+1)`,
/// plain triangles pass through. Other primitives are rejected in shp1.rs.
fn expand(prim: PrimitiveType, v: &[u32], out: &mut Vec<u32>) {
    match prim {
        PrimitiveType::TriangleStrip => {
            for i in 0..v.len().saturating_sub(2) {
                if i % 2 == 0 {
                    out.extend_from_slice(&[v[i], v[i + 1], v[i + 2]]);
                } else {
                    out.extend_from_slice(&[v[i + 1], v[i], v[i + 2]]);
                }
            }
        }
        PrimitiveType::TriangleFan => {
            for i in 1..v.len().saturating_sub(1) {
                out.extend_from_slice(&[v[0], v[i], v[i + 1]]);
            }
        }
        PrimitiveType::Triangles => {
            for c in v.chunks_exact(3) {
                out.extend_from_slice(c);
            }
        }
        _ => {} // unreachable: non-triangle prims rejected at parse
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bmd::jnt1::Joint;

    fn joint(name: &str, rot: [i16; 3], trans: [f32; 3]) -> Joint {
        Joint {
            name: name.to_string(),
            matrix_type: 0,
            no_inherit_scale: 0,
            scale: [1.0, 1.0, 1.0],
            rotation_s16: rot,
            translation: trans,
            radius: 0.0,
            bbox_min: [0.0; 3],
            bbox_max: [0.0; 3],
        }
    }

    #[test]
    fn strip_winding_even_odd() {
        let mut out = Vec::new();
        expand(PrimitiveType::TriangleStrip, &[0, 1, 2, 3, 4], &mut out);
        // even i: (0,1,2),(2,3,4); odd i: (2,1,3)
        assert_eq!(out, vec![0, 1, 2, 2, 1, 3, 2, 3, 4]);
    }

    #[test]
    fn fan_expansion() {
        let mut out = Vec::new();
        expand(PrimitiveType::TriangleFan, &[0, 1, 2, 3], &mut out);
        assert_eq!(out, vec![0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn degenerate_strip_emits_nothing() {
        let mut out = Vec::new();
        expand(PrimitiveType::TriangleStrip, &[0, 1], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn two_joint_chain_fk() {
        // root at origin, no rotation; child translated +X by 10, rotated 90°
        // about Z (0x4000). world(child) should place its local origin at
        // (10,0,0) and rotate its local axes.
        let jnt1 = Jnt1 {
            joints: vec![
                joint("root", [0, 0, 0], [0.0, 0.0, 0.0]),
                joint("child", [0, 0, 0x4000], [10.0, 0.0, 0.0]),
            ],
        };
        let parents = vec![None, Some(0)];
        let order = vec![0, 1];
        let world = joint_world_matrices(&jnt1, &parents, &order);
        let origin = world[1].transform_point3(Vec3::ZERO);
        assert!((origin - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-4);
        // local +X of the child maps to +Y after a 90° Z rotation
        let x_axis = world[1].transform_vector3(Vec3::X);
        assert!((x_axis - Vec3::Y).length() < 1e-4);
    }

    /// Loads the real cl.bdl if present (skips otherwise). Run via
    /// `just link-verify-p3`.
    fn load_real() -> Option<Vec<u8>> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/link/raw/cl.bdl");
        match std::fs::read(path) {
            Ok(d) => Some(d),
            Err(_) => {
                eprintln!("skipping: {path} not present (run `just extract-link`)");
                None
            }
        }
    }

    #[test]
    #[ignore = "requires extracted assets (just extract-link); run via just link-verify-p3"]
    fn real_geometry_probed_facts() {
        let Some(data) = load_real() else { return };
        let model = crate::bmd::parse_model(&data).unwrap();

        // INF1
        assert_eq!(model.inf1.nodes.len(), 241);
        assert_eq!(model.inf1.hierarchy_order.len(), 42);
        assert_eq!(model.inf1.draw.len(), 24);
        assert_eq!(model.inf1.vertex_count, 1591);
        // VTX1 (delta-derived counts; pos includes one 12-byte pad slot)
        assert_eq!(model.vtx1.positions.len(), 1592);
        assert_eq!(model.vtx1.normals.len(), 1506);
        assert_eq!(model.vtx1.uvs.len(), 816);
        // JNT1
        assert_eq!(model.jnt1.joints.len(), 42);
        assert!(model.jnt1.joints.iter().all(|j| j.scale == [1.0, 1.0, 1.0]));
        // EVP1 envelope histogram + inverse binds
        let mut hist = std::collections::BTreeMap::new();
        for e in &model.evp1.envelopes {
            *hist.entry(e.len()).or_insert(0) += 1;
        }
        assert_eq!(model.evp1.envelopes.len(), 120);
        assert_eq!(hist.get(&2), Some(&101));
        assert_eq!(hist.get(&3), Some(&18));
        assert_eq!(hist.get(&4), Some(&1));
        assert_eq!(model.evp1.inv_bind.len(), 42);
        // DRW1 split
        let rigid = model
            .drw1
            .slots
            .iter()
            .filter(|s| matches!(s, DrwSlot::Joint(_)))
            .count();
        assert_eq!(model.drw1.slots.len(), 270);
        assert_eq!(rigid, 30);
        // SHP1: 7 Multi + 17 Single, all triangle strips, no billboards
        use crate::gx::types::{PrimitiveType, ShapeMatrixType};
        let multi = model
            .shp1
            .shapes
            .iter()
            .filter(|s| s.mtx_type == ShapeMatrixType::Multi)
            .count();
        assert_eq!(model.shp1.shapes.len(), 24);
        assert_eq!(multi, 7);
        let prims: usize = model
            .shp1
            .shapes
            .iter()
            .flat_map(|s| &s.groups)
            .map(|g| g.primitives.len())
            .sum();
        assert_eq!(prims, 573);
        assert!(model.shp1.shapes.iter().flat_map(|s| &s.groups).all(|g| {
            g.primitives
                .iter()
                .all(|p| p.prim_type == PrimitiveType::TriangleStrip)
        }));
    }

    #[test]
    #[ignore = "requires extracted assets (just extract-link); run via just link-verify-p3"]
    fn real_bake_and_manifest() {
        let Some(data) = load_real() else { return };
        let model = crate::bmd::parse_model(&data).unwrap();
        let baked = bake(&model).unwrap();

        let tris: usize = baked.indices_per_shape.iter().map(|s| s.len() / 3).sum();
        assert_eq!(tris, 2874);
        assert!(baked.invbind_max_residual < INVBIND_EPS);
        assert!(baked.weighted_max_distance < WEIGHTED_EPS);
        assert_eq!(baked.vertices.len(), baked.skin.len());

        // Manifest round-trips through the shared serde types.
        let converted = crate::output::build(&model, &baked);
        assert_eq!(converted.indices.len(), tris * 3);
        let json = serde_json::to_string(&converted.manifest).unwrap();
        let back: vulkan_slang_renderer::model_manifest::Manifest =
            serde_json::from_str(&json).unwrap();
        assert_eq!(back.batches.len(), 24);
        assert_eq!(back.materials.len(), 24);
        assert_eq!(back.textures.len(), 41);
        assert_eq!(back.skeleton.joints.len(), 42);
        assert_eq!(back.buffers.vertex_count, baked.vertices.len() as u32);
    }

    #[test]
    fn invbind_identity_holds_for_pure_rotation() {
        // A joint that is a pure rotation; its inverse bind is the inverse
        // rotation, so world·invBind = I.
        let jnt1 = Jnt1 {
            joints: vec![joint("j", [0, 0x4000, 0], [0.0, 0.0, 0.0])],
        };
        let world = joint_world_matrices(&jnt1, &vec![None], &vec![0]);
        let inv = world[0].inverse();
        let residual = world[0] * inv - Mat4::IDENTITY;
        assert!(mat4_max_abs(&residual) < 1e-4);
    }
}
