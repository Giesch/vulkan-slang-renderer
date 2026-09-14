#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034"]
# ///

# Deterministic extraction of Toon/Child Link body (BCK) and face (BTP/BTK)
# animations from the GZLE01 Wind Waker disc. Companion to extract_link.sh;
# docs/link_animations.md is the maintained reference.
#
# Scope is frozen by two tracked golden files next to this script:
#   link_animation_selection.json  -- the reviewed inventory (identity +
#                                     classification/reason/size/hash per member)
#   link_animation_assets.sha256   -- golden hashes over the raw output tree
# Normal runs must reproduce both exactly; they never rewrite them.
# `--bootstrap` produces *candidate* manifests under the ignored
# assets/link/animations/candidate/ tree only, for human review before
# promotion (copy into scripts/ + commit) -- see the docs.

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from io import BytesIO
from pathlib import Path
from typing import Any

SCRIPT_DIR = Path(__file__).resolve().parent
EXAMPLE_DIR = SCRIPT_DIR.parent
ANIM_DIR = EXAMPLE_DIR / "assets/link/animations"

DEFAULT_DISC_REL = "orig/GZLE01/Legend of Zelda, The - The Wind Waker (USA, Canada).ciso"
DEFAULT_DTK_REL = "build/tools/dtk"

SELECTION_GOLDEN = SCRIPT_DIR / "link_animation_selection.json"
HASHES_GOLDEN = SCRIPT_DIR / "link_animation_assets.sha256"

SELECTION_VERSION = 1
INVENTORY_VERSION = 1

# The operator-approved archive allowlist. Other archives (Link.arc itself,
# prop archives, other regions) are out of scope; adding one requires a new
# reviewed manifest.
ARCHIVES = ("LkAnm", "LkD00", "LkD01")

# Member directories that hold candidate clips, by format. Everything else in
# the archives (LkAnm's bpk/brk/dat, directory entries, stray files) is not a
# candidate and is not extracted.
CANDIDATE_DIRS = {
    "bcks": "bck",
    "btp": "btp",
    "btk": "btk",
}

# Formats and their J3D1 file-type + chunk tags, for structural validation.
FORMAT_TAGS = {
    "bck": ("bck1", b"ANK1"),
    "btp": ("btp1", b"TPT1"),
    "btk": ("btk1", b"TTK1"),
}


class ExtractError(Exception):
    """Fatal extraction failure; the message is printed to stderr."""


# --- gclib imports (lazy so tests can import this module without gclib) ------

def _gclib_modules():
    try:
        from gclib import j3d as gclib_j3d
        from gclib import yaz0_yay0
        from gclib import rarc as gclib_rarc
    except ImportError as ex:  # pragma: no cover - depends on environment
        raise ExtractError(
            "gclib is not importable; run this script via `uv run` (it is a "
            "PEP-723 script with the pinned gclib dependency)"
        ) from ex
    return gclib_rarc, gclib_j3d, yaz0_yay0


# --- data model ---------------------------------------------------------------


@dataclass(frozen=True)
class Member:
    """One file member of one archive, with its full RARC identity."""

    archive: str
    path: str  # RARC-relative, e.g. "bcks/actiontaktrdw.bck"
    entry_index: int  # index into the flat RARC file-entry list (incl. dirs)
    resource_id: int  # the RARC per-file resource ID
    data: bytes  # decompressed content

    @property
    def format(self) -> str:
        return CANDIDATE_DIRS[self.path.split("/", 1)[0]]

    @property
    def output_rel(self) -> str:
        """Path relative to the raw tree root, never flattened across archives."""
        return f"{self.archive}/{self.path}"


@dataclass
class Candidate:
    member: Member
    classification: str  # "included" | "excluded"
    reason: str

    def identity(self) -> dict[str, Any]:
        m = self.member
        return {
            "archive": m.archive,
            "member": m.path,
            "entry_index": m.entry_index,
            "resource_id": m.resource_id,
            "format": m.format,
            "size": len(m.data),
            "sha256": sha256_hex(m.data),
            "classification": self.classification,
            "reason": self.reason,
        }


