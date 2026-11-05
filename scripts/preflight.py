#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27", "pydantic>=2.10"]
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""Pre-push check of the Halftone workspace: twelve steps, cheapest first, stopping at
the first failure and naming it.

     1  cargo check, all targets and features, zero warnings
     2  lockfile: one c2pa (0.91.x, D-011), one c2pa_cbor, one ureq; probe pins match
     3  rustfmt
     4  clippy, no default features (-D warnings)
     5  clippy, all features (-D warnings)
     6  tests, all features
     7  rustdoc (-D warnings)
     8  ground truth: every header collected with the vendored EKU policy (by hash) and
        both anchor lists; every expectations.json valid; corpus rules tracked
     9  differential test against ExifTool / c2patool / Credentio
    10  corpus integrity test: files are what their names say
    11  remote-manifest fetch: off by default, embedded files never fetch (one network
        fetch from cai-manifests.adobe.com unless --offline)
    12  docs keep the sentences listed in scripts/preflight.toml; working tree status

Steps 9-11 need the private strata on disk. Numbering is stable: other docs refer
to "preflight step 8". Exit 0 all passed, 1 a step failed, 2 setup error.
Never reads the clipboard.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import time
import tomllib
from collections.abc import Callable
from pathlib import Path
from typing import Annotated

import typer

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from models import ModelError, load_expectations  # noqa: E402
from progress import clock  # noqa: E402
from repo import CORPUS, ROOT, TRUST_DIR, C, die, need  # noqa: E402

GEN = CORPUS / "04-generators"
PROBE = ROOT / "tools" / "d011-softbinding" / "Cargo.toml"
DOCS = ROOT / "scripts" / "preflight.toml"


class Fail(Exception):
    pass


def run(*cmd: str, env: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True,
                          env={**os.environ, **(env or {})})


