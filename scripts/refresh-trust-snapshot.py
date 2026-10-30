#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27", "httpx>=0.28", "cryptography>=44"]
# ///
"""Refresh the vendored C2PA trust-list snapshot in crates/halftone-c2pa/trust/.

Finds the newest commit touching trust-list/ in c2pa-org/conformance-public, then
fetches both PEM bundles from that commit (not from `main`), so SNAPSHOT names the
commit the files actually came from. Every certificate must parse: a PEM block that
does not is an error, not one fewer anchor. Nothing is written unless both bundles
pass; then both are replaced together, with SNAPSHOT. When the bundles and the commit
are unchanged, nothing is written at all.

Also lists anchors that have expired or expire within --warn-days (informational:
SNAPSHOT's format is read by halftone-c2pa and scripts/cert-shelf-life.sh and does
not change).

Run from anywhere in the repo; commit the result. CI does not run this.
Network: api.github.com ($GITHUB_TOKEN or $GH_TOKEN if set, sent there only),
raw.githubusercontent.com.
Exit 0 written or unchanged, 1 a fetch or a certificate failed, 2 setup error.
"""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Annotated

import httpx
import typer
from cryptography import x509

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from repo import ROOT, TRUST_DIR, C, die  # noqa: E402

REPO = "c2pa-org/conformance-public"
BUNDLES = ["C2PA-TRUST-LIST.pem", "C2PA-TSA-TRUST-LIST.pem"]
TIMEOUT = httpx.Timeout(30.0, connect=10.0)

app = typer.Typer(add_completion=False, pretty_exceptions_show_locals=False)


def latest_commit(client: httpx.Client, headers: dict[str, str]) -> tuple[str, str] | None:
    """(sha, committer date) of the newest commit touching trust-list/, or None."""
    try:
        r = client.get(f"https://api.github.com/repos/{REPO}/commits",
                       params={"path": "trust-list", "per_page": 1}, headers=headers)
        r.raise_for_status()
        c = r.json()[0]
        return c["sha"], c["commit"]["committer"]["date"]
    except (httpx.HTTPError, KeyError, IndexError, ValueError):
        return None


def certificates(name: str, body: bytes) -> list[x509.Certificate]:
    blocks = body.count(b"-----BEGIN CERTIFICATE-----")
    if blocks == 0:
        die(f"{name}: no certificates in the response", 1)
    try:
        certs = x509.load_pem_x509_certificates(body)
    except ValueError as e:
        die(f"{name}: a certificate does not parse ({e})", 1)
    if len(certs) != blocks:
        die(f"{name}: {blocks} PEM blocks but {len(certs)} certificates parsed", 1)
    return certs


def label(c: x509.Certificate) -> str:
    for oid in (x509.NameOID.COMMON_NAME, x509.NameOID.ORGANIZATION_NAME):
        v = c.subject.get_attributes_for_oid(oid)
        if v:
            return str(v[0].value)
    return c.subject.rfc4514_string()


@app.command(help=__doc__)
def main(
    warn_days: Annotated[int, typer.Option(help="List anchors expiring within this many days.")] = 90,
) -> None:
    if not TRUST_DIR.is_dir():
        die(f"no trust directory at {TRUST_DIR}", 2)
    # The API allows 60 unauthenticated requests an hour per address; a token lifts it.
    token = os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")
    headers = {"Authorization": f"Bearer {token}"} if token else {}
    with httpx.Client(timeout=TIMEOUT, follow_redirects=True) as client:
        found = latest_commit(client, headers)
        ref = found[0] if found else "main"
        base = f"https://raw.githubusercontent.com/{REPO}/{ref}/trust-list"
        if not found:
            print(f"{C.YELLOW}GitHub API unavailable: fetching from main, commit unknown{C.RESET}")
        bodies: dict[str, bytes] = {}
        for name in BUNDLES:
            try:
                r = client.get(f"{base}/{name}")
                r.raise_for_status()
            except httpx.HTTPError as e:
                die(f"{name}: {e}", 1)
            bodies[name] = r.content
    certs = {name: certificates(name, body) for name, body in bodies.items()}

    head = f"{REPO}@{found[0][:12]} {found[1]}" if found else f"{REPO}@unknown (API unavailable)"
    snapshot = TRUST_DIR / "SNAPSHOT"
    old = snapshot.read_text().splitlines() if snapshot.is_file() else []
    old_head = old[0] if old else ""
    bundle_lines = [f"{n} sha256 {hashlib.sha256(bodies[n]).hexdigest()} ({len(certs[n])} certs)" for n in BUNDLES]
    # Unchanged means the files on disk and the SNAPSHOT that describes them both match.
    same = all((TRUST_DIR / n).is_file() and (TRUST_DIR / n).read_bytes() == b for n, b in bodies.items()) \
        and all(line in old for line in bundle_lines)
    if same and (old_head == head or not found):
        # Without the API the commit is unknown; same bytes keep the old record.
        print(f"unchanged: {old_head}; nothing written")
    else:
        for name, body in bodies.items():  # both validated above: replace them together
            tmp = TRUST_DIR / f"{name}.new"
            tmp.write_bytes(body)
            tmp.replace(TRUST_DIR / name)
        lines = [head, f"source: {base}",
                 f"fetched: {datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}"]
        lines += bundle_lines
        snapshot.write_text("\n".join(lines) + "\n")
        print(snapshot.read_text(), end="")

    now = datetime.now(timezone.utc)
    soon = now + timedelta(days=warn_days)
    notes = [(c.not_valid_after_utc, n, label(c)) for n in BUNDLES for c in certs[n] if c.not_valid_after_utc <= soon]
    if notes:
        print(f"\nanchors expired or expiring within {warn_days} days:")
        for when, n, who in sorted(notes):
            state = f"{C.RED}expired{C.RESET}" if when <= now else "expires"
            print(f"  {state} {when:%Y-%m-%d}  {who}  ({n})")
    subprocess.run(["git", "status", "--short", "--", str(TRUST_DIR.relative_to(ROOT))], cwd=ROOT)


if __name__ == "__main__":
    app()