@dataclass
class Report:
    included: list[Candidate] = field(default_factory=list)
    excluded: list[Candidate] = field(default_factory=list)
    skipped_dirs: dict[str, int] = field(default_factory=dict)

    def to_json(self, cl_meta: dict[str, Any]) -> dict[str, Any]:
        entries = [c.identity() for c in self.included + self.excluded]
        entries.sort(key=lambda e: (e["archive"], e["member"]))
        return {
            "cl_model": cl_meta,
            "skipped_directories": dict(sorted(self.skipped_dirs.items())),
            "entries": entries,
        }

    def sorted_included(self) -> list[Candidate]:
        return sorted(self.included, key=lambda c: (c.member.archive, c.member.path))


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


# --- disc / dtk ---------------------------------------------------------------


def resolve_paths(disc: str | None, dtk: str | None) -> dict[str, Path]:
    """Resolve the checkout from TWW_DIR and optional disc/tool overrides."""
    tww_dir = os.environ.get("TWW_DIR")
    if not tww_dir:
        raise ExtractError("Set TWW_DIR to the absolute path of your tww checkout")
    tww = Path(tww_dir).resolve()
    disc_path = Path(disc).resolve() if disc else (tww / DEFAULT_DISC_REL)
    dtk_path = Path(dtk).resolve() if dtk else (tww / DEFAULT_DTK_REL)
    return {"tww": tww, "disc": disc_path, "dtk": dtk_path}


def check_prerequisites(paths: dict[str, Path]) -> None:
    if not paths["tww"].is_dir():
        raise ExtractError(f"tww checkout not found at '{paths['tww']}' (set TWW_DIR)")
    if not paths["disc"].is_file():
        raise ExtractError(f"disc image not found: {paths['disc']}")
    if not (paths["dtk"].is_file() and os.access(paths["dtk"], os.X_OK)):
        raise ExtractError(f"dtk binary not found or not executable: {paths['dtk']}")


def dtk_vfs_cp(dtk: Path, disc: Path, vsrc: str, dest: Path) -> None:
    """Copy one VFS entry out of the disc. Argument arrays only, never a shell."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    argv = [str(dtk), "vfs", "cp", f"{disc}:{vsrc}", str(dest)]
    proc = subprocess.run(argv, capture_output=True, text=True)
    if proc.returncode != 0:
        raise ExtractError(f"dtk vfs cp failed for {vsrc}: {proc.stderr.strip() or proc.stdout.strip()}")
    if not dest.is_file():
        raise ExtractError(f"dtk vfs cp did not produce {dest} for {vsrc}")


# --- RARC walking --------------------------------------------------------------


def safe_member_path(parts: list[str]) -> str:
    """Reject anything that is not a plain relative path."""
    if not parts:
        raise ExtractError("empty member path")
    for p in parts:
        if (
            not p
            or p in (".", "..")
            or p.startswith("/")
            or p.startswith("\\")
            or "/" in p
            or "\\" in p
            or "\0" in p
        ):
            raise ExtractError(f"unsafe path component {p!r} in member path {parts!r}")
    return "/".join(parts)


def walk_rarc(rarc, archive: str, seen_output: set[str]) -> list[Member]:
    """Enumerate the archive's file members with identity, rejecting cycles."""
    _gclib_modules()  # validates gclib availability before touching the archive
    members: list[Member] = []
    seen_names: set[str] = set()
    entry_index = {id(e): i for i, e in enumerate(rarc.file_entries)}

    def visit(node, prefix: str, chain: frozenset) -> None:
        if id(node) in chain:
            raise ExtractError(f"{archive}: directory cycle at '{prefix}'")
        next_chain = chain | {id(node)}
        for entry in node.files:
            name = entry.name
            if name in (".", ".."):
                if entry.is_dir:
                    continue
                # A *file* named "." or ".." would traverse on write.
                raise ExtractError(f"{archive}: file entry named {name!r} in '{prefix}'")
            if entry.is_dir:
                visit(entry.node, f"{prefix}{name}/", next_chain)
                continue
            parts = prefix.split("/")[:-1] if prefix else []
            rel = safe_member_path(parts + [name])
            out = f"{archive}/{rel}"
            if out in seen_names:
                raise ExtractError(f"{archive}: duplicate member path '{rel}'")
            seen_names.add(out)
            if out in seen_output:
                raise ExtractError(f"duplicate output path across archives: '{out}'")
            seen_output.add(out)
            members.append(
                Member(
                    archive=archive,
                    path=rel,
                    entry_index=entry_index[id(entry)],
                    resource_id=entry.id,
                    data=member_bytes(entry),
                )
            )

    visit(rarc.nodes[0], "", frozenset())
    members.sort(key=lambda m: (m.archive, m.path))
    return members


