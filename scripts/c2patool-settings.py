#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Print the c2patool settings file that scripts/lib/c2p.py uses, for shell callers.

    uv run scripts/c2patool-settings.py                    # network off, no anchors
    uv run scripts/c2patool-settings.py --trust vendored   # anchors + TSA + EKU policy
    uv run scripts/c2patool-settings.py --trust eku        # EKU policy only
    uv run scripts/c2patool-settings.py --fetch            # remote manifest fetch on

Built by differential.py's own c2patool_settings(), through lib/c2p.py. The fish
frames in scripts/pub/ use it via scripts/pub/c2p.fish. Offline. Exit 2 when a trust
file is missing.
"""

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from c2p import settings_text  # noqa: E402


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--trust", choices=["none", "eku", "vendored"], default="none")
    ap.add_argument("--fetch", action="store_true", help="remote_manifest_fetch = true")
    a = ap.parse_args()
    try:
        sys.stdout.write(settings_text(a.trust, a.fetch))
    except FileNotFoundError as e:
        print(f"error: {e}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
