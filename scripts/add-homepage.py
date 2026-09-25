#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# ///
"""Make every workspace member inherit `homepage` from `[workspace.package]`.

Inserts `homepage.workspace = true` right after `repository.workspace = true` in each
crates/*/Cargo.toml. Idempotent: files that already have a homepage line are skipped.
Exits 1 if a member has no `repository.workspace = true` to anchor on.
"""

import sys
from pathlib import Path

ANCHOR = "repository.workspace = true"
LINE = "homepage.workspace = true"


def main() -> int:
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.cwd()
    manifests = sorted(root.glob("crates/*/Cargo.toml"))
    if not manifests:
        print(f"no crates/*/Cargo.toml under {root}", file=sys.stderr)
        return 1
    missing = []
    for path in manifests:
        lines = path.read_text().splitlines(keepends=True)
        if any(l.lstrip().startswith("homepage") for l in lines):
            print(f"skip     {path.relative_to(root)} (already has homepage)")
            continue
        at = next((i for i, l in enumerate(lines) if l.strip() == ANCHOR), None)
        if at is None:
            missing.append(path)
            print(f"MISSING  {path.relative_to(root)} (no '{ANCHOR}')")
            continue
        lines.insert(at + 1, LINE + "\n")
        path.write_text("".join(lines))
        print(f"added    {path.relative_to(root)}")
    return 1 if missing else 0


if __name__ == "__main__":
    sys.exit(main())