def member_bytes(entry) -> bytes:
    """A member's content, Yaz0-decompressed if flagged."""
    _, _, yaz0_yay0 = _gclib_modules()
    data = entry.data.getvalue()
    if data[:4] == b"Yaz0":
        data = yaz0_yay0.Yaz0.decompress(BytesIO(data)).getvalue()
    return data


# --- J3D structural validation --------------------------------------------------


def validate_j3d(member: Member) -> None:
    """Magic/type/length/chunk-bounds validation; raises ExtractError."""
    fmt = member.format
    file_type, chunk_tag = FORMAT_TAGS[fmt]
    data = member.data
    what = f"{member.output_rel}"
    if len(data) < 0x20:
        raise ExtractError(f"{what}: {len(data)} bytes is smaller than a J3D1 header")
    magic, ftype, length, nchunks = struct.unpack_from(">4s4sII", data, 0)
    if magic != b"J3D1":
        raise ExtractError(f"{what}: bad magic {magic!r}, expected J3D1")
    if ftype.decode("ascii", "replace") != file_type:
        raise ExtractError(f"{what}: bad file type {ftype!r}, expected {file_type!r}")
    if length != len(data):
        raise ExtractError(f"{what}: header claims {length} bytes but file is {len(data)}")
    if nchunks != 1:
        raise ExtractError(f"{what}: expected exactly 1 chunk, found {nchunks}")
    cmagic, csize = struct.unpack_from(">4sI", data, 0x20)
    if cmagic != chunk_tag:
        raise ExtractError(f"{what}: chunk {cmagic!r} does not match format {fmt} (expected {chunk_tag!r})")
    # Vanilla chunk size fields can overrun the file by up to one 0x20 pad
    # block (observed on LkAnm BTKs, e.g. cutfh.btk: chunk 0x224 bytes at
    # 0x20 in a 0x240 file); the single-chunk J3D loader never walks past the
    # only chunk, and every table/pool read is bounds-checked where the data
    # is parsed. Larger overruns mean corruption.
    chunk_end = 0x20 + csize
    if csize < 0x18 or chunk_end > len(data) + 0x20:
        raise ExtractError(f"{what}: chunk claims {csize} bytes, ending {chunk_end - len(data)} bytes past the file ({len(data)})")
    if fmt == "bck":
        sound_off = struct.unpack_from(">I", data, 0x1C)[0]
        if sound_off != 0xFFFFFFFF:
            if sound_off < chunk_end:
                raise ExtractError(f"{what}: sound data offset {sound_off:#x} inside the ANK1 chunk")
            if sound_off + 2 > len(data):
                raise ExtractError(f"{what}: sound data offset {sound_off:#x} out of bounds")


# --- classification -----------------------------------------------------------


def ank1_joint_count(data: bytes) -> int:
    """Joint count from the ANK1 header (offset 0x0C in the chunk)."""
    return struct.unpack_from(">H", data, 0x20 + 0x0C)[0]


