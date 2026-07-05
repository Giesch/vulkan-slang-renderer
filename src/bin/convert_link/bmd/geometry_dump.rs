//! Canonical `--dump-geometry` table: raw file data only (exact bytes → exact
//! text, no computed floats), diffed byte-for-byte against
//! `scripts/link_geometry_table.py`. This module's format IS the spec the
//! oracle reimplements independently. Discipline (same as `mat3_dump`): fixed
//! field order, no indentation, single spaces, absent values print `-`, floats
//! print the stored f32 at `{:.6}` (identical bit pattern on both sides).
//!
//! Sections in order: INF1 (header + flat node stream), VTX1 (formats +
//! element counts), JNT1 (joints verbatim), EVP1 (envelopes + weights +
//! inverse binds), DRW1 (slot table), SHP1 (per shape: matrix type, attr set,
//! per-group matrix tables + primitive summaries + display-list sizes).

use std::fmt::Write;

use crate::bmd::Model;
use crate::bmd::drw1::DrwSlot;

fn f(v: f32) -> String {
    format!("{v:.6}")
}

fn f3(v: [f32; 3]) -> String {
    format!("{},{},{}", f(v[0]), f(v[1]), f(v[2]))
}

pub fn canonical(model: &Model) -> String {
    let mut o = String::new();
    inf1(&mut o, model);
    vtx1(&mut o, model);
    jnt1(&mut o, model);
    evp1(&mut o, model);
    drw1(&mut o, model);
    shp1(&mut o, model);
    o
}

fn inf1(o: &mut String, model: &Model) {
    let i = &model.inf1;
    writeln!(
        o,
        "INF1 flags={:#06x} rule={} packets={} vertices={} nodes={}",
        i.flags,
        i.scaling_rule,
        i.packet_count,
        i.vertex_count,
        i.nodes.len()
    )
    .unwrap();
    for (n, (ty, val)) in i.nodes.iter().enumerate() {
        writeln!(o, "node {n} {ty} {val}").unwrap();
    }
}

fn vtx1(o: &mut String, model: &Model) {
    let v = &model.vtx1;
    writeln!(o, "VTX1 formats={}", v.formats.len()).unwrap();
    for fmt in &v.formats {
        writeln!(
            o,
            "vtxfmt {} count={} type={} shift={}",
            fmt.attr, fmt.comp_count, fmt.comp_type, fmt.shift
        )
        .unwrap();
    }
    writeln!(
        o,
        "vtxcount pos={} nrm={} uv0={}",
        v.positions.len(),
        v.normals.len(),
        v.uvs.len()
    )
    .unwrap();
}

fn jnt1(o: &mut String, model: &Model) {
    writeln!(o, "JNT1 joints={}", model.jnt1.joints.len()).unwrap();
    for (i, j) in model.jnt1.joints.iter().enumerate() {
        writeln!(
            o,
            "joint {i} {} type={} nis={} s={} r={},{},{} t={} radius={} min={} max={}",
            j.name,
            j.matrix_type,
            j.no_inherit_scale,
            f3(j.scale),
            j.rotation_s16[0],
            j.rotation_s16[1],
            j.rotation_s16[2],
            f3(j.translation),
            f(j.radius),
            f3(j.bbox_min),
            f3(j.bbox_max),
        )
        .unwrap();
    }
}

fn evp1(o: &mut String, model: &Model) {
    let e = &model.evp1;
    writeln!(
        o,
        "EVP1 envelopes={} invbinds={}",
        e.envelopes.len(),
        e.inv_bind.len()
    )
    .unwrap();
    for (i, env) in e.envelopes.iter().enumerate() {
        let infl = env
            .iter()
            .map(|(j, w)| format!("{j}:{}", f(*w)))
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(o, "env {i} {} {infl}", env.len()).unwrap();
    }
    for (j, m) in e.inv_bind.iter().enumerate() {
        let vals = m
            .iter()
            .flatten()
            .map(|v| f(*v))
            .collect::<Vec<_>>()
            .join(",");
        writeln!(o, "invbind {j} {vals}").unwrap();
    }
}

fn drw1(o: &mut String, model: &Model) {
    writeln!(o, "DRW1 slots={}", model.drw1.slots.len()).unwrap();
    for (i, slot) in model.drw1.slots.iter().enumerate() {
        match slot {
            DrwSlot::Joint(idx) => writeln!(o, "drw {i} JOINT {idx}").unwrap(),
            DrwSlot::Envelope(idx) => writeln!(o, "drw {i} ENV {idx}").unwrap(),
        }
    }
}

fn shp1(o: &mut String, model: &Model) {
    writeln!(o, "SHP1 shapes={}", model.shp1.shapes.len()).unwrap();
    for (s, shape) in model.shp1.shapes.iter().enumerate() {
        let attrs = shape
            .attrs
            .iter()
            .map(|(a, t)| format!("{a}/{t}"))
            .collect::<Vec<_>>()
            .join(" ");
        writeln!(
            o,
            "shape {s} type={} groups={} attrs=[{attrs}] radius={} min={} max={}",
            shape.mtx_type,
            shape.groups.len(),
            f(shape.radius),
            f3(shape.bbox_min),
            f3(shape.bbox_max),
        )
        .unwrap();
        for (g, group) in shape.groups.iter().enumerate() {
            let use_mtx = group
                .use_mtx
                .iter()
                .map(|&e| {
                    if e == 0xFFFF {
                        "-".to_string()
                    } else {
                        e.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                o,
                "group {s} {g} use_mtx=[{use_mtx}] dlsize={} prims={}",
                group.dl_size,
                group.primitives.len()
            )
            .unwrap();
            for (p, prim) in group.primitives.iter().enumerate() {
                writeln!(
                    o,
                    "prim {s} {g} {p} {} {}",
                    prim.prim_type,
                    prim.verts.len()
                )
                .unwrap();
            }
        }
    }
}
