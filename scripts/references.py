#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = []
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""Build and use the pinned reference tools image (docker/references.dockerfile).

    uv run scripts/references.py init --credentio-commit <full sha>   # write docker/references.env from this machine
    uv run scripts/references.py show                 # pins vs this machine vs the image
    uv run scripts/references.py build                # build the image for the current pins
    uv run scripts/references.py collect 04 06        # = collect.py 04 06, inside the image
    uv run scripts/references.py run -- bash          # any command inside the image
    uv run scripts/references.py bump credentio       # newest Credentio commit at least 14 days old
    uv run scripts/references.py bump c2patool --dry-run   # what the newest c2patool would be, change nothing
    uv run scripts/references.py bump credentio c2patool   # both, in one build, collection and test
    uv run scripts/references.py bump all --dry-run        # the newest of every tool, change nothing
    uv run scripts/references.py hold openssl <why>        # keep a pin put; bump passes over it

The image holds ExifTool, OpenSSL, c2patool, Credentio's c2pa_validate, a
conformance-public checkout, uv and Python, each at the version in
docker/references.env. Its tag is derived from the pins and the Dockerfile, so a
changed pin is a new image. The same pins give the same reference versions, the ones
the headers record; a rebuild is not bit-for-bit identical, since the base images,
apt packages, LLVM point release and Python patch release float. Collection runs in the
container against this checkout (bind-mounted at /work, as your user), with uv's
cache in the named volume halftone-references-uv. ht and `cargo test` stay on the host.

`bump <tool>[=value] ...` is the manual tool update: it edits the pins, builds, re-collects
every stratum in the image, runs the differential test on the host and prints the
diff. It never commits; review the diff and write a D-entry if a fact changed. It
refuses to start while corpus/differential or the pins have uncommitted changes, so
the diff holds this bump only. Several tools can move in one bump: every fact belongs
to one tool's field, so the diff still shows which tool changed what. Fixture pins
are not tools and are bumped separately (refresh-public-strata.py), never in the same
change.