def read_string_table(data: bytes, off: int) -> list[str]:
    """ResNTAB: {u16 count, u16 pad, {u16 hash, u16 offset}[], NUL-terminated names}."""
    if off + 4 > len(data):
        raise ExtractError(f"name table at {off:#x} out of bounds")
    (count,) = struct.unpack_from(">H", data, off)
    names: list[str] = []
    for i in range(count):
        entry_off = off + 4 + 4 * i
        if entry_off + 4 > len(data):
            raise ExtractError(f"name table entry {i} out of bounds")
        (data_off,) = struct.unpack_from(">H", data, entry_off + 2)
        start = off + data_off
        if start >= len(data):
            raise ExtractError(f"name {i} data offset {data_off:#x} out of bounds")
        end = data.index(b"\0", start)
        try:
            names.append(data[start:end].decode("ascii"))
        except UnicodeDecodeError as ex:
            raise ExtractError(f"name {i} is not ASCII") from ex
    return names


def tpt1_names(data: bytes) -> list[str]:
    (off,) = struct.unpack_from(">I", data, 0x20 + 0x1C)
    return read_string_table(data, 0x20 + off)


def ttk1_names(data: bytes) -> list[str]:
    (off,) = struct.unpack_from(">I", data, 0x20 + 0x1C)
    return read_string_table(data, 0x20 + off)


def ttk1_post_names(data: bytes) -> list[str] | None:
    """Post-set material names, or None when the clip has no post set.

    Presence follows the Rust converter's consistency rule: the post set
    exists iff both the post name-table offset and the post track count are
    nonzero (the J3D loader reads both; anything else is inconsistent and
    the converter rejects it, so classification must not accept it either).
    """
    (names_off,) = struct.unpack_from(">I", data, 0x20 + 0x44)
    (track_count,) = struct.unpack_from(">H", data, 0x20 + 0x34)
    if names_off == 0 and track_count == 0:
        return None
    if names_off == 0 or track_count == 0 or track_count % 3 != 0:
        raise ExtractError("btk clip has an inconsistent post set")
    return read_string_table(data, 0x20 + names_off)


def classify(member: Member, cl_joints: int, cl_materials: frozenset[str]) -> Candidate:
    """Deterministic Link-only selection policy.

    - BCK candidates must live in bcks/ and animate exactly the CL joint count.
    - BTP/BTK candidates must target only CL material names (exact match).
    - Mixed in-scope/out-of-scope targets are a hard classification failure.
    """
    what = member.output_rel
    fmt = member.format
    if fmt == "bck":
        joints = ank1_joint_count(member.data)
        if joints == cl_joints:
            return Candidate(member, "included", f"cl-skeleton-{cl_joints}-joints")
        return Candidate(member, "excluded", f"joint-count-{joints}-not-cl-{cl_joints}")
    names = tpt1_names(member.data) if fmt == "btp" else ttk1_names(member.data)
    targets = set(names)
    if fmt == "btk":
        # Every target of an included clip must be in scope, including the
        # optional post track set the converter preserves.
        post = ttk1_post_names(member.data)
        if post is not None:
            targets |= set(post)
    if not targets:
        raise ExtractError(f"{what}: {fmt} clip with an empty material name table")
    matched = targets & cl_materials
    if matched and targets - cl_materials:
        raise ExtractError(
            f"{what}: mixed material targets {sorted(targets)}; CL matches only {sorted(matched)}"
        )
    if matched:
        kind = "face" if fmt == "btp" else "face-scroll"
        return Candidate(member, "included", f"{kind}-targets-{'+'.join(sorted(matched))}")
    return Candidate(member, "excluded", f"no-cl-materials-{'+'.join(sorted(targets))}")


# --- selection manifest -------------------------------------------------------


def load_selection(path: Path) -> dict[str, Any]:
    try:
        raw = path.read_text()
    except FileNotFoundError as ex:
        raise ExtractError(
            f"golden selection manifest {path} is missing; run with --bootstrap to "
            "produce candidate manifests, review them, then promote (see docs/link_animations.md)"
        ) from ex
    doc = json.loads(raw)
    if doc.get("version") != SELECTION_VERSION:
        raise ExtractError(f"{path}: unsupported selection manifest version {doc.get('version')!r}")
    return doc


