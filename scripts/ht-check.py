#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
# ///
"""Is the `ht` the scripts would run built from the source as it is now?

    uv run scripts/ht-check.py            # say which binary, when built, and the verdict
    uv run scripts/ht-check.py -q         # only the exit code, and the reason when stale
    uv run scripts/ht-check.py --ht target/debug/ht

The same check every script makes before it calls `ht` (lib/htbin.py, which has the
rule and its limits). Here on its own, for a look before a session (`dev ht-check`)
and for the fish frames, which cannot import it. A frame calls the script itself,
after `cd $repo`, so it does not depend on `dev` being installed:

    uv run scripts/ht-check.py -q; or exit 2
    uv run scripts/ht-check.py -q --ht $HT; or exit 2     # a frame that runs $HT

HT_STALE_OK=1 (or `dev --stale-ok ht-check`) makes a stale binary pass with a
warning, as it does in the scripts.
Offline. Reads no file's content.
Exit 0 current (or allowed), 2 stale or not found.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from htbin import describe, require_ht, state  # noqa: E402
from repo import C  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--ht", default=None, help="ht binary (default: ht on PATH).")
    ap.add_argument("-q", "--quiet", action="store_true", help="Print nothing when the binary is current.")
    from complete import serve  # tab completion (lib/complete.py); returns unless asked

    serve(ap)
    a = ap.parse_args()

    require_ht(a.ht)  # exits 2 with the reason when stale or missing
    if not a.quiet:
        s = state(a.ht)
        verdict = "allowed by HT_STALE_OK" if s.stale else "not checked" if s.foreign else "current"
        print(f"{C.GREEN if verdict == 'current' else C.YELLOW}{verdict}{C.RESET}")
        for line in describe(s):
            print(f"  {line}")


if __name__ == "__main__":
    main()
