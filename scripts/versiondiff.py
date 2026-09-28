# /// script
# requires-python = ">=3.14"
# ///
"""Snapshot Halftone (and optionally c2patool) over the corpus, then compare snapshots.

Built for dependency upgrades (c2pa first): snapshot the build you have, snapshot the
upgraded build, and review every change in what Halftone says, not just the state.

  snapshot   run one ht build (and optionally one c2patool) over corpus dirs and/or the
             C2PA v2.2 conformance samples; write a JSON snapshot
  compare    diff two snapshots: every changed ht field, grouped; c2patool changes;
             Halftone-vs-c2patool mismatches inside each snapshot; conformance hits

    uv run scripts/versiondiff.py snapshot --label 0.90 --ht /tmp/ht-base/target/release/ht \
        --c2patool /opt/homebrew/bin/c2patool --settings-style legacy \
        --conformance corpus/differential/0*/
    uv run scripts/versiondiff.py compare target/versiondiff/0.90.json target/versiondiff/0.91.json

compare exits 1 when the newer snapshot has a Halftone-vs-c2patool mismatch (a Halftone
bug candidate); changes between snapshots are for review and never fail the run.
Snapshots live in target/versiondiff/ and are not committed: they carry rationale text
about private corpus files.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import urllib.request
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

SHA = "ae61d4c5cee15cf641cd9455916c75db81753567"  # public-testfiles, v2.2 samples (c2pa-rs#2738)
BASE = f"https://raw.githubusercontent.com/c2pa-org/public-testfiles/{SHA}/"
CONF = Path("target/conformance-v22")
OUT = Path("target/versiondiff")
TRUST = Path("crates/halftone-c2pa/trust")
SKIP_SUFFIXES = {".json", ".txt", ".md", ".toml", ".gitignore", ".fish", ".py"}
# Fields that change on every run, not with the library. Extend with --ignore.
DEFAULT_IGNORE = {"evaluated_at", "inspected_at", "generated_at", "elapsed_ms", "duration_ms"}


# ---------------------------------------------------------------- inputs

def conformance_files() -> dict[str, list[str]]:
    CONF.mkdir(parents=True, exist_ok=True)
    cat = CONF / "samples.json"
    if not cat.exists():
        urllib.request.urlretrieve(BASE + "samples.json", cat)
    out = {}
    for s in json.loads(cat.read_text()):
        p = CONF / s["file"]
        if not p.exists():
            p.parent.mkdir(parents=True, exist_ok=True)
            urllib.request.urlretrieve(BASE + s["file"], p)
        out[str(p)] = s["expectedFailureCodes"]
    return out


def corpus_files(dirs: list[Path]) -> list[str]:
    files = []
    for d in dirs:
        for p in sorted(d.rglob("*")):
            if p.is_file() and p.suffix.lower() not in SKIP_SUFFIXES and not p.name.startswith("."):
                files.append(str(p))
    return files


# ---------------------------------------------------------------- tools

def settings_toml(style: str) -> str:
    manifest = (TRUST / "C2PA-TRUST-LIST.pem").read_text()
    tsa = (TRUST / "C2PA-TSA-TRUST-LIST.pem").read_text()
    eku = (TRUST / "C2PA-EKU-CONFIG.cfg").read_text()
    head = "[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\nverify_trust = true\n\n"
    if style == "legacy":  # c2pa <= 0.90: flat keys, one bundle
        return head + f'[trust]\ntrust_anchors = """\n{manifest}\n{tsa}"""\ntrust_config = """\n{eku}"""\n'
    return head + (  # c2pa >= 0.91: typed anchors
        f'[trust]\ntrust_config = """\n{eku}"""\n\n'
        f'[[trust.anchors]]\ntrust_kind = "manifest"\ntrust_anchors = """\n{manifest}"""\n\n'
        f'[[trust.anchors]]\ntrust_kind = "tsa"\ntrust_anchors = """\n{tsa}"""\n'
    )


def codes(obj, key: str) -> set[str]:
    out: set[str] = set()
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k == key and isinstance(v, list):
                out |= {e["code"] if isinstance(e, dict) and "code" in e else str(e)
                        for e in v if isinstance(e, (dict, str))}
            else:
                out |= codes(v, key)
    elif isinstance(obj, list):
        for v in obj:
            out |= codes(v, key)
    return out


def run_c2patool(exe: str, settings: Path, env: dict, path: str) -> dict:
    p = subprocess.run([exe, "--settings", str(settings), path], capture_output=True, text=True, env=env)
    if p.returncode != 0:
        msg = (p.stderr.strip().splitlines() or [f"exit {p.returncode}"])[-1].strip()
        if "no jumbf" in msg.lower() or "no claim found" in msg.lower():
            return {"present": False}
        return {"error": msg[:200]}
    j = json.loads(p.stdout)
    fail = codes(j, "failure") | {e.get("code") for e in j.get("validation_status", []) if isinstance(e, dict)}
    # c2pa moves codes between buckets across versions (0.91 logs ocsp.notRevoked as
    # success, 0.90 as informational), so record all three.
    return {"state": j.get("validation_state"), "failure": sorted(c for c in fail if c),
            "info": sorted(codes(j, "informational")), "success": sorted(codes(j, "success"))}


def strip(obj, ignore: set[str]):
    if isinstance(obj, dict):
        return {k: strip(v, ignore) for k, v in obj.items() if k not in ignore}
    if isinstance(obj, list):
        return [strip(v, ignore) for v in obj]
    return obj


def source_records(j) -> dict[str, dict]:
    """Every object carrying both a rationale and details: one per evidence source."""
    found, stack, n = {}, [j], 0
    while stack:
        o = stack.pop(0)
        if isinstance(o, dict):
            if "rationale" in o and isinstance(o.get("details"), dict):
                src = o.get("source") or o.get("id") or o.get("name")
                name = str(src.get("name", src) if isinstance(src, dict) else src or f"source{n}")
                n += 1
                found[name] = o
                continue
            stack.extend(o.values())
        elif isinstance(o, list):
            stack.extend(o)
    return found


def run_ht(exe: str, ignore: set[str], path: str) -> dict:
    p = subprocess.run([exe, "inspect", "--json", path], capture_output=True, text=True)
    try:
        j = json.loads(p.stdout)
    except json.JSONDecodeError:
        return {"error": f"ht exit {p.returncode}: {p.stderr.strip()[:200]}"}
    recs = source_records(j)
    if not recs:
        return {"error": "no source records in ht JSON"}
    return {"sources": {k: strip(v, ignore) for k, v in recs.items()}}


def tool_version(exe: str) -> str:
    try:
        return subprocess.run([exe, "--version"], capture_output=True, text=True).stdout.strip()
    except OSError as e:
        return f"unavailable: {e}"


def snapshot(a) -> int:
    ignore = DEFAULT_IGNORE | set(a.ignore)
    expected: dict[str, list[str]] = conformance_files() if a.conformance else {}
    files = corpus_files([Path(d) for d in a.dirs]) + list(expected)
    if not files:
        print("no input files", file=sys.stderr)
        return 2
    with tempfile.TemporaryDirectory() as tmp:
        env = {k: v for k, v in os.environ.items() if k not in ("C2PATOOL_SETTINGS", "C2PATOOL_TRUST_ANCHORS")}
        env["XDG_CONFIG_HOME"] = os.path.join(tmp, "empty")  # D-008: no ambient c2pa.toml
        os.makedirs(env["XDG_CONFIG_HOME"])
        settings = Path(tmp, "c2pa.toml")
        if a.c2patool:
            settings.write_text(settings_toml(a.settings_style))

        def one(f: str) -> tuple[str, dict]:
            row = {"ht": run_ht(a.ht, ignore, f)}
            if a.c2patool:
                row["c2patool"] = run_c2patool(a.c2patool, settings, env, f)
            if f in expected:
                row["expected"] = expected[f]
            return f, row

        with ThreadPoolExecutor(max_workers=a.jobs) as pool:
            rows = dict(pool.map(one, files))

    OUT.mkdir(parents=True, exist_ok=True)
    out = a.out or OUT / f"{a.label}.json"
    meta = {"label": a.label, "note": a.note, "ht": tool_version(a.ht),
            "c2patool": tool_version(a.c2patool) if a.c2patool else None,
            "settings_style": a.settings_style if a.c2patool else None,
            "conformance_commit": SHA if a.conformance else None, "ignored_fields": sorted(ignore)}
    out.write_text(json.dumps({"meta": meta, "files": rows}, indent=2, sort_keys=True) + "\n")
    for d in a.dirs:
        print(f"  {sum(1 for f in rows if f.startswith(str(Path(d))))} from {d}")
    if expected:
        print(f"  {len(expected)} conformance samples")
    errs = sum(1 for r in rows.values() if "error" in r["ht"])
    print(f"{len(rows)} files -> {out}" + (f" ({errs} ht errors)" if errs else ""))
    return 0


# ---------------------------------------------------------------- compare

def flatten(obj, prefix: str = "") -> dict[str, str]:
    out: dict[str, str] = {}
    if isinstance(obj, dict):
        for k, v in obj.items():
            out |= flatten(v, f"{prefix}.{k}" if prefix else k)
    elif isinstance(obj, list) and obj and all(isinstance(v, dict) and "code" in v for v in obj):
        # status lists: order is not meaningful; compare the code set, then per-code fields
        out[prefix] = json.dumps(sorted({v["code"] for v in obj}))
        per: dict[str, dict[str, list]] = defaultdict(lambda: defaultdict(list))
        for v in obj:
            for k, x in v.items():
                if k != "code":
                    per[v["code"]][k].append(json.dumps(x))
        for code, fields in per.items():
            for k, xs in fields.items():
                out[f"{prefix}{{{code}}}.{k}"] = " | ".join(sorted(xs))
    elif isinstance(obj, list) and all(not isinstance(v, (dict, list)) for v in obj):
        out[prefix] = json.dumps(sorted(obj, key=str) if prefix.endswith(("codes", "status")) else obj)
    elif isinstance(obj, list):
        for i, v in enumerate(obj):
            out |= flatten(v, f"{prefix}[{i}]")
    else:
        out[prefix] = json.dumps(obj)
    return out


def manifest_view(ht: dict) -> tuple:
    for name, rec in ht.get("sources", {}).items():
        d = rec.get("details", {})
        if "validation_state" in d or (("error" in d or "remote_manifest_url" in d)
                                       and any(t in name for t in ("c2pa", "manifest"))):
            return d.get("validation_state"), frozenset(codes(d, "validation_status"))
    return ("error", frozenset()) if "error" in ht else (None, frozenset())


def ref_view(c: dict) -> tuple:
    if c.get("present") is False:
        return None, frozenset()
    if "error" in c:
        return None, frozenset()  # c2patool raising = ht "could not be read" (state None)
    return c.get("state"), frozenset(c.get("failure", []))


def mismatches(snap: dict) -> list[str]:
    out = []
    for f, r in snap["files"].items():
        if "c2patool" in r and manifest_view(r["ht"]) != ref_view(r["c2patool"]):
            h, c = manifest_view(r["ht"]), ref_view(r["c2patool"])
            out.append(f"{f}: ht={h[0]} {sorted(h[1])}  c2patool={c[0]} {sorted(c[1])}")
    return out


def conformance_hits(snap: dict) -> dict[str, bool]:
    return {f: set(r["expected"]) <= set(manifest_view(r["ht"])[1])
            for f, r in snap["files"].items() if r.get("expected")}


def short(v: str, n: int = 140) -> str:
    return v if len(v) <= n else v[: n - 1] + "…"


def compare(a) -> int:
    A, B = (json.loads(Path(p).read_text()) for p in (a.old, a.new))
    la, lb = A["meta"]["label"], B["meta"]["label"]
    print(f"# versiondiff {la} → {lb}\n")
    for s in (A, B):
        m = s["meta"]
        print(f"- **{m['label']}**: ht `{m['ht']}`, c2patool `{m['c2patool']}` ({m['settings_style']}), {m['note'] or ''}")
    fa, fb = set(A["files"]), set(B["files"])
    if fa ^ fb:
        print(f"\nfile sets differ: {len(fa - fb)} only in {la}, {len(fb - fa)} only in {lb}; comparing the {len(fa & fb)} in both")

    # ht: every changed field, grouped by (source, field, old, new)
    groups: dict[tuple, list[str]] = defaultdict(list)
    for f in sorted(fa & fb):
        ha, hb = A["files"][f]["ht"], B["files"][f]["ht"]
        sa, sb = ha.get("sources", {}), hb.get("sources", {})
        for src in sorted(set(sa) | set(sb)):
            da, db = sa.get(src, {}).get("details", {}), sb.get(src, {}).get("details", {})
            if ("error" in da) != ("error" in db):
                def state(d, r):
                    return f"could not be read: {d['error']}" if "error" in d else \
                        f"{d.get('validation_state')}: {r.get('rationale', '')}"
                groups[(src, "readability", state(da, sa.get(src, {})), state(db, sb.get(src, {})))].append(f)
                continue
            xa, xb = flatten(sa.get(src, {})), flatten(sb.get(src, {}))
            for k in sorted(set(xa) | set(xb)):
                if xa.get(k) != xb.get(k):
                    groups[(src, k, xa.get(k, "∅"), xb.get(k, "∅"))].append(f)
        if ("error" in ha) != ("error" in hb) or ha.get("error") != hb.get("error"):
            groups[("ht", "error", str(ha.get("error", "∅")), str(hb.get("error", "∅")))].append(f)
    changed_files = {f for fs in groups.values() for f in fs}
    print(f"\n## Halftone: {len(changed_files)} of {len(fa & fb)} files changed, {len(groups)} distinct changes\n")
    for (src, k, old, new), fs in sorted(groups.items(), key=lambda kv: (-len(kv[1]), kv[0])):
        print(f"- **{src} · {k}** on {len(fs)} file(s), e.g. `{Path(fs[0]).name}`")
        print(f"  - {la}: {short(old)}")
        print(f"  - {lb}: {short(new)}")
        if a.verbose and len(fs) > 1:
            print("  - files: " + ", ".join(Path(x).name for x in fs))

    # c2patool changes
    cg: dict[tuple, list[str]] = defaultdict(list)
    for f in sorted(fa & fb):
        ca, cb = A["files"][f].get("c2patool"), B["files"][f].get("c2patool")
        if ca is not None and cb is not None and ca != cb:
            for k in sorted(set(ca) | set(cb)):
                if ca.get(k) != cb.get(k):
                    cg[(k, json.dumps(ca.get(k)), json.dumps(cb.get(k)))].append(f)
    if cg:
        print(f"\n## c2patool: {len({x for fs in cg.values() for x in fs})} files changed\n")
        for (k, old, new), fs in sorted(cg.items(), key=lambda kv: -len(kv[1])):
            print(f"- **{k}** on {len(fs)} file(s), e.g. `{Path(fs[0]).name}`: {short(old, 90)} → {short(new, 90)}")

    # consistency inside each snapshot
    print("\n## Halftone vs c2patool inside each snapshot (bug candidates)\n")
    worst = 0
    for s in (A, B):
        mm = mismatches(s)
        print(f"- {s['meta']['label']}: {len(mm)} mismatches")
        for m in mm:
            print(f"  - {m}")
        worst = len(mm)  # ends on B

    # conformance
    ha_, hb_ = conformance_hits(A), conformance_hits(B)
    if ha_ or hb_:
        print(f"\n## Conformance samples: expected codes reported {sum(ha_.values())}/{len(ha_)} → "
              f"{sum(hb_.values())}/{len(hb_)}\n")
        for f in sorted(set(ha_) & set(hb_)):
            if ha_[f] != hb_[f]:
                print(f"- {Path(f).name}: {'hit' if ha_[f] else 'miss'} → {'hit' if hb_[f] else 'miss'}")
    return 1 if worst else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("snapshot")
    s.add_argument("dirs", nargs="*", help="corpus directories (recursive)")
    s.add_argument("--label", required=True)
    s.add_argument("--ht", required=True)
    s.add_argument("--c2patool")
    s.add_argument("--settings-style", choices=["legacy", "typed"], default="typed")
    s.add_argument("--conformance", action="store_true", help="include the C2PA v2.2 samples")
    s.add_argument("--note", default="", help="free text recorded in the snapshot, e.g. the commit")
    s.add_argument("--ignore", action="append", default=[], help="details field to drop (repeatable)")
    s.add_argument("--jobs", type=int, default=os.cpu_count() or 4)
    s.add_argument("--out", type=Path)
    c = sub.add_parser("compare")
    c.add_argument("old")
    c.add_argument("new")
    c.add_argument("-v", "--verbose", action="store_true", help="list every file per change")
    a = ap.parse_args()
    return snapshot(a) if a.cmd == "snapshot" else compare(a)


if __name__ == "__main__":
    sys.exit(main())