def compare_selection(golden: dict[str, Any], report: dict[str, Any], source: str) -> None:
    """Every normal run must match the frozen manifest exactly."""
    if golden.get("disc") != "GZLE01":
        raise ExtractError(f"{source}: manifest is for disc {golden.get('disc')!r}, this tool extracts GZLE01 only")
    if list(golden.get("archives", ())) != list(ARCHIVES):
        raise ExtractError(f"{source}: manifest archives {golden.get('archives')} do not match the tool allowlist {list(ARCHIVES)}")
    golden_entries = {(e["archive"], e["member"]): e for e in golden["entries"]}
    actual_entries = {(e["archive"], e["member"]): e for e in report["entries"]}
    problems: list[str] = []
    for key in sorted(set(golden_entries) - set(actual_entries)):
        problems.append(f"missing member {key[0]}:{key[1]}")
    for key in sorted(set(actual_entries) - set(golden_entries)):
        problems.append(f"new member {key[0]}:{key[1]}")
    for key in sorted(set(golden_entries) & set(actual_entries)):
        g, a = golden_entries[key], actual_entries[key]
        if g != a:
            changed = [f for f in g if g[f] != a.get(f)]
            problems.append(f"altered {key[0]}:{key[1]}: fields {changed}")
    if golden.get("cl_model", {}).get("joint_count") != report["cl_model"].get("joint_count") or (
        golden.get("cl_model", {}).get("materials") != report["cl_model"].get("materials")
    ):
        problems.append("cl model metadata differs from the frozen manifest")
    if problems:
        raise ExtractError(
            f"extracted inventory does not match {source}:\n  " + "\n  ".join(problems)
        )


def selection_document(report: Report, cl_meta: dict[str, Any]) -> dict[str, Any]:
    return {
        "version": SELECTION_VERSION,
        "disc": "GZLE01",
        "archives": list(ARCHIVES),
        "cl_model": cl_meta,
        "entries": report.to_json(cl_meta)["entries"],
    }


# --- staging and output trees ----------------------------------------------------


def write_selection_json(path: Path, doc: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc, indent=2, sort_keys=False) + "\n")


def write_inventory(raw_root: Path, report: Report) -> dict[str, Any]:
    """inventory.json for a raw tree: the included set with full identity."""
    entries = [
        {
            "archive": c.member.archive,
            "member": c.member.path,
            "entry_index": c.member.entry_index,
            "resource_id": c.member.resource_id,
            "format": c.member.format,
            "size": len(c.member.data),
            "sha256": sha256_hex(c.member.data),
        }
        for c in sorted(report.included, key=lambda c: (c.member.archive, c.member.path))
    ]
    doc = {"version": INVENTORY_VERSION, "disc": "GZLE01", "entries": entries}
    write_selection_json(raw_root / "inventory.json", doc)
    return doc


def stage_raw(stage: Path, report: Report) -> None:
    """Write every included clip under stage/raw/<archive>/<member>, hashing on reread."""
    raw = stage / "raw"
    for cand in report.included:
        m = cand.member
        out = raw / m.output_rel
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_bytes(m.data)
        reread = out.read_bytes()
        if sha256_hex(reread) != sha256_hex(m.data):
            raise ExtractError(f"staged {m.output_rel} does not hash back to its source bytes")


def verify_hashes(manifest: Path, raw_root: Path, rel_prefix: str) -> None:
    """`sha256sum --check` equivalent over an explicit raw root, no shell."""
    try:
        lines = manifest.read_text().splitlines()
    except FileNotFoundError as ex:
        raise ExtractError(
            f"golden hash manifest {manifest} is missing; it is written by --bootstrap "
            "and promoted after review (see docs/link_animations.md)"
        ) from ex
    for line in lines:
        if not line.strip():
            continue
        digest, _, rel = line.partition("  ")
        rel = rel.removeprefix("*").removeprefix(rel_prefix).lstrip("/")
        path = raw_root / rel
        try:
            data = path.read_bytes()
        except FileNotFoundError as ex:
            raise ExtractError(f"{manifest.name}: {rel}: missing from raw tree") from ex
        if sha256_hex(data) != digest:
            raise ExtractError(f"{manifest.name}: {rel}: hash mismatch (extracted data differs from golden)")


