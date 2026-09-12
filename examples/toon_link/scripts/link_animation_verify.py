#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034"]
# ///

# Real-asset verification driver for Link animations, invoked by
# `just toon_link link-verify-animations` (never auto-discovered by cargo).
# Runs, in order:
#   1. raw tree checks: inventory present, every entry hashed, no extras
#   2. canonical parity: `convert_link_animations --dump-canonical` vs the
#      independent oracle (byte-identical), including the gclib cross-check
#   3. conversion: catalog + clip documents, exact membership + hashes vs
#      scripts/link_animation_converted.sha256
#   4. repeatability: a second conversion run is byte-identical
#   5. tamper detection on *copies*: a flipped raw byte and a deleted clip
#      must both fail verification (never by touching the real raw tree)
#   6. model-gate isolation: the unchanged model test commands pass with the
#      animation directories hidden, proving model gates never require
#      animation assets and no real-animation driver is cargo-discovered
#
# Requires the real assets (this script fails loudly when they are absent);
# a clean checkout runs the asset-free suite via `just toon_link
# link-test-animations` instead.

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
EXAMPLE_DIR = SCRIPT_DIR.parent
REPO_ROOT = EXAMPLE_DIR.parent.parent
sys.path.insert(0, str(SCRIPT_DIR))

import link_animation_oracle as oracle  # noqa: E402


class VerifyError(Exception):
    pass


def step(name: str) -> None:
    print(f"[link-verify-animations] {name}")


def sha256_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(argv: list[str], **kw) -> subprocess.CompletedProcess:
    proc = subprocess.run(argv, capture_output=True, text=True, **kw)
    if proc.returncode != 0:
        raise VerifyError(
            f"command failed ({' '.join(str(a) for a in argv[:3])}…):\n"
            f"{proc.stdout[-2000:]}\n{proc.stderr[-2000:]}"
        )
    return proc


def require(cond: bool, message: str) -> None:
    if not cond:
        raise VerifyError(message)


def real_inventory_matches_selection(raw_dir: Path) -> list[dict]:
    step("1. raw tree checks (real_inventory_matches_selection)")
    require(raw_dir.is_dir(), f"raw tree {raw_dir} is missing; run the extraction recipe first")
    inventory_path = raw_dir / "inventory.json"
    require(inventory_path.is_file(), f"inventory.json missing in {raw_dir}")
    entries = oracle.load_inventory(raw_dir)
    expected = {f"{e['archive']}/{e['member']}" for e in entries}
    actual = {
        str(p.relative_to(raw_dir))
        for p in raw_dir.rglob("*")
        if p.is_file() and p.name != "inventory.json"
    }
    require(
        actual == expected,
        "raw tree membership differs from inventory: "
        f"extra={sorted(actual - expected)[:3]} missing={sorted(expected - actual)[:3]}",
    )
    for e in entries:
        path = raw_dir / e["archive"] / e["member"]
        require(sha256_file(path) == e["sha256"], f"{e['archive']}/{e['member']}: raw hash mismatch")
    print(f"  {len(entries)} clips verified")
    return entries


def real_animation_oracle_parity(converter: Path, raw_dir: Path, workdir: Path) -> None:
    step("2. canonical parity vs independent oracle (real_animation_oracle_parity)")
    rust = run([str(converter), str(raw_dir), str(workdir / "unused-out"), "--dump-canonical"]).stdout
    (workdir / "rust-canonical.txt").write_text(rust)
    oracle_text = oracle.dump_all(raw_dir)
    (workdir / "oracle-canonical.txt").write_text(oracle_text)
    require(rust == oracle_text, "canonical dumps differ; see rust-canonical.txt / oracle-canonical.txt in the workdir")
    # gclib agreement layer on every clip (raises on any disagreement)
    for e in sorted(oracle.load_inventory(raw_dir), key=lambda e: (e["archive"], e["member"])):
        data = (raw_dir / e["archive"] / e["member"]).read_bytes()
        oracle.cross_check(data, e["format"], f"{e['archive']}/{e['member']}")
    print(f"  byte-identical; gclib agrees on all {len(oracle.load_inventory(raw_dir))} clips")


def convert_and_hash(converter: Path, raw_dir: Path, out_dir: Path) -> dict[str, str]:
    run([str(converter), str(raw_dir), str(out_dir)])
    return {str(p.relative_to(out_dir)): sha256_file(p) for p in sorted(out_dir.rglob("*.json"))}


