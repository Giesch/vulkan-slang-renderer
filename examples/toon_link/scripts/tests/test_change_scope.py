# animation_change_scope_audit: the working tree may contain only the
# permitted code/docs/hash-manifest changes for this feature — no game
# payloads, no snapshots derived from real data, no changes to the example's
# Rust/shader sources, no edits to the model hash manifests or todo.org.

from __future__ import annotations

import subprocess
import sys
import unittest
from pathlib import Path

TESTS_DIR = Path(__file__).resolve().parent
REPO_ROOT = TESTS_DIR.parent.parent.parent.parent

ALLOWED_PREFIXES = (
    "crates/gx/src/animation_manifest.rs",
    "crates/gx/src/lib.rs",
    "crates/gx/Cargo.toml",
    "crates/convert-link/src/animation/",
    "crates/convert-link/src/bin/convert_link_animations.rs",
    "crates/convert-link/tests/",
    "crates/convert-link/Cargo.toml",
    "examples/toon_link/scripts/extract_link_animations.py",
    "examples/toon_link/scripts/link_animation_oracle.py",
    "examples/toon_link/scripts/link_animation_verify.py",
    "examples/toon_link/scripts/link_animation_selection.json",
    "examples/toon_link/scripts/link_animation_assets.sha256",
    "examples/toon_link/scripts/link_animation_converted.sha256",
    "examples/toon_link/scripts/tests/",
    "examples/toon_link/justfile",
    "docs/link_animations.md",
    "docs/testing.md",
    "AGENTS.md",
    "Cargo.lock",
    ".gitignore",
)

FORBIDDEN_SUFFIXES = (".png", ".ktx2", ".spv", ".arc", ".bck", ".btp", ".btk", ".bdl", ".ciso")

MODEL_HASH_MANIFESTS = (
    "examples/toon_link/scripts/link_assets.sha256",
    "examples/toon_link/scripts/link_converted.sha256",
)


def git(*args: str) -> str:
    proc = subprocess.run(["git", "-C", str(REPO_ROOT), *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise AssertionError(f"git {' '.join(args)} failed: {proc.stderr}")
    return proc.stdout


class ChangeScopeAudit(unittest.TestCase):
    def _changed_paths(self) -> list[tuple[str, str]]:
        lines = git("status", "--porcelain").splitlines()
        out = []
        for line in lines:
            if not line.strip():
                continue
            status = line[:2]
            path = line[3:].strip('"').strip()
            if status.startswith("R") or status.startswith("C"):
                # rename/copy lines carry two paths; keep the destination
                path = path.split(" -> ")[-1].strip('"')
            if "__pycache__" in path or path.endswith(".pyc"):
                continue  # transient bytecode, never committed
            out.append((status, path))
        return out

    def test_changed_paths_are_in_scope(self) -> None:
        problems = []
        for status, path in self._changed_paths():
            if path.startswith("examples/toon_link/assets/"):
                continue  # gitignored output; never tracked
            if path == "todo.org" or path.startswith("llm_notes/"):
                problems.append(f"forbidden path touched: {status} {path}")
                continue
            if path.endswith(FORBIDDEN_SUFFIXES) and not path.endswith(".md"):
                problems.append(f"possible payload committed: {status} {path}")
                continue
            # git reports untracked directories with a trailing slash; accept
            # one when every allowed path it could contain is in scope.
            if path.endswith("/") and any(p.startswith(path) for p in ALLOWED_PREFIXES):
                continue
            if not any(path == p or path.startswith(p) for p in ALLOWED_PREFIXES):
                problems.append(f"out-of-scope change: {status} {path}")
        self.assertEqual(problems, [], "\n".join(problems))

    def test_model_hash_manifests_unchanged(self) -> None:
        for manifest in MODEL_HASH_MANIFESTS:
            diff = git("diff", "--", manifest)
            staged = git("diff", "--cached", "--", manifest)
            self.assertEqual(diff, "", f"{manifest} has unstaged modifications")
            self.assertEqual(staged, "", f"{manifest} has staged modifications")

    def test_no_example_rust_or_shader_changes(self) -> None:
        for prefix in ("examples/toon_link/src/", "examples/toon_link/shaders/"):
            diff = git("diff", "--", prefix)
            staged = git("diff", "--cached", "--", prefix)
            self.assertEqual(diff, "", f"{prefix} modified")
            self.assertEqual(staged, "", f"{prefix} staged-modified")
        # No *new* animation payloads: untracked game-derived files anywhere.
        for status, path in self._changed_paths():
            if status.strip() == "??" and path.endswith(FORBIDDEN_SUFFIXES):
                self.fail(f"untracked payload: {path}")

    def test_gitignore_covers_animation_outputs(self) -> None:
        proc = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "check-ignore", "examples/toon_link/assets/link/animations/raw"],
            capture_output=True,
            text=True,
        )
        self.assertEqual(proc.returncode, 0, "assets/link/animations must stay gitignored")


if __name__ == "__main__":
    unittest.main()