def promote_raw(staged: Path, final_raw: Path, out_dir: Path) -> None:
    """Replace the owned raw tree only after staging verified.

    The previous tree is moved aside (outside any directory the caller
    cleans up) and restored on any failure — including a partial
    destination left by a failed copy — so a valid tree always survives.
    """
    final_raw.parent.mkdir(parents=True, exist_ok=True)
    backup = out_dir / ".previous-raw"
    if backup.exists():
        shutil.rmtree(backup)
    if final_raw.exists():
        shutil.move(str(final_raw), str(backup))
    try:
        shutil.move(str(staged), str(final_raw))
    except BaseException:
        # A failed move can leave a partial destination; drop it before
        # restoring, or the restore would fail on a non-empty directory.
        if final_raw.exists():
            shutil.rmtree(final_raw, ignore_errors=True)
        if backup.exists():
            shutil.move(str(backup), str(final_raw))
        raise
    # Success: the backup is no longer needed.
    if backup.exists():
        shutil.rmtree(backup)


# --- cl.bdl metadata -----------------------------------------------------------


def cl_model_metadata(temp_dir: Path, paths: dict[str, Path]) -> dict[str, Any]:
    """CL joint count + material names from a temporary copy of Link.arc:bdl/cl.bdl."""
    _, gclib_j3d, _ = _gclib_modules()
    cl_path = temp_dir / "cl.bdl"
    dtk_vfs_cp(paths["dtk"], paths["disc"], "/files/res/Object/Link.arc:bdl/cl.bdl", cl_path)
    bdl = gclib_j3d.BDL(str(cl_path))
    materials = list(bdl.mat3.mat_names)
    if len(materials) != len(set(materials)):
        raise ExtractError("cl.bdl MAT3 name table contains duplicate names")
    return {"joint_count": bdl.jnt1.joint_count, "materials": materials}


# --- pipeline -------------------------------------------------------------------


