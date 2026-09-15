#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.12"
# dependencies = ["gclib @ git+https://github.com/LagoLunatic/gclib@64127742467acb633d51685b9b1798ab45bb4034"]
# ///

# Asset-free unittest driver for the Link animation tooling. Run by
# `just toon_link link-test-animations`. The pinned gclib dependency is
# resolved by uv via this script's PEP-723 header; without uv (or offline)
# this fails loudly instead of skipping.

import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

TESTS_DIR = Path(__file__).resolve().parent


def main() -> int:
    try:
        import gclib  # noqa: F401
    except ImportError as ex:
        print(
            "link-test-animations: error: gclib is not importable — run this "
            "suite via `uv run scripts/tests/run_tests.py` (uv resolves the "
            "pinned dependency in the script header)",
            file=sys.stderr,
        )
        print(f"({ex})", file=sys.stderr)
        return 2
    loader = unittest.TestLoader()
    suite = loader.discover(start_dir=str(TESTS_DIR), pattern="test_*.py")
    runner = unittest.TextTestRunner(verbosity=2)
    result = runner.run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