def real_conversion_hashes_and_repeatability(converter: Path, raw_dir: Path, converted_dir: Path, golden: Path, workdir: Path) -> None:
    step("3. converted output membership + hashes (real_conversion_hashes_and_repeatability)")
    require(golden.is_file(), f"golden converted-hash manifest {golden} is missing")
    produced = convert_and_hash(converter, raw_dir, converted_dir)
    prefix = f"{converted_dir.relative_to(EXAMPLE_DIR)}/"
    lines = golden.read_text().splitlines()
    for line in lines:
        digest, _, rel = line.partition("  ")
        rel = rel.removeprefix(prefix)
        require(rel in produced, f"golden entry {rel} not produced")
        require(produced[rel] == digest, f"{rel}: produced hash differs from golden")
    require(len(lines) == len(produced), f"golden lists {len(lines)} files but {len(produced)} were produced")

    step("4. repeatability (real_conversion_hashes_and_repeatability)")
    again = convert_and_hash(converter, raw_dir, workdir / "converted-again")
    require(again == produced, "second conversion differs from the first")
    print(f"  {len(produced)} files, two runs byte-identical")


def tamper_gate(converter: Path, raw_dir: Path, workdir: Path) -> None:
    step("5. tamper/deletion detection on copies")
    import json

    copy = workdir / "tamper-raw"
    if copy.exists():
        shutil.rmtree(copy)
    shutil.copytree(raw_dir, copy)
    victim = copy / "LkAnm" / "bcks" / sorted((copy / "LkAnm" / "bcks").iterdir())[0].name
    data = bytearray(victim.read_bytes())
    data[-1] ^= 0xFF
    victim.write_bytes(bytes(data))
    proc = subprocess.run([str(converter), str(copy), str(workdir / "tamper-out")], capture_output=True, text=True)
    require(proc.returncode != 0, "tampered raw byte was accepted")

    shutil.rmtree(copy)
    shutil.copytree(raw_dir, copy)
    victim.unlink()
    proc = subprocess.run([str(converter), str(copy), str(workdir / "tamper-out")], capture_output=True, text=True)
    require(proc.returncode != 0, "missing raw clip was accepted")
    print("  tamper + deletion both rejected")


def model_gates_do_not_require_animation_assets() -> None:
    step("6. model gates do not require animation assets (model_gates_do_not_require_animation_assets)")
    anim_dir = EXAMPLE_DIR / "assets/link/animations"
    model_raw = EXAMPLE_DIR / "assets/link/raw/cl.bdl"
    require(model_raw.is_file(), "model fixtures missing; extract-link first (this gate needs them)")
    with tempfile.TemporaryDirectory(dir=EXAMPLE_DIR / "assets/link") as hidden:
        hidden_path = Path(hidden) / "animations"
        if anim_dir.exists():
            shutil.move(str(anim_dir), str(hidden_path))
        try:
            proc = subprocess.run(
                ["cargo", "test", "-p", "convert-link", "--", "--include-ignored"],
                cwd=REPO_ROOT,
                capture_output=True,
                text=True,
            )
            require(proc.returncode == 0, f"model tests failed without animations:\n{proc.stdout[-1500:]}")
            # The asset-free animation tests are cargo-discovered and must
            # pass; what must NOT exist is an *ignored* (asset-requiring)
            # real-animation test that this gate would silently pull in.
            import re

            ignored_anim = re.search(r"test (\S*animation\S*) \.\.\. ignored", proc.stdout)
            require(
                ignored_anim is None,
                f"real-animation test {ignored_anim.group(1) if ignored_anim else ''!r} is cargo-discovered",
            )
        finally:
            if hidden_path.exists():
                shutil.move(str(hidden_path), str(anim_dir))
    print("  model gate passes with animations hidden; no asset-requiring animation driver invoked")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Real-asset Link animation verification")
    parser.add_argument("--converter", type=Path, required=True, help="path to the convert_link_animations binary")
    parser.add_argument("--raw-dir", type=Path, default=EXAMPLE_DIR / "assets/link/animations/raw")
    parser.add_argument("--converted-dir", type=Path, default=EXAMPLE_DIR / "assets/link/animations/converted")
    parser.add_argument("--converted-golden", type=Path, default=SCRIPT_DIR / "link_animation_converted.sha256")
    args = parser.parse_args(argv)

    require(args.converter.is_file() and os.access(args.converter, os.X_OK), f"converter binary not found: {args.converter}")
    workdir = Path(tempfile.mkdtemp(prefix=".verify-", dir=EXAMPLE_DIR / "assets/link/animations"))
    try:
        real_inventory_matches_selection(args.raw_dir)
        real_animation_oracle_parity(args.converter, args.raw_dir, workdir)
        real_conversion_hashes_and_repeatability(args.converter, args.raw_dir, args.converted_dir, args.converted_golden, workdir)
        tamper_gate(args.converter, args.raw_dir, workdir)
        model_gates_do_not_require_animation_assets()
    except VerifyError as ex:
        print(f"link-verify-animations: FAIL: {ex}", file=sys.stderr)
        return 1
    finally:
        shutil.rmtree(workdir, ignore_errors=True)
    print("link-verify-animations: OK: all real-asset gates passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