def stream(*cmd: str, show: re.Pattern[str]) -> tuple[int, list[str]]:
    """Run, printing only lines that match `show` as they arrive; returns all output."""
    p = subprocess.Popen(cmd, cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    lines = []
    assert p.stdout is not None
    for line in p.stdout:
        lines.append(line.rstrip("\n"))
        if show.match(line):
            print(line, end="", flush=True)
    return p.wait(), lines


def must(cmd: list[str], why: str) -> None:
    """Run with output on the terminal; Fail(why) on a non-zero exit."""
    if subprocess.run(cmd, cwd=ROOT).returncode != 0:
        raise Fail(why)


# ---- steps ------------------------------------------------------------------------------

def check(_: bool) -> str:
    r = run("cargo", "check", "--workspace", "--all-targets", "--all-features")
    out = r.stdout + r.stderr
    if r.returncode != 0:
        print(out)
        raise Fail("cargo check")
    warnings = [l for l in out.splitlines() if l.startswith("warning")]
    if warnings:
        print(out)
        raise Fail(f"{len(warnings)} warning line(s)")
    return "no warnings"


def lockfile(_: bool) -> str:
    r = run("cargo", "tree", "--workspace", "--all-features", "-e", "normal", "--prefix", "none")
    if r.returncode != 0:
        raise Fail("cargo tree")

    def versions(name: str) -> list[str]:
        return sorted({m.group(0) for l in r.stdout.splitlines() if (m := re.match(rf"{name} v\S+", l))})

    c2pa, cbor, ureq = versions("c2pa"), versions("c2pa_cbor"), versions("ureq")
    for name, found in (("c2pa", c2pa), ("c2pa_cbor", cbor), ("ureq", ureq)):
        if len(found) != 1:
            raise Fail(f"{name} versions: {found or 'none'}")
    if not c2pa[0].startswith("c2pa v0.91."):
        raise Fail(f"expected c2pa 0.91.x (D-011), got {c2pa[0]}")
    # The soft-binding probe reproduces what Halftone's c2pa discards, so it must run
    # the same c2pa and c2pa_cbor (a scratch-branch bump once leaked past it).
    probe = PROBE.read_text()
    for name, found in (("c2pa", c2pa[0]), ("c2pa_cbor", cbor[0])):
        pin = re.search(rf'^{name} = "=([^"]+)"', probe, re.M)
        if not pin or found != f"{name} v{pin.group(1)}":
            raise Fail(f"{PROBE.relative_to(ROOT)} pins {name} '{pin.group(1) if pin else '?'}', workspace has {found}")
    return f"{c2pa[0]} · {cbor[0]} · {ureq[0]} · probe pinned to the same"


def fmt(_: bool) -> str:
    must(["cargo", "fmt", "--all", "--check"], "cargo fmt --check (run: cargo fmt --all)")
    return "formatted"


def clippy_min(_: bool) -> str:
    must(["cargo", "clippy", "-q", "--workspace", "--all-targets", "--no-default-features", "--", "-D", "warnings"],
         "clippy --no-default-features")
    return "clean"


def clippy_all(_: bool) -> str:
    must(["cargo", "clippy", "-q", "--workspace", "--all-targets", "--all-features", "--", "-D", "warnings"],
         "clippy --all-features")
    return "clean"


def tests(_: bool) -> str:
    must(["cargo", "test", "-q", "--workspace", "--all-features"], "cargo test")
    return "passed"


def rustdoc(_: bool) -> str:
    env = {**os.environ, "RUSTDOCFLAGS": "-D warnings"}
    if subprocess.run(["cargo", "doc", "-q", "--workspace", "--no-deps", "--all-features"],
                      cwd=ROOT, env=env).returncode != 0:
        raise Fail("cargo doc")
    return "clean"


def ground_truth(_: bool) -> str:
    eku = hashlib.sha256((TRUST_DIR / "C2PA-EKU-CONFIG.cfg").read_bytes()).hexdigest()
    paths = sorted(CORPUS.glob("*/expectations.json"))
    for p in paths:
        try:
            exp = load_expectations(p)
        except ModelError as e:
            raise Fail(str(e).replace(f"{ROOT}/", "")) from None
        if not exp.full_trust(eku):
            raise Fail(f"{p.relative_to(ROOT)}: collected under other trust inputs; "
                       f"re-collect with uv run scripts/collect.py {p.parent.name}")
    # The corpus rules are reviewed like code: they must be tracked, not ignored.
    rules = (GEN / "corpus-rules.json").relative_to(ROOT)
    if run("git", "check-ignore", "-q", str(rules)).returncode == 0:
        raise Fail(f"{rules} is ignored (corpus/.gitignore patterns are relative to corpus/)")
    if run("git", "ls-files", "--error-unmatch", str(rules)).returncode != 0:
        raise Fail(f"{rules} is not tracked")
    return f"{len(paths)} headers valid and on the vendored trust inputs; corpus rules tracked"


def differential(_: bool) -> str:
    rc, out = stream("cargo", "test", "-p", "halftone-cli", "--features", "c2pa", "--test", "differential",
                     "--", "--ignored", "--nocapture", show=re.compile(r"^differential"))
    if rc != 0:
        print("\n".join(l for l in out if l.startswith("|")))
        raise Fail("differential disagreements")
    return "0 disagreements"


def corpus(_: bool) -> str:
    rc, out = stream("cargo", "test", "--release", "-p", "halftone-cli", "--test", "corpus",
                     "--", "--ignored", "--nocapture", show=re.compile(r"^corpus"))
    if rc != 0:
        print("\n".join(l for l in out if l.startswith("|")))
        raise Fail("corpus integrity (HALFTONE_CORPUS_REPORT=1 shows every score; "
                   "uv run scripts/corpus-inspect.py shows the images)")
    return "names, formats and images consistent"


def _c2pa(ht: str, *args: str, env: dict[str, str] | None = None) -> dict:
    """The c2pa evidence record of `ht inspect --json --only manifest ARGS`. ht exits 2
    when a source is Present, so the exit status is not an error here."""
    r = run(ht, "inspect", "--json", "--only", "manifest", *args, env=env)
    try:
        doc = json.loads(r.stdout)
    except json.JSONDecodeError:
        raise Fail(f"ht printed no JSON: {r.stderr.strip()[:200]}") from None
    for ev in doc.get("evidence", []):
        if (ev.get("source") or {}).get("name") == "c2pa":
            return ev
    raise Fail("no c2pa evidence in ht's output")


def remote_fetch(offline: bool) -> str:
    must(["cargo", "build", "-q", "-p", "halftone-cli", "--features", "c2pa"], "cargo build")
    ht = str(ROOT / "target" / "debug" / "ht")
    # Picked by pattern, so a path-token rename in the corpus does not break this step.
    ff = next(iter(sorted(GEN.glob("adobe-firefly__firefly-image-5__web-download*__p3__3.png"))), None)
    goog = next(iter(sorted(GEN.glob("google-flow__nano-banana-2__web-download*__p1__1.jpeg"))), None)
    if ff is None:
        raise Fail(f"no Firefly Image 5 p3 download in {GEN.relative_to(ROOT)}")
    if goog is None:
        raise Fail(f"no Flow p1 download in {GEN.relative_to(ROOT)}")

    ev = _c2pa(ht, str(ff))
    d = ev.get("details") or {}
    if not (ev.get("status") == "inconclusive" and d.get("remote_manifest_url") is not None
            and d.get("fetched_from") is None and "--fetch-remote-manifests" in (ev.get("rationale") or "")):
        raise Fail("default run on a remote-only file should be offline Inconclusive naming the flag")
    print(f"{C.GREEN}ok{C.RESET}  remote-only file, no flag: Inconclusive, URL reported, nothing fetched")

    # The flag on a file with an embedded manifest must not reach the fetch path. Both
    # streams are searched, so this holds whether tracing logs to stdout or stderr.
    r = run(ht, "inspect", "--json", "--only", "manifest", "--fetch-remote-manifests", str(goog),
            env={"RUST_LOG": "halftone_c2pa=info"})
    if "fetching remote C2PA manifest" in r.stdout + r.stderr:
        raise Fail("an embedded manifest triggered a fetch")
    if (_c2pa(ht, "--fetch-remote-manifests", str(goog)).get("details") or {}).get("fetched_from") is not None:
        raise Fail("embedded manifest reported fetched_from")
    print(f"{C.GREEN}ok{C.RESET}  embedded manifest + flag: no fetch")

    if offline:
        return "skipped the online fetch (--offline)"
    ev = _c2pa(ht, "--fetch-remote-manifests", str(ff))
    d = ev.get("details") or {}
    if not (d.get("fetched_from") is not None and d.get("fetched_at") is not None
            and d.get("validation_state") is not None and d.get("remote_manifest_url") is None
            and (ev.get("rationale") or "").startswith("Fetched on request")):
        raise Fail("fetch from cai-manifests.adobe.com (network down? rerun with --offline)")
    return "remote-only file + flag: fetched, dated, validated"


def docs(_: bool) -> str:
    rows = tomllib.loads(DOCS.read_text()).get("require", [])
    for row in rows:
        path = ROOT / row["file"]
        if not path.is_file():
            raise Fail(f"{row['file']} is missing ({row['why']}); see {DOCS.relative_to(ROOT)}")
        if row["text"] not in path.read_text():
            raise Fail(f"{row['file']} no longer says '{row['text']}' ({row['why']}); "
                       f"see {DOCS.relative_to(ROOT)}")
    subprocess.run(["git", "status", "--short"], cwd=ROOT)
    return f"{len(rows)} required passages present"


STEPS: list[tuple[str, Callable[[bool], str]]] = [
    ("cargo check, all targets and features: zero warnings", check),
    ("lockfile: one c2pa (0.91.x, D-011), one c2pa_cbor, one ureq; probe pins match", lockfile),
    ("rustfmt", fmt),
    ("clippy, no default features (-D warnings)", clippy_min),
    ("clippy, all features (-D warnings)", clippy_all),
    ("tests, all features", tests),
    ("rustdoc (-D warnings): missing docs, broken intra-doc links", rustdoc),
    ("ground truth: vendored trust inputs in every header; corpus rules tracked", ground_truth),
    ("differential test against ExifTool / c2patool / Credentio", differential),
    ("corpus: files are what their names say", corpus),
    ("remote-manifest fetch: off by default, embedded files never fetch", remote_fetch),
    ("docs and working tree", docs),
]

app = typer.Typer(add_completion=False, pretty_exceptions_show_locals=False)


@app.command(help=__doc__)
def main(
    offline: Annotated[bool, typer.Option("--offline", help="Skip step 11's fetch from cai-manifests.adobe.com.")] = False,
    start: Annotated[int, typer.Option("--from", min=1, max=len(STEPS), help="Start at this step (after a fix).")] = 1,
    list_steps: Annotated[bool, typer.Option("--list", help="List the steps and exit.")] = False,
) -> None:
    if list_steps:
        for i, (title, _) in enumerate(STEPS, 1):
            print(f"{i:>3}  {title}")
        raise typer.Exit(0)
    need("cargo", "git")
    if not (ROOT / "Cargo.toml").is_file():
        die(f"no workspace at {ROOT}", 2)
    started = time.monotonic()
    for i, (title, fn) in enumerate(STEPS, 1):
        if i < start:
            continue
        print(f"\n{C.BOLD_CYAN}== {i}. {title}{C.RESET}", flush=True)
        t = time.monotonic()
        try:
            msg = fn(offline)
        except Fail as e:
            print(f"{C.RED}FAIL (step {i}): {e}{C.RESET}", file=sys.stderr)
            print(f"{C.DIM}fix it, then: uv run scripts/preflight.py --from {i}{C.RESET}", file=sys.stderr)
            raise typer.Exit(1) from None
        print(f"{C.GREEN}ok{C.RESET}  {msg}  {C.DIM}{clock(time.monotonic() - t)}{C.RESET}", flush=True)
    done = "all" if start == 1 else f"steps {start}-{len(STEPS)}"
    print(f"\n{C.GREEN}{done} passed{C.RESET}  {C.DIM}{clock(time.monotonic() - started)}{C.RESET}")


if __name__ == "__main__":
    app()
