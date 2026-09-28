# /// script
# requires-python = ">=3.14"
# ///
"""Which validation codes does c2patool report that Halftone's output never mentions?

Reads a versiondiff snapshot (default: newest target/versiondiff/head-*.json) that was
taken with --c2patool, so no tools run. For every code c2patool reported, failure or
informational, counts the files where the code appears anywhere in Halftone's record
for that file (details or rationale).

    uv run scripts/info-codes.py [snapshot.json] [--files a-bad-12.jpg ...]

--files prints Halftone's full manifest record (rationale untruncated, every details
field) for the named files, matched by file name.
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import defaultdict
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("snapshot", nargs="?")
    ap.add_argument("--files", nargs="*", default=[])
    a = ap.parse_args()
    snap = Path(a.snapshot) if a.snapshot else max(Path("target/versiondiff").glob("head-*.json"),
                                                  key=lambda p: p.stat().st_mtime)
    s = json.loads(snap.read_text())
    if not s["meta"].get("c2patool"):
        print(f"{snap} was taken without --c2patool; nothing to compare against", file=sys.stderr)
        return 2
    print(f"snapshot {snap.name}: ht {s['meta']['ht']}, c2patool {s['meta']['c2patool']}\n")

    seen: dict[tuple[str, str], list[str]] = defaultdict(list)    # (kind, code) -> files
    missing: dict[tuple[str, str], list[str]] = defaultdict(list)
    for f, r in s["files"].items():
        c = r.get("c2patool", {})
        ht_text = json.dumps(r["ht"])
        for kind in ("failure", "info", "success"):  # 0.91 moved ocsp.notRevoked to success
            for code in c.get(kind) or []:
                seen[(kind, code)].append(f)
                if code not in ht_text:
                    missing[(kind, code)].append(f)

    print("| kind | code | files (c2patool) | Halftone mentions it | example missing |")
    print("|---|---|---|---|---|")
    for (kind, code), fs in sorted(seen.items(), key=lambda kv: (kv[0][0], -len(kv[1]))):
        miss = missing.get((kind, code), [])
        ex = Path(miss[0]).name if miss else ""
        print(f"| {kind} | `{code}` | {len(fs)} | {len(fs) - len(miss)} | {ex} |")

    fail_missing = sum(len(v) for (k, _), v in missing.items() if k == "failure")
    info_missing = sum(len(v) for (k, _), v in missing.items() if k == "info")
    succ_missing = sum(len(v) for (k, _), v in missing.items() if k == "success")
    print(f"\nfailure codes Halftone never mentions: {fail_missing} (should be 0)")
    print(f"informational codes Halftone never mentions: {info_missing}")
    print(f"success codes Halftone never mentions: {succ_missing}"
          + ("" if any(k == "success" for k, _ in seen) else
             " (snapshot has no success codes: taken before versiondiff recorded them; re-snapshot)"))

    for name in a.files:
        hits = [f for f in s["files"] if Path(f).name == name]
        for f in hits:
            print(f"\n=== {f}")
            print("c2patool:", json.dumps(s["files"][f].get("c2patool"), indent=1))
            for src, rec in s["files"][f]["ht"].get("sources", {}).items():
                d = rec.get("details", {})
                if "validation_state" in d or "error" in d:
                    print(f"ht {src} verdict: {rec.get('verdict') or rec.get('status')}")
                    print(f"ht rationale: {rec.get('rationale')}")
                    print("ht details:", json.dumps({k: v for k, v in d.items() if k != "trust"}, indent=1))
        if not hits:
            print(f"\n=== {name}: not in snapshot")
    return 0


if __name__ == "__main__":
    sys.exit(main())
