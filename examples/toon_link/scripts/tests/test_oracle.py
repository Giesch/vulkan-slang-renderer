# Oracle synthetic edge cases: the independent oracle must parse fixtures
# exercising the corners gclib cannot (non-identity remaps, post BTK tracks,
# nonstandard tangent words, zero-key descriptors) and must agree with gclib
# wherever gclib has coverage. Errors must raise, not misparse.

from __future__ import annotations

import sys
import unittest
from pathlib import Path

TESTS_DIR = Path(__file__).resolve().parent
SCRIPTS_DIR = TESTS_DIR.parent
sys.path.insert(0, str(SCRIPTS_DIR))
sys.path.insert(0, str(TESTS_DIR))

import gen_fixtures as fx  # noqa: E402
import link_animation_oracle as oracle  # noqa: E402


def axis(s, r, t) -> dict:
    return {"s": s, "r": r, "t": t}


class OracleSyntheticEdgeCases(unittest.TestCase):
    """Check synthetic edge cases in the independent animation oracle."""
    def test_zero_one_and_multi_keys(self):
        # axis x: all default; axis y: constants; axis z: keyed S (shared
        # tangent) and keyed R (split tangent). Pools sized exactly.
        joints = [
            [
                axis((0, 0, 0), (0, 0, 0), (0, 0, 0)),
                axis((1, 0, 0), (1, 0, 0), (1, 0, 0)),
                axis((2, 0, 0), (2, 0, 1), (0, 0, 0)),
            ]
        ]
        scale_pool = [0.0, 1.0, 0.5, 2.0, 3.0, 0.25, 7.0]
        # keyed z-rotation reads words 0..8 (2 keys x stride 4); y's constant
        # gets its own slot at the end of the pool.
        rotation_pool = [0, 16383, 1, 2, 5, -16383, 3, 4, -32768]
        joints[0][1]["r"] = (1, 8, 0)
        joints[0][1]["s"] = (1, 6, 0)
        translation_pool = [5.5]
        chunk = fx.bck_chunk(
            joints,
            scale_pool=scale_pool,
            rotation_pool=rotation_pool,
            translation_pool=translation_pool,
        )
        data = fx.j3d_file(b"bck1", chunk)
        out = oracle.dump_bck(data, "t.bck")
        self.assertIn("axis x scale=default rotation=default translation=default", out)
        self.assertIn(
            "axis y scale=constant=f32:0x40E00000 rotation=constant=-32768 translation=constant=f32:0x40B00000",
            out,
        )
        self.assertIn(
            "axis z scale=keyed[ty=0] (f32:0x00000000,f32:0x3F800000,f32:0x3F000000,f32:0x3F000000);(f32:0x40000000,f32:0x40400000,f32:0x3E800000,f32:0x3E800000)",
            out,
        )
        self.assertIn("rotation=keyed[ty=1] (0,16383,1,2);(5,-16383,3,4)", out)

    def test_nonstandard_tangent_word_preserved(self):
        # tangent type 7 (beyond gclib's 0/1 enum): J3D reads any nonzero as
        # split; the oracle must not reject it.
        joints = [[axis((2, 0, 7), (0, 0, 0), (0, 0, 0))] * 3]
        chunk = fx.bck_chunk(joints, scale_pool=[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
        out = oracle.dump_bck(fx.j3d_file(b"bck1", chunk), "t.bck")
        self.assertIn("scale=keyed[ty=7]", out)

    def test_negative_rotation_and_units(self):
        joints = [[axis((0, 0, 0), (1, 0, 0), (0, 0, 0))] * 3]
        chunk = fx.bck_chunk(joints, rotation_pool=[-32768])
        out = oracle.dump_bck(fx.j3d_file(b"bck1", chunk), "t.bck")
        self.assertIn("rotation=constant=-32768", out)

    def test_btp_non_identity_remap_duplicates_and_sample_count(self):
        rows = [
            {"material": "mouth", "remap": 14, "texno": 0, "samples": [27, 7, 7]},
            {"material": "mouth", "remap": 99, "texno": 1, "samples": [4]},
            {"material": "eyeL", "remap": 1, "texno": 0, "samples": [0, 1, 2, 3, 4]},
        ]
        data = fx.j3d_file(b"btp1", fx.btp_chunk(rows, duration=10))
        out = oracle.dump_btp(data, "t.btp")
        self.assertIn("target 0 material=mouth remap=14 texno=0 samples=3:27,7,7", out)
        self.assertIn("target 1 material=mouth remap=99 texno=1 samples=1:4", out)
        self.assertIn("target 2 material=eyeL remap=1 texno=0 samples=5:0,1,2,3,4", out)

    def _btk_target(self, **overrides) -> dict:
        t = dict(
            material="eyeL",
            remap=0,
            texgen=0,
            center=[0.0, 0.0, 0.0],
            axes=[axis((0, 0, 0), (0, 0, 0), (0, 0, 0))] * 3,
            scale_pool=[],
            rotation_pool=[],
            translation_pool=[],
        )
        t.update(overrides)
        return t

    def test_btk_post_tracks_and_matrix_flag(self):
        post = self._btk_target(material="eyeR", texgen=1)
        data = fx.j3d_file(b"btk1", fx.btk_chunk([self._btk_target()], post_targets=[post], matrix_calc=1))
        out = oracle.dump_btk(data, "t.btk")
        self.assertIn("matrix_calc=1", out)
        self.assertIn("post targets=1", out)
        self.assertIn("material=eyeR", out.split("post targets=1", 1)[1])

    def test_btk_non_identity_remap(self):
        t = self._btk_target(remap=37)
        data = fx.j3d_file(b"btk1", fx.btk_chunk([t]))
        out = oracle.dump_btk(data, "t.btk")
        self.assertIn("remap=37", out)

    def test_bas_absent_present_truncated(self):
        chunk = fx.bck_chunk([[fx.default_axis()] * 3])
        absent = oracle.dump_bck(fx.j3d_file(b"bck1", chunk), "t.bck")
        self.assertIn("bas=absent", absent)
        present = oracle.dump_bck(fx.bck_with_bas(chunk, entries=2), "t.bck")
        self.assertIn("bas=present off=", present)
        self.assertIn("len=72", present)
        with self.assertRaises(oracle.OracleError):
            oracle.dump_bck(fx.bck_with_bas(chunk, entries=2, truncate=True), "t.bck")

    def test_loop_attribute_and_rotation_shift(self):
        chunk = fx.bck_chunk([[fx.default_axis()] * 3], loop=4, rotation_shift=3)
        out = oracle.dump_bck(fx.j3d_file(b"bck1", chunk), "t.bck")
        self.assertIn("duration=6 loop=4 rotation_shift=3", out)

    def test_wrong_format_dispatch_errors(self):
        btp = fx.j3d_file(b"btp1", fx.btp_chunk([{"material": "m", "remap": 0, "texno": 0, "samples": [1]}]))
        with self.assertRaises(oracle.OracleError):
            oracle.dump_bck(btp, "mislabeled.bck")
        bck = fx.j3d_file(b"bck1", fx.bck_chunk([[fx.default_axis()]]))
        with self.assertRaises(oracle.OracleError):
            oracle.dump_btp(bck, "mislabeled.btp")

    def test_malformed_structures_error(self):
        # keyed track with non-increasing times
        joints = [[axis((2, 0, 0), (0, 0, 0), (0, 0, 0))] * 3]
        chunk = fx.bck_chunk(joints, scale_pool=[5.0, 1.0, 0.0, 1.0, 1.0, 0.0])
        with self.assertRaises(oracle.OracleError):
            oracle.dump_bck(fx.j3d_file(b"bck1", chunk), "t.bck")
        # pool overrun
        joints = [[axis((1, 99, 0), (0, 0, 0), (0, 0, 0))] * 3]
        chunk = fx.bck_chunk(joints, scale_pool=[1.0])
        with self.assertRaises(oracle.OracleError):
            oracle.dump_bck(fx.j3d_file(b"bck1", chunk), "t.bck")


class GclibAgreement(unittest.TestCase):
    """Where gclib has coverage, the oracle must agree with it; where gclib
    raises (its known limits), the oracle stands alone."""

    def test_bck_header_and_bas_agreement(self):
        chunk = fx.bck_chunk(
            [[fx.default_axis()] * 3 for _ in range(3)],
            loop=2,
            rotation_shift=1,
            duration=7,
            scale_pool=[1.0],
            rotation_pool=[-2],
            translation_pool=[3.5],
        )
        data = fx.j3d_file(b"bck1", chunk)
        oracle.cross_check(data, "bck", "t.bck")  # must not raise
        with_bas = fx.bck_with_bas(chunk, entries=1)
        oracle.cross_check(with_bas, "bck", "t.bck")

    def test_btp_header_agreement(self):
        rows = [{"material": "mouth", "remap": 3, "texno": 0, "samples": [1, 2]}]
        data = fx.j3d_file(b"btp1", fx.btp_chunk(rows, duration=4))
        oracle.cross_check(data, "btp", "t.btp")

    def test_btk_identity_agreement_and_gap(self):
        keyed = axis((2, 0, 1), (1, 0, 0), (2, 0, 0))
        t = dict(
            material="eyeL",
            remap=0,  # identity: gclib can parse
            texgen=0,
            center=[0.25, 0.5, 0.75],
            axes=[keyed, keyed, keyed],
            scale_pool=[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            rotation_pool=[0x4000],
            translation_pool=[0.0, 1.0, 9.9, 2.0, 3.0, 4.0],
        )
        data = fx.j3d_file(b"btk1", fx.btk_chunk([t]))
        oracle.cross_check(data, "btk", "t.btk")  # deep agreement incl. tracks

        # Non-identity remap: gclib's own assert fires on load, so the
        # cross-check skips gclib (documented limitation) and the struct
        # walk stands alone for these clips.
        t2 = dict(t, remap=9)
        data2 = fx.j3d_file(b"btk1", fx.btk_chunk([t2]))
        dump = oracle.dump_btk(data2, "t2.btk")
        self.assertIn("remap=9", dump)
        oracle.cross_check(data2, "btk", "t2.btk")  # returns without raising


class RustOracleDifferential(unittest.TestCase):
    """Edge fixtures that real clips (and gclib) cannot reach, compared
    byte-for-byte between the Rust converter's canonical dump and the
    oracle's: nonstandard tangent words, non-identity BTK remaps, post
    track sets, duplicate BTP materials with sample-count != duration.
    Requires CONVERT_LINK_ANIMATIONS (the recipe builds the binary first).
    """

    def _run_differential(self, members: dict[str, bytes]) -> None:
        import hashlib
        import json
        import os
        import subprocess
        import tempfile

        converter = os.environ.get("CONVERT_LINK_ANIMATIONS")
        if not converter:
            self.fail("CONVERT_LINK_ANIMATIONS is unset; the recipe must build the converter first")
        with tempfile.TemporaryDirectory() as tmp:
            raw = Path(tmp) / "raw"
            entries = []
            for rel, data in sorted(members.items()):
                archive, _, member = rel.partition("/")
                path = raw / archive / member
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
                fmt = {"bck": "bck", "btp": "btp", "btk": "btk"}[member.rsplit(".", 1)[1]]
                entries.append(
                    {
                        "archive": archive,
                        "member": member,
                        "entry_index": len(entries),
                        "resource_id": len(entries),
                        "format": fmt,
                        "size": len(data),
                        "sha256": hashlib.sha256(data).hexdigest(),
                    }
                )
            (raw / "inventory.json").write_text(
                json.dumps({"version": 1, "disc": "GZLE01", "entries": entries})
            )
            rust = subprocess.run(
                [converter, str(raw), str(Path(tmp) / "out"), "--dump-canonical"],
                capture_output=True,
                text=True,
            )
            self.assertEqual(rust.returncode, 0, rust.stderr)
            self.assertEqual(rust.stdout, oracle.dump_all(raw))

    def test_nonstandard_tangent_word(self) -> None:
        joints = [[axis((2, 0, 7), (0, 0, 0), (0, 0, 0))] * 3]
        chunk = fx.bck_chunk(joints, scale_pool=[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0])
        self._run_differential({"LkAnm/bcks/tangent7.bck": fx.j3d_file(b"bck1", chunk)})

    def test_btk_non_identity_remap_and_post_set(self) -> None:
        keyed = axis((2, 0, 1), (1, 0, 0), (2, 0, 0))
        main = dict(
            material="eyeL",
            remap=37,  # non-identity: gclib refuses, both walks must agree
            texgen=2,
            center=[0.5, -0.5, 0.25],
            axes=[keyed, keyed, keyed],
            scale_pool=[0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            rotation_pool=[0x4000],
            translation_pool=[0.0, 1.0, 9.9, 2.0, 3.0, 4.0],
        )
        post = dict(
            material="eyeR",
            remap=11,
            texgen=1,
            center=[0.0, 0.0, 0.0],
            axes=[axis((0, 0, 5), (0, 0, 0), (0, 0, 0))] * 3,
            scale_pool=[],
            rotation_pool=[],
            translation_pool=[],
        )
        data = fx.j3d_file(b"btk1", fx.btk_chunk([main], post_targets=[post], matrix_calc=1))
        self._run_differential({"LkD01/btk/remap_post.btk": data})

    def test_btp_duplicates_and_sample_count(self) -> None:
        rows = [
            {"material": "mouth", "remap": 14, "texno": 0, "samples": [27, 7, 7]},
            {"material": "mouth", "remap": 99, "texno": 1, "samples": [4]},
            {"material": "eyeL", "remap": 1, "texno": 0, "samples": [0, 1, 2, 3, 4]},
        ]
        data = fx.j3d_file(b"btp1", fx.btp_chunk(rows, duration=10))
        self._run_differential({"LkAnm/btp/dupes.btp": data})


class FixtureSelfTest(unittest.TestCase):
    def test_yaz0_roundtrip(self) -> None:
        payload = bytes(range(256)) * 3 + b"tail"
        self.assertEqual(fx.yaz0_roundtrip(payload), payload)

    def test_synthetic_cl_bdl_parses(self) -> None:
        from gclib.j3d import BDL
        import tempfile

        with tempfile.NamedTemporaryFile(suffix=".bdl", delete=False) as tmp:
            tmp.write(fx.cl_bdl(42, ["mouth", "eyeL"]))
            path = tmp.name
        bdl = BDL(path)
        self.assertEqual(bdl.jnt1.joint_count, 42)
        self.assertEqual(list(bdl.mat3.mat_names), ["mouth", "eyeL"])

    def test_rarc_roundtrip(self) -> None:
        from gclib.rarc import RARC
        import tempfile

        members = {"bcks/a.bck": b"J3D1bytes", "btp/x.btp": b"more", "root.bin": b"toplevel"}
        with tempfile.NamedTemporaryFile(suffix=".arc", delete=False) as tmp:
            tmp.write(fx.rarc(members, compress={"bcks/a.bck"}))
            path = tmp.name
        rarc = RARC(path)
        by_dir = {}
        for e in rarc.file_entries:
            if e.is_dir or e.name in (".", ".."):
                continue
            by_dir[(e.parent_node.name, e.name)] = e.data.getvalue()
        # Uncompressed members come back raw; compressed ones stay as the
        # Yaz0 stream (the extraction reader decompresses by magic); a
        # root-level member lives directly under the root node.
        self.assertEqual(by_dir[("btp", "x.btp")], b"more")
        self.assertEqual(by_dir[("archive", "root.bin")], b"toplevel")
        self.assertEqual(by_dir[("bcks", "a.bck")], fx.yaz0_compress(b"J3D1bytes"))
        self.assertEqual(fx.yaz0_roundtrip(b"J3D1bytes"), b"J3D1bytes")


if __name__ == "__main__":
    unittest.main()