Without a value, a tool resolves its newest release at least --cooldown days old
(default 14, the same rule as the scripts' Python locks), never one older than the
pin. `all` does this for every tool except llvm. Everything but c2patool is read with
git (`ls-remote` for tags, a shallow fetch without file contents for dates), so no
API, token or rate limit is involved:
  uv, Bazelisk, ExifTool   release tags
  OpenSSL                  release tags on the pinned series (3.5.x stays 3.5.x)
  conformance, Credentio   the newest commit on the default branch
  c2patool                 crates.io, but only a version whose own Cargo.lock (the one
                           `cargo install --locked` builds) has c2pa on Halftone's
                           line, e.g. 0.91.x for c2pa 0.91. c2patool moves with
                           Halftone's c2pa: another line compares two library
                           versions, so it needs --c2pa-upgrade. An explicit version
                           is checked the same way.
  llvm                     never resolved: it follows the Clang version Credentio asks for.

A hold keeps a pin where it is, with the reason written next to it in the pins file:

    uv run scripts/references.py hold openssl 3.5 LTS, the series the strata were collected with
    uv run scripts/references.py unhold openssl

`bump all` passes over a held tool and says why; `bump openssl` alone refuses; a value
given for it (`bump openssl=3.5.10`) moves it and keeps the hold. A move to an older
version is marked as a downgrade in the summary and the suggested commit message.

Standard library only. Network: `build` (sources and packages), `collect` on the
first run (the scripts' packages into the uv volume), and `bump` without a value
(git remotes on GitHub and Gitiles, crates.io via curl). Never reads the clipboard.
Exit 0 ok, 1 a step failed, 2 usage or setup error.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from progress import step  # noqa: E402
from repo import ROOT, C, die, need  # noqa: E402

DOCKERFILE = ROOT / "docker" / "references.dockerfile"
PINS = ROOT / "docker" / "references.env"
IMAGE = "halftone-references"
UV_VOLUME = "halftone-references-uv"
DIFFERENTIAL_TEST = [
    "cargo", "test", "-p", "halftone-cli", "--features", "c2pa",
    "--test", "differential", "--", "--ignored", "--nocapture",
]

# tool name on the command line -> (pin key, what the value is)
TOOLS = {
    "uv": ("UV_VERSION", "uv release"),
    "exiftool": ("EXIFTOOL_VERSION", "ExifTool release (GitHub tag)"),
    "openssl": ("OPENSSL_VERSION", "OpenSSL release"),
    "c2patool": ("C2PATOOL_VERSION", "c2patool crate version"),
    "credentio": ("CREDENTIO_COMMIT", "Credentio commit"),
    "conformance": ("CONFORMANCE_COMMIT", "conformance-public commit (rubrics)"),
    "bazelisk": ("BAZELISK_VERSION", "Bazelisk release (builds Credentio)"),
    "llvm": ("LLVM_VERSION", "Clang/libc++ major version (builds Credentio)"),
}
# Build-only pins: they change how Credentio is built, not what any header records.
BUILD_ONLY = {"BAZELISK_VERSION", "LLVM_VERSION"}
# Commits are pinned in full: a short hash can become ambiguous as a repository grows.
COMMITS = {"CREDENTIO_COMMIT", "CONFORMANCE_COMMIT"}
FULL_SHA = re.compile(r"[0-9a-f]{40}")
DEFAULTS = {"BAZELISK_VERSION": "v1.28.1", "LLVM_VERSION": "19"}

PINS_HEADER = """\
# Reference tool pins for docker/references.dockerfile, one KEY=VALUE per line.
# Change them with `uv run scripts/references.py bump <tool> [value]`, which
# re-collects every stratum with the new image; never edit and commit by hand.
# Written by `references.py init` from the tools on the machine that collected
# the committed strata, so the first collection in the image changes no header.
"""
HOLDS_HEADER = """\
# Held pins: `bump` without a value (and `bump all`) leaves them where they are.
# Move one only by giving its value; `references.py unhold <tool>` lifts the hold.
# Holds are not build inputs: they never change the image or its tag.
"""


def hold_key(tool: str) -> str:
    return f"HOLD_{tool.upper()}"


# --- pins ------------------------------------------------------------------------


def read_entries() -> dict[str, str]:
    if not PINS.is_file():
        die(f"no {PINS.relative_to(ROOT)}; run `uv run scripts/references.py init` first", 2)
    pins: dict[str, str] = {}
    for n, line in enumerate(PINS.read_text().splitlines(), 1):
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, sep, value = line.partition("=")
        if not sep or not key.strip() or not value.strip():
            die(f"{PINS.relative_to(ROOT)}:{n}: expected KEY=VALUE, got {line!r}", 2)
        pins[key.strip()] = value.strip()
    return pins


def read_holds() -> dict[str, str]:
    """tool -> why it is held."""
    if not PINS.is_file():
        return {}
    entries = read_entries()
    holds = {tool: entries[hold_key(tool)] for tool in TOOLS if hold_key(tool) in entries}
    stray = [k for k in entries if k.startswith("HOLD_") and k not in {hold_key(t) for t in TOOLS}]
    if stray:
        die(f"{PINS.relative_to(ROOT)}: {', '.join(stray)} names no tool", 2)
    return holds


def read_pins() -> dict[str, str]:
    """The build inputs: every pin, without the holds."""
    pins = {k: v for k, v in read_entries().items() if not k.startswith("HOLD_")}
    missing = [k for k, _ in TOOLS.values() if k not in pins]
    if missing:
        die(f"{PINS.relative_to(ROOT)} lacks {', '.join(missing)}", 2)
    short = [k for k in COMMITS if not FULL_SHA.fullmatch(pins[k])]
    if short:
        die(f"{PINS.relative_to(ROOT)}: {', '.join(short)} must be a full 40-character commit", 2)
    return pins


def write_pins(pins: dict[str, str], holds: dict[str, str] | None = None) -> None:
    """Write every pin, keeping the holds already in the file unless `holds` is given."""
    holds = read_holds() if holds is None else holds
    lines = [PINS_HEADER]
    for key, what in TOOLS.values():
        lines.append(f"# {what}\n{key}={pins[key]}\n")
    if holds:
        lines.append(HOLDS_HEADER + "".join(f"{hold_key(t)}={holds[t]}\n" for t in TOOLS if t in holds))
    PINS.write_text("\n".join(lines))


def tag(pins: dict[str, str]) -> str:
    """Content tag: the pins and the Dockerfile. Same inputs, same tag."""
    h = hashlib.sha256()
    for key in sorted(pins):
        h.update(f"{key}={pins[key]}\n".encode())
    h.update(DOCKERFILE.read_bytes())
    return h.hexdigest()[:12]


# --- versions, read the same way on the host and in the image -------------------


def output(cmd: list[str], cwd: Path | None = None) -> str | None:
    if shutil.which(cmd[0]) is None:
        return None
    r = subprocess.run(cmd, capture_output=True, text=True, cwd=cwd)
    return (r.stdout or r.stderr).strip() if r.returncode == 0 else None


def first(pattern: str, text: str | None) -> str | None:
    m = re.search(pattern, text or "", re.M)
    return m.group(1) if m else None


def credentio_name(path: Path) -> str | None:
    """The commit in `c2pa_validate-<commit>`, the name the symlink points to."""
    real = path.expanduser().resolve()
    if not real.is_file():
        return None
    return real.name.removeprefix("c2pa_validate-") if real.name.startswith("c2pa_validate-") else None


def host_versions() -> dict[str, str | None]:
    cred = Path(os.environ.get("CREDENTIO", "~/oss-bins/c2pa_validate"))
    rub = (ROOT / os.environ.get("RUBRICS", "../conformance-public")).resolve()
    return {
        "UV_VERSION": first(r"^uv (\S+)", output(["uv", "--version"])),
        "EXIFTOOL_VERSION": (output(["exiftool", "-ver"]) or "").strip() or None,
        "OPENSSL_VERSION": first(r"^OpenSSL (\S+)", output(["openssl", "version"])),
        "C2PATOOL_VERSION": first(r"(\d+\.\d+\.\d+\S*)", output(["c2patool", "--version"])),
        "CREDENTIO_COMMIT": credentio_name(cred),
        "CONFORMANCE_COMMIT": output(["git", "-C", str(rub), "rev-parse", "HEAD"]) if rub.is_dir() else None,
    }


IMAGE_PROBE = r"""
echo "UV_VERSION=$(uv --version | cut -d' ' -f2)"
echo "EXIFTOOL_VERSION=$(exiftool -ver)"
echo "OPENSSL_VERSION=$(openssl version | cut -d' ' -f2)"
echo "C2PATOOL_VERSION=$(c2patool --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+[^ ]*' | head -1)"
echo "CREDENTIO_COMMIT=$(basename "$(readlink -f "$CREDENTIO")" | sed 's/^c2pa_validate-//')"
echo "CONFORMANCE_COMMIT=$(git -C "$RUBRICS" rev-parse HEAD)"
"""


def image_versions(image: str) -> dict[str, str]:
    r = subprocess.run(["docker", "run", "--rm", image, "sh", "-c", IMAGE_PROBE], capture_output=True, text=True)
    if r.returncode != 0:
        die(f"could not read versions from {image}:\n{r.stderr.strip()}", 1)
    return dict(line.split("=", 1) for line in r.stdout.splitlines() if "=" in line)


def same(pin: str, got: str | None) -> bool:
    """Commits match on a prefix (a pin may be short or full), versions exactly."""
    if not got:
        return False
    if re.fullmatch(r"[0-9a-f]{7,40}", pin):
        return got.startswith(pin) or pin.startswith(got)
    return got == pin


# --- docker -----------------------------------------------------------------------


def docker_ready() -> None:
    need("docker")
    if subprocess.run(["docker", "info"], capture_output=True).returncode != 0:
        die("Docker is installed but not running (start Docker Desktop)", 2)


def image_exists(image: str) -> bool:
    return subprocess.run(["docker", "image", "inspect", image], capture_output=True).returncode == 0


def build(pins: dict[str, str], jobs: int, force: bool = False) -> str:
    image = f"{IMAGE}:{tag(pins)}"
    if image_exists(image) and not force:
        print(f"{C.GREEN}image{C.RESET}   {image} (already built for these pins)")
        return image
    print(f"{C.BOLD_CYAN}build{C.RESET}   {image}")
    print(f"{C.DIM}the first build compiles OpenSSL, c2patool and Credentio (Bazel): expect 20–40 min;")
    print(f"later builds reuse every stage whose pin did not change{C.RESET}")
    cmd = ["docker", "build", "-f", str(DOCKERFILE), "-t", image, "--build-arg", f"PINS_TAG={tag(pins)}",
           "--build-arg", f"BAZEL_JOBS={jobs}"]
    for key, value in pins.items():
        cmd += ["--build-arg", f"{key}={value}"]
    cmd.append(str(ROOT))
    env = {**os.environ, "DOCKER_BUILDKIT": "1"}
    if subprocess.run(cmd, env=env).returncode != 0:
        die("docker build failed (see above); the last stage named is the one that broke", 1)
    return image


def run_in(image: str, cmd: list[str]) -> int:
    """Run cmd in the image against this checkout, as the calling user."""
    tty = ["-t"] if sys.stdin.isatty() and sys.stdout.isatty() else []
    env: list[str] = []
    for name in ("TERM", "NO_COLOR", "CI"):
        if name in os.environ:
            env += ["-e", name]
    full = ["docker", "run", "--rm", "-i", *tty, "--user", f"{os.getuid()}:{os.getgid()}",
            "-v", f"{ROOT}:/work", "-w", "/work", "-v", f"{UV_VOLUME}:/cache", *env, image, *cmd]
    return subprocess.run(full).returncode


def built_image(pins: dict[str, str]) -> str:
    image = f"{IMAGE}:{tag(pins)}"
    if not image_exists(image):
        die(f"{image} is not built for the current pins; run `uv run scripts/references.py build`", 2)
    return image


# --- newest releases (bump without a value) ---------------------------------------
#
# Every resolver applies the same rule: the newest release at least --cooldown days
# old, never one older than the pin. Everything but c2patool comes from git itself:
# `git ls-remote` lists the tags, and a shallow fetch without file contents (a few
# hundred kB) dates them. No API, no token, no rate limit. c2patool comes from
# crates.io, which says which c2pa each release locks.

CREDENTIO_REPO = "https://mediaprovenance.googlesource.com/credentio"
CONFORMANCE_REPO = "https://github.com/c2pa-org/conformance-public"
CRATES = "https://crates.io/api/v1/crates"
CRATE_FILES = "https://static.crates.io/crates"
# crates.io asks API clients for a user agent that says who they are.
USER_AGENT = "halftone scripts/references.py (https://github.com/gitbadger)"
MAX_LOOKUPS = 10  # per tool: the newest candidates dated, or crate downloads, before giving up
GIT_ENV = {**os.environ, "GIT_TERMINAL_PROMPT": "0"}  # never ask for credentials


def get(url: str) -> tuple[bool, str, bytes, str]:
    """GET with curl (the system's certificates, which a uv-managed Python on macOS may
    not find): (ok, HTTP status, body, curl's error)."""
    need("curl")
    r = subprocess.run(["curl", "-sSL", "--fail-with-body", "-A", USER_AGENT, "-w", "\n%{http_code}", url],
                       capture_output=True)
    body, _, code = r.stdout.rpartition(b"\n")
    return r.returncode == 0, code.decode(), body, r.stderr.decode(errors="replace").strip()


def fetch(url: str, status: dict[str, str] | None = None) -> bytes:
    """`status` maps an HTTP status to the message to exit with (2); any other failure exits 1."""
    ok, code, body, err = get(url)
    if ok:
        return body
    if status and code in status:
        die(status[code], 2)
    die(f"{url}: {(err or body.decode(errors='replace')).strip()[:200] or 'HTTP ' + code}", 1)


class Remote:
    """A throwaway bare repository for asking one remote about its tags and history."""

    def __init__(self, url: str):
        need("git")
        self.url = url
        self.dir = tempfile.TemporaryDirectory(prefix="references-")
        self.git("init", "-q", "--bare")
        self.git("remote", "add", "origin", url)

    def git(self, *args: str) -> str:
        r = subprocess.run(["git", "-C", self.dir.name, *args], capture_output=True, text=True, env=GIT_ENV)
        if r.returncode != 0:
            die(f"git {args[0]} {self.url}: {r.stderr.strip()[:300]}", 1)
        return r.stdout

    def tags(self) -> list[str]:
        out = self.git("ls-remote", "--tags", "--refs", "origin")
        return [line.split("refs/tags/", 1)[1] for line in out.splitlines() if "refs/tags/" in line]

    def tag_dates(self, tags: list[str]) -> dict[str, datetime]:
        """Committer date of each tag's commit: one shallow fetch, commits and trees only."""
        if not tags:
            return {}
        self.git("fetch", "-q", "--depth=1", "--filter=blob:none", "--no-tags", "origin",
                 *(f"refs/tags/{t}:refs/tags/{t}" for t in tags))
        return {t: utc(self.git("log", "-1", "--format=%cI", f"refs/tags/{t}").strip()) for t in tags}

    def log(self, n: int = 100) -> list[tuple[str, datetime]]:
        """The newest `n` commits on the default branch, newest first."""
        self.git("fetch", "-q", f"--depth={n}", "--filter=blob:none", "--no-tags", "origin", "HEAD")
        out = self.git("log", "--format=%H %cI", "FETCH_HEAD")
        return [(sha, utc(when)) for sha, when in (line.split() for line in out.splitlines())]


def utc(text: str) -> datetime:
    return datetime.fromisoformat(text.replace("Z", "+00:00"))


def age(when: datetime) -> str:
    days = (datetime.now(timezone.utc) - when).days
    return f"{when:%Y-%m-%d}, {days} day{'s' if days != 1 else ''} ago"


def vkey(version: str) -> tuple[int, ...]:
    return tuple(int(x) for x in re.findall(r"\d+", version.split("-")[0])[:3])


def newest_version(label: str, current: str, versions: dict[str, datetime], a: argparse.Namespace,
                   accept: Callable[[str], str | None] | None = None) -> str | None:
    """The newest of `versions` past the cooldown and newer than `current`. `accept`
    returns a reason to skip a version, or None to take it."""
    cutoff = datetime.now(timezone.utc) - timedelta(days=a.cooldown)
    newer = sorted((v for v in versions if vkey(v) > vkey(current)), key=vkey, reverse=True)
    if not newer:
        print(f"{C.GREEN}current{C.RESET} {label} {current} is the newest release")
        return None
    young: list[str] = []
    for lookups, version in enumerate(newer, 1):
        if lookups > MAX_LOOKUPS:
            break
        if versions[version] > cutoff:
            young.append(version)
            continue
        if accept and (reason := accept(version)):
            print(f"{C.DIM}skip    {label} {version}: {reason}{C.RESET}")
            continue
        print(f"{C.BOLD_CYAN}newest{C.RESET}  {label} {version} ({age(versions[version])})")
        if young:
            print(f"{C.DIM}        skipped, under {a.cooldown} days old: {', '.join(young)}{C.RESET}")
        return version
    why = f"; under {a.cooldown} days old: {', '.join(young)}" if young else ""
    print(f"{C.GREEN}current{C.RESET} {label} {current} is the newest release that fits{why}")
    return None


def git_versions(url: str, pattern: str, current: str,
                 keep: Callable[[str], bool] = lambda v: True) -> dict[str, datetime]:
    """Versions from tags matching `pattern` (group 1 is the version) that are newer
    than `current` and pass `keep`, the newest MAX_LOOKUPS of them, dated."""
    remote = Remote(url)
    tagged = {m.group(1): tag for tag in remote.tags() if (m := re.fullmatch(pattern, tag))}
    newer = sorted((v for v in tagged if vkey(v) > vkey(current) and keep(v)), key=vkey, reverse=True)
    dates = remote.tag_dates([tagged[v] for v in newer[:MAX_LOOKUPS]])
    return {v: dates[tagged[v]] for v in newer[:MAX_LOOKUPS]}


def newest_commit(label: str, current: str, log: list[tuple[str, datetime]], a: argparse.Namespace,
                  compare: str) -> str | None:
    """The newest commit in `log` (newest first) past the cooldown, if newer than `current`.
    `compare` is a URL with {old} and {new} for reviewing the range."""
    cutoff = datetime.now(timezone.utc) - timedelta(days=a.cooldown)
    younger = 0
    for i, (commit, when) in enumerate(log):
        if commit == current:
            note = f"; {younger} newer commit{'s' if younger != 1 else ''} not yet {a.cooldown} days old" if younger else ""
            print(f"{C.GREEN}current{C.RESET} {label} {current[:7]} is the newest commit old enough{note}")
            return None
        if when > cutoff:
            younger += 1
            continue
        behind = next((j - i for j, (c, _) in enumerate(log) if c == current), None)
        count = (f"{behind} commit{'s' if behind != 1 else ''} after {current[:7]}" if behind
                 else f"more than {len(log) - i} commits after {current[:7]}")
        print(f"{C.BOLD_CYAN}newest{C.RESET}  {label} {commit[:7]} ({age(when)}), {count}")
        if younger:
            print(f"{C.DIM}        {younger} newer commit{'s' if younger != 1 else ''} skipped: under {a.cooldown} days old{C.RESET}")
        print(f"{C.DIM}        changes: {compare.format(old=current, new=commit)}{C.RESET}")
        return commit
    die(f"no commit in {label}'s last {len(log)} is both {a.cooldown} days old and newer than {current[:7]}", 1)


def newest_credentio(current: str, a: argparse.Namespace) -> str | None:
    return newest_commit("Credentio", current, Remote(CREDENTIO_REPO).log(), a,
                         CREDENTIO_REPO + "/+log/{old}..{new}")


def newest_conformance(current: str, a: argparse.Namespace) -> str | None:
    return newest_commit("conformance-public", current, Remote(CONFORMANCE_REPO).log(), a,
                         CONFORMANCE_REPO + "/compare/{old}...{new}")


def newest_uv(current: str, a: argparse.Namespace) -> str | None:
    return newest_version("uv", current, git_versions("https://github.com/astral-sh/uv", r"(\d+\.\d+\.\d+)", current), a)


def newest_bazelisk(current: str, a: argparse.Namespace) -> str | None:
    bare = current.removeprefix("v")
    found = newest_version("Bazelisk", bare,
                           git_versions("https://github.com/bazelbuild/bazelisk", r"v(\d+\.\d+\.\d+)", bare), a)
    return f"v{found}" if found else None


def newest_exiftool(current: str, a: argparse.Namespace) -> str | None:
    return newest_version("ExifTool", current,
                          git_versions("https://github.com/exiftool/exiftool", r"(\d+\.\d+)", current), a)


def series(version: str) -> tuple[int, ...]:
    return vkey(version)[:2]


def newest_openssl(current: str, a: argparse.Namespace) -> str | None:
    """The newest patch on the pinned series; a newer series is only ever given by hand."""
    found = newest_version("OpenSSL", current,
                           git_versions("https://github.com/openssl/openssl", r"openssl-(\d+\.\d+\.\d+)", current,
                                        keep=lambda v: series(v) == series(current)), a)
    return found


# c2patool: crates.io, on Halftone's c2pa line ----------------------------------------

def c2pa_line(version: str) -> str:
    return ".".join(version.split(".")[:2])


def halftone_c2pa() -> str:
    lock = ROOT / "Cargo.lock"
    if not lock.is_file():
        die("no Cargo.lock at the repo root", 2)
    found = [p["version"] for p in tomllib.loads(lock.read_text()).get("package", []) if p["name"] == "c2pa"]
    if not found:
        die("Cargo.lock has no c2pa package", 2)
    return max(found, key=vkey)


def c2patool_c2pa(version: str) -> str:
    """The c2pa version in c2patool's published Cargo.lock, which `cargo install
    --locked` builds: what the collector's c2patool actually runs."""
    missing = f"no c2patool {version} on crates.io"
    data = fetch(f"{CRATE_FILES}/c2patool/c2patool-{version}.crate",
                 status={"403": missing, "404": missing})  # static.crates.io answers 403 for a missing file
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as t:
        member = t.extractfile(f"c2patool-{version}/Cargo.lock")
        if member is None:
            die(f"c2patool {version} was published without a Cargo.lock", 1)
        lock = tomllib.loads(member.read().decode())
    found = [p["version"] for p in lock.get("package", []) if p["name"] == "c2pa"]
    if not found:
        die(f"c2patool {version}'s Cargo.lock has no c2pa package", 1)
    return found[0]


def c2patool_fits(version: str, ours: str, a: argparse.Namespace) -> tuple[bool, str]:
    """Whether this c2patool may be pinned without --c2pa-upgrade, and the c2pa it locks."""
    theirs = c2patool_c2pa(version)
    return a.c2pa_upgrade or c2pa_line(theirs) == c2pa_line(ours), theirs


def report_c2pa(version: str, theirs: str, ours: str) -> None:
    print(f"        c2patool {version} locks c2pa {theirs}; Halftone's Cargo.lock has c2pa {ours}")
    if theirs != ours:
        print(f"{C.YELLOW}        note{C.RESET}: the differential compares c2pa {theirs} (c2patool) with {ours} "
              f"(Halftone); a disagreement can be a c2pa difference, not a Halftone one")


def newest_c2patool(current: str, a: argparse.Namespace) -> str | None:
    ours = halftone_c2pa()
    data = json.loads(fetch(f"{CRATES}/c2patool"))
    versions = {v["num"]: utc(v["created_at"]) for v in data["versions"]
                if not v["yanked"] and "-" not in v["num"]}
    locked: dict[str, str] = {}

    def accept(version: str) -> str | None:
        fits, locked[version] = c2patool_fits(version, ours, a)
        return None if fits else (f"locks c2pa {locked[version]}, not Halftone's c2pa {c2pa_line(ours)} line "
                                  "(--c2pa-upgrade takes it)")

    found = newest_version("c2patool", current, versions, a, accept=accept)
    if found:
        report_c2pa(found, locked[found], ours)
    return found


RESOLVERS = {
    "uv": newest_uv,
    "exiftool": newest_exiftool,
    "openssl": newest_openssl,
    "c2patool": newest_c2patool,
    "credentio": newest_credentio,
    "conformance": newest_conformance,
    "bazelisk": newest_bazelisk,
}
# Tools without a resolver, and why.
NO_RESOLVER = {
    "llvm": "llvm follows the Clang version Credentio's README asks for (19), not the newest "
            "release; give it a value when Credentio moves",
}


# --- commands ---------------------------------------------------------------------


def cmd_init(a: argparse.Namespace) -> int:
    if PINS.exists() and not a.force:
        die(f"{PINS.relative_to(ROOT)} exists; `bump` changes a pin (or pass --force to rewrite all)", 2)
    host = host_versions()
    short = host.get("CREDENTIO_COMMIT")
    if a.credentio_commit:
        if not FULL_SHA.fullmatch(a.credentio_commit):
            die("--credentio-commit must be the full 40-character commit", 2)
        if short and not a.credentio_commit.startswith(short):
            die(f"--credentio-commit {a.credentio_commit} is not the commit this machine's "
                f"Credentio was built from (c2pa_validate-{short})", 2)
        host["CREDENTIO_COMMIT"] = a.credentio_commit
    elif short and not FULL_SHA.fullmatch(short):
        die(f"this machine's Credentio is named c2pa_validate-{short}, a short commit; pass the full one: "
            f"--credentio-commit <sha> (git -C <your credentio checkout> rev-parse {short})", 2)
    missing = [k for k, v in host.items() if not v]
    if missing:
        die("could not read from this machine: " + ", ".join(missing)
            + " (is the tool on PATH; are CREDENTIO and RUBRICS right?)", 2)
    rub = (ROOT / os.environ.get("RUBRICS", "../conformance-public")).resolve()
    if output(["git", "-C", str(rub), "status", "--porcelain"]):
        print(f"{C.YELLOW}warning{C.RESET} {rub} has local changes; the image gets the commit without them")
    holds = read_holds()
    write_pins({**DEFAULTS, **host}, holds)  # type: ignore[arg-type]
    print(f"wrote {PINS.relative_to(ROOT)}:")
    for key, value in read_pins().items():
        print(f"  {key:<20} {value}")
    for tool, why in holds.items():
        print(f"  {C.YELLOW}held{C.RESET}  {tool}: {why}")
    print("\nnext: uv run scripts/references.py build")
    return 0


def cmd_show(a: argparse.Namespace) -> int:
    pins = read_pins()
    image = f"{IMAGE}:{tag(pins)}"
    host = host_versions()
    if shutil.which("docker") is None:
        have, state = False, "Docker not installed"
    elif subprocess.run(["docker", "info"], capture_output=True).returncode != 0:
        have, state = False, "Docker not running, image state unknown"
    else:
        have = image_exists(image)
        state = "built" if have else "not built"
    inside = image_versions(image) if have else {}
    print(f"image  {image} {'(' + state + ')' if have else f'{C.YELLOW}({state}){C.RESET}'}\n")
    print(f"  {'pin':<20} {'pinned':<16} {'this machine':<16} {'image':<16}")
    for key, value in pins.items():
        if key in BUILD_ONLY:
            print(f"  {key:<20} {value[:14]:<16} {C.DIM}{'(build only)':<16} {'':<16}{C.RESET}")
            continue
        h, i = host.get(key), inside.get(key)
        hc = C.GREEN if same(value, h) else C.YELLOW
        ic = (C.GREEN if same(value, i) else C.RED) if have else C.DIM
        print(f"  {key:<20} {value[:14]:<16} {hc}{(h or 'missing')[:14]:<16}{C.RESET} "
              f"{ic}{(i or '-')[:14]:<16}{C.RESET}")
    holds = read_holds()
    if holds:
        print()
        for tool, why in holds.items():
            print(f"  {C.YELLOW}held{C.RESET}  {tool} at {pins[TOOLS[tool][0]]}: {why}")
    print(f"\n{C.DIM}yellow: this machine differs from the pin, so collecting here (not in the image)"
          f" would change headers.\nred: the image differs from its own pin; rebuild with --force.{C.RESET}")
    return 1 if have and any(not same(v, inside.get(k)) for k, v in pins.items() if k not in BUILD_ONLY) else 0


def cmd_build(a: argparse.Namespace) -> int:
    docker_ready()
    image = build(read_pins(), a.jobs, force=a.force)
    print(f"\n{C.GREEN}ready{C.RESET}   {image}")
    print("check:   uv run scripts/references.py show")
    return 0


def cmd_run(a: argparse.Namespace) -> int:
    docker_ready()
    cmd = a.cmd[1:] if a.cmd[:1] == ["--"] else a.cmd
    if not cmd:
        die("name a command: references.py run -- bash", 2)
    return run_in(built_image(read_pins()), cmd)


def cmd_collect(a: argparse.Namespace) -> int:
    docker_ready()
    args = a.args[1:] if a.args[:1] == ["--"] else a.args
    rc = run_in(built_image(read_pins()), ["uv", "run", "scripts/collect.py", *args])
    if rc == 0:
        print(f"\n{C.DIM}then, on the host: {' '.join(DIFFERENTIAL_TEST)}{C.RESET}")
    return rc


def cmd_hold(a: argparse.Namespace) -> int:
    if a.tool not in TOOLS:
        die(f"unknown tool {a.tool!r}; one of: {', '.join(TOOLS)}", 2)
    why = " ".join(a.reason).strip()
    if not why:
        die("say why it is held: references.py hold openssl 3.5 LTS, the series the strata were collected with", 2)
    pins, holds = read_pins(), read_holds()
    was = holds.get(a.tool)
    holds[a.tool] = why
    write_pins(pins, holds)
    pin = pins[TOOLS[a.tool][0]]
    print(f"{C.YELLOW}held{C.RESET}    {a.tool} at {pin}: {why}" + (f"\n        (was: {was})" if was else ""))
    subject = f"refs: hold {a.tool} at {short(TOOLS[a.tool][0], pin)}"
    print(f"commit it on its own:\n  git commit -m {shlex.quote(subject)} -m {shlex.quote(why)} "
          f"{PINS.relative_to(ROOT)}")
    return 0


def cmd_unhold(a: argparse.Namespace) -> int:
    holds = read_holds()
    if a.tool not in holds:
        die(f"{a.tool} is not held" + (f"; held: {', '.join(holds)}" if holds else ""), 2)
    why = holds.pop(a.tool)
    write_pins(read_pins(), holds)
    print(f"{C.GREEN}unheld{C.RESET}  {a.tool} (was: {why}); `bump {a.tool}` moves it again")
    return 0


def parse_targets(items: list[str]) -> list[tuple[str, str | None, bool]]:
    """`credentio c2patool`, `credentio=<sha> c2patool`, or the older `credentio <sha>`:
    a word that is not a tool name is the value of the tool before it. `all` adds every
    tool with a resolver that is not named otherwise. Each target is (tool, value or
    None, whether it was named rather than added by `all`)."""
    targets: list[list] = []
    everything = "all" in items
    for item in (i for i in items if i != "all"):
        tool, sep, value = item.partition("=")
        if sep:
            if tool not in TOOLS:
                die(f"unknown tool {tool!r}; one of: {', '.join(TOOLS)}", 2)
            targets.append([tool, value or None])
        elif item in TOOLS:
            targets.append([item, None])
        elif targets and targets[-1][1] is None:
            targets[-1][1] = item
        else:
            die(f"unknown tool {item!r}; one of: {', '.join(TOOLS)}", 2)
    seen = [t for t, _ in targets]
    if dupes := sorted({t for t in seen if seen.count(t) > 1}):
        die(f"{', '.join(dupes)} named twice", 2)
    if everything:
        targets += [[t, None, False] for t in RESOLVERS if t not in seen]
        targets.sort(key=lambda t: list(TOOLS).index(t[0]))
    return [(t[0], t[1], len(t) == 2) for t in targets]


def resolve(tool: str, value: str | None, pins: dict[str, str], a: argparse.Namespace,
            holds: dict[str, str], named: bool) -> str | None:
    """The new pin for one tool, or None when it stays where it is. A held tool moves
    only to a value given for it; `all` passes over it, naming it alone is an error."""
    key, _ = TOOLS[tool]
    if value is None and tool in holds:
        if named:
            die(f"{tool} is held at {pins[key]}: {holds[tool]}\n"
                f"give a version to move it anyway (the hold stays), or `references.py unhold {tool}`", 2)
        print(f"{C.YELLOW}held{C.RESET}    {tool} {short(key, pins[key])}: {holds[tool]}")
        return None
    if value is not None and tool in holds:
        print(f"{C.YELLOW}held{C.RESET}    {tool}: moving to {value} as given; the hold stays ({holds[tool]})")
    if value is None:
        if tool not in RESOLVERS:
            die(NO_RESOLVER.get(tool, f"{tool} needs a value"), 2)
        return RESOLVERS[tool](pins[key], a)
    if key in COMMITS and not FULL_SHA.fullmatch(value):
        die(f"{tool} is pinned by its full 40-character commit, not {value!r}", 2)
    if tool == "c2patool":
        ours = halftone_c2pa()
        fits, theirs = c2patool_fits(value, ours, a)
        if not fits:
            die(f"c2patool {value} locks c2pa {theirs}, Halftone has c2pa {ours}: a different c2pa line "
                "compares two library versions. Bump it as part of the c2pa upgrade, with --c2pa-upgrade.", 2)
        report_c2pa(value, theirs, ours)
    if pins[key] == value:
        print(f"{C.GREEN}current{C.RESET} {tool} is already {value}")
        return None
    return value


def short(key: str, value: str) -> str:
    return value[:7] if key in COMMITS else value


def cmd_bump(a: argparse.Namespace) -> int:
    targets = parse_targets(a.targets)
    pins, holds = read_pins(), read_holds()
    changes: dict[str, tuple[str, str]] = {}  # pin key -> (old, new), in the order named
    for tool, value, named in targets:
        new = resolve(tool, value, pins, a, holds, named)
        if new is not None:
            key, _ = TOOLS[tool]
            changes[key] = (pins[key], new)
    if not changes:
        print("\nnothing to bump")
        return 0
    names = {key: tool for tool, (key, _) in TOOLS.items()}
    def moved(k: str, old: str, new: str) -> str:
        down = k not in COMMITS and vkey(new) < vkey(old)
        return f"{names[k]} {short(k, old)} -> {short(k, new)}" + (" (downgrade)" if down else "")

    summary = ", ".join(moved(k, old, new) for k, (old, new) in changes.items())
    if a.dry_run:
        print(f"\ndry run: would bump {summary}; nothing changed")
        return 0
    dirty = output(["git", "status", "--porcelain", "--", "corpus/differential", str(PINS.relative_to(ROOT))],
                   cwd=ROOT)
    if dirty and not a.force:
        die("corpus/differential or the pins have uncommitted changes; commit or stash them so the "
            f"diff shows this bump only (or --force):\n{dirty}", 2)
    docker_ready()
    need("cargo", "git")

    before = dict(pins)
    for key, (_, new) in changes.items():
        pins[key] = new
    write_pins(pins)
    print(f"\n{C.BOLD_CYAN}bump{C.RESET}    {summary}")
    try:
        image = build(pins, a.jobs)
    except SystemExit:
        write_pins(before)
        print(f"{C.YELLOW}restored{C.RESET} {PINS.relative_to(ROOT)}", file=sys.stderr)
        raise
    if set(changes) <= BUILD_ONLY:
        print("\nthese pins only change how Credentio is built; re-collecting checks the binary is unchanged.")

    print(f"\n{C.BOLD_CYAN}collect{C.RESET} every stratum, in {image}")
    if run_in(image, ["uv", "run", "scripts/collect.py"]) != 0:
        print(f"{C.RED}collect failed or skipped a stratum{C.RESET}; the new pins stay for you to inspect")
        return 1

    print(f"\n{C.BOLD_CYAN}test{C.RESET}    {' '.join(DIFFERENTIAL_TEST)}")
    with step("differential test (host)", summary=False):
        test = subprocess.run(DIFFERENTIAL_TEST, cwd=ROOT, capture_output=True, text=True)
    for line in (test.stdout + test.stderr).splitlines():
        if line.startswith("differential") or "test result" in line or "panicked" in line:
            print(f"  {line}")

    print(f"\n{C.BOLD_CYAN}diff{C.RESET}")
    subprocess.run(["git", "diff", "--stat", "--", "corpus/differential", str(PINS.relative_to(ROOT))], cwd=ROOT)
    verdict = C.GREEN + "passed" if test.returncode == 0 else C.RED + "FAILED"
    several = ("\nSeveral tools moved: each fact belongs to one tool's field, so attribute every changed "
               "entry to the tool that\nwrote it, one D-entry per tool.") if len(changes) > 1 else ""
    if {"CREDENTIO_COMMIT", "CONFORMANCE_COMMIT"} <= set(changes):
        several += (f"\n{C.YELLOW}Credentio and conformance-public both moved{C.RESET}: they feed the same rubric "
                    "fields. If a rubric result\nchanged, find which one with `references.py run` against the old "
                    "image, or bump them one at a time.")
    print(f"\ndifferential test {verdict}{C.RESET}. Nothing is committed. Review the diff: header-only "
          f"changes are a clean bump;\na changed file entry needs a D-entry.{several} Suggested message:\n"
          f"  refs: {summary}; <no fact changes | D-0NN>")
    return 0 if test.returncode == 0 else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0],
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)

    p = sub.add_parser("init", help="write docker/references.env from this machine's tools")
    p.add_argument("--force", action="store_true", help="rewrite an existing pins file")
    p.add_argument("--credentio-commit", metavar="SHA",
                   help="Credentio's full commit; needed when the binary's name carries a short one")
    p.set_defaults(fn=cmd_init)

    p = sub.add_parser("show", help="pins vs this machine vs the image")
    p.set_defaults(fn=cmd_show)

    p = sub.add_parser("build", help="build the image for the current pins")
    p.add_argument("--jobs", type=int, default=4, help="Bazel jobs for Credentio (default 4; lower if Docker runs out of memory)")
    p.add_argument("--force", action="store_true", help="rebuild even if the tag exists")
    p.set_defaults(fn=cmd_build)

    p = sub.add_parser("run", help="run a command in the image: run -- bash")
    p.add_argument("cmd", nargs=argparse.REMAINDER)
    p.set_defaults(fn=cmd_run)

    p = sub.add_parser("collect", help="collect.py inside the image; arguments pass through")
    p.add_argument("args", nargs=argparse.REMAINDER)
    p.set_defaults(fn=cmd_collect)

    p = sub.add_parser("hold", help="keep a tool's pin where it is: bump without a value passes over it")
    p.add_argument("tool", help=", ".join(TOOLS))
    p.add_argument("reason", nargs="+", help="why, recorded next to the pin")
    p.set_defaults(fn=cmd_hold)

    p = sub.add_parser("unhold", help="let bump move a held tool again")
    p.add_argument("tool", help=", ".join(TOOLS))
    p.set_defaults(fn=cmd_unhold)

    p = sub.add_parser("bump", help="change one pin, rebuild, re-collect everything, test, show the diff")
    p.add_argument("targets", nargs="+", metavar="TOOL[=VALUE]",
                   help=f"`all`, or one or more of: {', '.join(TOOLS)}. A value is a version or full "
                        "commit (`tool=value`, or `tool value` for one tool); without one, the newest "
                        "release past the cooldown (every tool but llvm)")
    p.add_argument("--cooldown", type=int, default=14, metavar="DAYS",
                   help="without a value, only releases at least this old (default 14)")
    p.add_argument("--dry-run", action="store_true", help="show what the bump would be, change nothing")
    p.add_argument("--c2pa-upgrade", action="store_true",
                   help="allow a c2patool on another c2pa line than Halftone's (part of a c2pa upgrade)")
    p.add_argument("--jobs", type=int, default=4, help="Bazel jobs, as for build")
    p.add_argument("--force", action="store_true", help="start even with uncommitted corpus or pin changes")
    p.set_defaults(fn=cmd_bump)

    a = ap.parse_args()
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