def extract(
    paths: dict[str, Path],
    out_dir: Path,
    selection_path: Path,
    hashes_path: Path,
    bootstrap: bool,
) -> dict[str, Any]:
    """Run the full pipeline. Raises ExtractError on any failure.

    In normal mode, output and goldens live in their production locations. In
    bootstrap mode everything stays under out_dir (an ignored candidate tree)
    and the tracked goldens are neither required nor touched.
    """
    gclib_rarc, _, _ = _gclib_modules()
    check_prerequisites(paths)
    if not bootstrap:
        # Fail fast on missing/invalid goldens before touching the disc.
        load_selection(selection_path)
    out_dir.mkdir(parents=True, exist_ok=True)
    temp_dir = Path(tempfile.mkdtemp(prefix=".tmp-archives-", dir=out_dir))
    stage = Path(tempfile.mkdtemp(prefix=".staging-", dir=out_dir))
    try:
        # -- fetch the three allowlisted archives + cl.bdl into the temp dir ----
        for archive in ARCHIVES:
            dtk_vfs_cp(paths["dtk"], paths["disc"], f"/files/res/Object/{archive}.arc", temp_dir / f"{archive}.arc")
        cl_meta = cl_model_metadata(temp_dir, paths)
        cl_materials = frozenset(cl_meta["materials"])
        if cl_meta["joint_count"] == 0:
            raise ExtractError("cl.bdl reports 0 joints; refusing to classify against an empty skeleton")

        # -- enumerate + classify ------------------------------------------------
        report = Report()
        seen_output: set[str] = set()
        for archive in ARCHIVES:
            rarc = gclib_rarc.RARC(str(temp_dir / f"{archive}.arc"))
            for member in walk_rarc(rarc, archive, seen_output):
                member_dir = member.path.split("/", 1)[0]
                if member_dir not in CANDIDATE_DIRS:
                    report.skipped_dirs[member_dir] = report.skipped_dirs.get(member_dir, 0) + 1
                    continue
                cand = classify(member, cl_meta["joint_count"], cl_materials)
                validate_j3d(member)
                (report.included if cand.classification == "included" else report.excluded).append(cand)
        if not report.included:
            raise ExtractError("selection policy matched no clips; refusing to write an empty raw tree")

        # -- freeze / verify against goldens --------------------------------------
        selection_doc = selection_document(report, cl_meta)
        if bootstrap:
            candidate_raw = out_dir / "candidate" / "raw"
            if candidate_raw.exists():
                shutil.rmtree(candidate_raw)
            stage_raw(stage, report)
            write_inventory(stage / "raw", report)
            promote_raw(stage / "raw", candidate_raw, out_dir)
            write_selection_json(out_dir / "candidate" / "selection.json", selection_doc)
            # Candidate hash lines are relative to the candidate tree itself so
            # `sha256sum --check` works from inside candidate/.
            (out_dir / "candidate" / "link_animation_assets.sha256").write_text(
                "\n".join(
                    f"{sha256_hex(c.member.data)}  raw/{c.member.output_rel}"
                    for c in report.sorted_included()
                )
                + "\n"
            )
            write_selection_json(out_dir / "candidate" / "extraction_report.json", report.to_json(cl_meta))
            print(f"extract_link_animations: BOOTSTRAP: candidate manifests under {out_dir}/candidate/ -- review, then promote")
            return {"bootstrap": True, "included": len(report.included), "excluded": len(report.excluded)}

        golden = load_selection(selection_path)
        compare_selection(golden, selection_doc, str(selection_path))

        # -- stage, verify staged bytes against both goldens, then promote ----------
        stage_raw(stage, report)
        write_inventory(stage / "raw", report)
        verify_hashes(hashes_path, stage / "raw", "assets/link/animations/raw")
        promote_raw(stage / "raw", out_dir / "raw", out_dir)
        write_selection_json(out_dir / "extraction_report.json", report.to_json(cl_meta))
        return {
            "bootstrap": False,
            "included": len(report.included),
            "excluded": len(report.excluded),
        }
    finally:
        shutil.rmtree(temp_dir, ignore_errors=True)
        shutil.rmtree(stage, ignore_errors=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Extract reviewed Link body/face animations from the GZLE01 disc (requires TWW_DIR)"
    )
    parser.add_argument("--disc", help="disc image (default: <tww>/orig/GZLE01/*.ciso)")
    parser.add_argument("--dtk", help="dtk binary (default: <tww>/build/tools/dtk)")
    parser.add_argument(
        "--bootstrap",
        action="store_true",
        help="produce candidate manifests under the ignored candidate/ tree only; never touch goldens",
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=None,
        help="output root (default: assets/link/animations; bootstrap writes under <out>/candidate)",
    )
    parser.add_argument("--selection", type=Path, default=SELECTION_GOLDEN, help="golden selection manifest path")
    parser.add_argument("--hashes", type=Path, default=HASHES_GOLDEN, help="golden sha256 manifest path")
    args = parser.parse_args(argv)

    out_dir = (args.out_dir or ANIM_DIR).resolve()
    if args.bootstrap:
        # Bootstrap never reads or writes the tracked goldens: force the
        # selection/hashes arguments to live under the ignored candidate tree.
        selection = out_dir / "candidate" / "selection.json"
        hashes = out_dir / "candidate" / "link_animation_assets.sha256"
    else:
        selection = args.selection
        hashes = args.hashes

    try:
        paths = resolve_paths(args.disc, args.dtk)
        result = extract(paths, out_dir, selection, hashes, args.bootstrap)
    except ExtractError as ex:
        print(f"extract_link_animations: error: {ex}", file=sys.stderr)
        return 1
    print(
        "extract_link_animations: OK: "
        f"{result['included']} included, {result['excluded']} excluded clips"
        + ("" if result["bootstrap"] else f", raw tree verified against goldens")
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
