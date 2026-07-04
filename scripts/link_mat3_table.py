#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = [
#     "gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034",
# ]
# ///

# P2 MAT3 oracle: prints the canonical material table for a .bdl from
# gclib's parse, implementing the format spec in
# src/bin/convert_link/bmd/mat3_dump.rs (module doc comment). The verify
# recipe literally diffs this against `convert_link --dump-mat3`. Enum
# spellings are gclib's member names — the shared vocabulary both sides
# implement from the spec.

import sys

from gclib.j3d import BDL


def b(v) -> str:
    return "true" if v else "false"


def opt(v, f=str) -> str:
    return "-" if v is None else f(v)


def enum(v) -> str:
    return v.name


def c8(c) -> str:
    return f"{c.r},{c.g},{c.b},{c.a}"


def f6(v: float) -> str:
    return f"{v:.6f}"


def floats(vals) -> str:
    return ",".join(f6(v) for v in vals)


def tokens(slots, f) -> str:
    return " ".join("-" if s is None else f(s) for s in slots)


def texgen_line(g) -> str:
    return f"type={enum(g.type_)} src={enum(g.source)} mtx={enum(g.tex_gen_matrix)}"


def texmtx_line(m) -> str:
    effect = [v for row in (m.effect_matrix.r0, m.effect_matrix.r1, m.effect_matrix.r2, m.effect_matrix.r3) for v in row]
    return (
        f"proj={enum(m.projection)} mode={enum(m.map_mode)} maya={b(m.is_maya)}"
        f" center={floats(m.center.xyz)} scale={floats(m.scale.xy)} rot={m.rotation}"
        f" trans={floats(m.translation.xy)} effect={floats(effect)}"
    )


def stage_line(s) -> str:
    return (
        f"mode=0x{s.tev_mode:02x}"
        f" a={enum(s.color_in_a)} b={enum(s.color_in_b)} c={enum(s.color_in_c)} d={enum(s.color_in_d)}"
        f" op={enum(s.color_op)} bias={enum(s.color_bias)} scale={enum(s.color_scale)}"
        f" clamp={b(s.color_clamp)} reg={enum(s.color_reg_id)}"
        f" aa={enum(s.alpha_in_a)} ab={enum(s.alpha_in_b)} ac={enum(s.alpha_in_c)} ad={enum(s.alpha_in_d)}"
        f" aop={enum(s.alpha_op)} abias={enum(s.alpha_bias)} ascale={enum(s.alpha_scale)}"
        f" aclamp={b(s.alpha_clamp)} areg={enum(s.alpha_reg_id)}"
    )


def main() -> None:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} <file.bdl>", file=sys.stderr)
        sys.exit(2)
    bdl = BDL(sys.argv[1])
    m3 = bdl.mat3
    out = [f"MAT3 materials={m3.material_count}"]
    for i, (name, m) in enumerate(zip(m3.mat_names, m3.materials)):
        w = out.append
        w(f"material {i} {name}")
        w(f"pe_mode {enum(m.pixel_engine_mode)}")
        w(f"cull {opt(m.cull_mode, enum)}")
        w(f"num_color_chans {opt(m.num_color_chans)}")
        w(f"num_tex_gens {opt(m.num_tex_gens)}")
        w(f"num_tev_stages {opt(m.num_tev_stages)}")
        w(f"z_compare_loc {opt(m.z_compare, b)}")
        if m.z_mode is None:
            w("z_mode -")
        else:
            w(f"z_mode test={b(m.z_mode.depth_test)} func={enum(m.z_mode.depth_func)} write={b(m.z_mode.depth_write)}")
        w(f"dither {opt(m.dither, b)}")
        w(f"mat_colors {tokens(m.material_colors, c8)}")
        for ci, chan in enumerate(m.color_channels):
            if chan is None:
                w(f"chan {ci} -")
            else:
                w(
                    f"chan {ci} enable={b(chan.lighting_enabled)} mat_src={enum(chan.mat_color_src)}"
                    f" amb_src={enum(chan.ambient_color_src)} diffuse={enum(chan.diffuse_function)}"
                    f" attn={enum(chan.attenuation_function)} lit_mask=0x{chan.lit_mask:02x}"
                )
        w(f"amb_colors {tokens(m.ambient_colors, c8)}")
        w(f"light_colors {tokens(m.light_colors, c8)}")
        for gi, gen in enumerate(m.tex_coord_gens):
            w(f"texgen {gi} {opt(gen, texgen_line)}")
        for gi, gen in enumerate(m.post_tex_coord_gens):
            w(f"post_texgen {gi} {opt(gen, texgen_line)}")
        for mi, mtx in enumerate(m.tex_matrixes):
            w(f"texmtx {mi} {opt(mtx, texmtx_line)}")
        for mi, mtx in enumerate(m.post_tex_matrixes):
            w(f"post_texmtx {mi} {opt(mtx, texmtx_line)}")
        w(f"texture_indices {tokens(m.textures, str)}")
        w(f"konst_colors {tokens(m.tev_konst_colors, c8)}")
        w(f"kcsels {' '.join(enum(s.value) for s in m.tev_konst_color_sels)}")
        w(f"kasels {' '.join(enum(s.value) for s in m.tev_konst_alpha_sels)}")
        for oi, order in enumerate(m.tev_orders):
            if order is None:
                w(f"tev_order {oi} -")
            else:
                w(f"tev_order {oi} coord={enum(order.tex_coord_id)} map={enum(order.tex_map_id)} chan={enum(order.channel_id)}")
        w(f"tev_colors {tokens(m.tev_colors, c8)}")
        for si, stage in enumerate(m.tev_stages):
            w(f"tev_stage {si} {opt(stage, stage_line)}")
        w(f"swap_modes {tokens(m.tev_swap_modes, lambda s: f'{s.ras_sel}:{s.tex_sel}')}")
        w(f"swap_tables {tokens(m.tev_swap_mode_tables, lambda t: f'{t.r},{t.g},{t.b},{t.a}')}")
        fog = m.fog_info
        if fog is None:
            w("fog -")
        else:
            w(
                f"fog type={enum(fog.fog_type)} enable={b(fog.enable)} center={fog.center}"
                f" start={f6(fog.start_z)} end={f6(fog.end_z)} near={f6(fog.near_z)} far={f6(fog.far_z)}"
                f" color={c8(fog.color)} ranges={','.join(str(v) for v in fog.range_adjustments)}"
            )
        a = m.alpha_compare
        if a is None:
            w("alpha_comp -")
        else:
            w(f"alpha_comp comp0={enum(a.comp0)} ref0={a.ref0} op={enum(a.operation)} comp1={enum(a.comp1)} ref1={a.ref1}")
        bl = m.blend_mode
        if bl is None:
            w("blend -")
        else:
            w(f"blend mode={enum(bl.mode)} src={enum(bl.source_factor)} dst={enum(bl.destination_factor)} logic={enum(bl.logic_op)}")
        n = m.nbt_scale
        if n is None:
            w("nbt_scale -")
        else:
            w(f"nbt_scale enable={b(n.enable)} scale={floats(n.scale.xyz)}")
        ind = m.tex_indirect
        if ind is None:
            w("indirect -")
        else:
            w(f"indirect enable={b(ind.enable)} stages={ind.num_ind_tex_stages}")
    print("\n".join(out))


if __name__ == "__main__":
    main()
