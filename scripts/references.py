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
    uv run scripts/references.py bump credentio <commit>

The image holds ExifTool, OpenSSL, c2patool, Credentio's c2pa_validate, a
conformance-public checkout, uv and Python, each at the version in
docker/references.env. Its tag is derived from the pins and the Dockerfile, so a
changed pin is a new image. The same pins give the same reference versions, the ones
the headers record; a rebuild is not bit-for-bit identical, since the base images,
apt packages, LLVM point release and Python patch release float. Collection runs in the
container against this checkout (bind-mounted at /work, as your user), with uv's
cache in the named volume halftone-references-uv. ht and `cargo test` stay on the host.

`bump <tool> <value>` is the manual tool update: it edits the pin, builds, re-collects
every stratum in the image, runs the differential test on the host and prints the
diff. It never commits; review the diff and write a D-entry if a fact changed. It
refuses to start while corpus/differential or the pins have uncommitted changes, so
the diff holds this bump only. A c2patool bump needs --c2pa-upgrade: c2patool moves
with Halftone's c2pa, as part of the upgrade procedure. Fixture pins are not tools
and are bumped separately (refresh-public-strata.py), never in the same change.

Standard library only. Network: `build` (sources and packages), and `collect` on the
first run (the scripts' packages into the uv volume). Never reads the clipboard.
Exit 0 ok, 1 a step failed, 2 usage or setup error.
"""

from __future__ import annotations

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
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
# Change them with `uv run scripts/references.py bump <tool> <value>`, which
# re-collects every stratum with the new image; never edit and commit by hand.
# Written by `references.py init` from the tools on the machine that collected
# the committed strata, so the first collection in the image changes no header.
"""


# --- pins ------------------------------------------------------------------------


def read_pins() -> dict[str, str]:
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
    missing = [k for k, _ in TOOLS.values() if k not in pins]
    if missing:
        die(f"{PINS.relative_to(ROOT)} lacks {', '.join(missing)}", 2)
    short = [k for k in COMMITS if not FULL_SHA.fullmatch(pins[k])]
    if short:
        die(f"{PINS.relative_to(ROOT)}: {', '.join(short)} must be a full 40-character commit", 2)
    return pins


def write_pins(pins: dict[str, str]) -> None:
    lines = [PINS_HEADER]
    for key, what in TOOLS.values():
        lines.append(f"# {what}\n{key}={pins[key]}\n")
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
    write_pins({**DEFAULTS, **host})  # type: ignore[arg-type]
    print(f"wrote {PINS.relative_to(ROOT)}:")
    for key, value in read_pins().items():
        print(f"  {key:<20} {value}")
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


def cmd_bump(a: argparse.Namespace) -> int:
    if a.tool not in TOOLS:
        die(f"unknown tool {a.tool!r}; one of: {', '.join(TOOLS)}", 2)
    key, what = TOOLS[a.tool]
    if a.tool == "c2patool" and not a.c2pa_upgrade:
        die("c2patool moves with Halftone's c2pa crate (0.28.x is c2pa 0.91): bumping it alone "
            "compares two library versions. Bump it as part of the c2pa upgrade, with --c2pa-upgrade.", 2)
    if key in COMMITS and not FULL_SHA.fullmatch(a.value):
        die(f"{a.tool} is pinned by its full 40-character commit, not {a.value!r}", 2)
    pins = read_pins()
    if pins[key] == a.value:
        die(f"{key} is already {a.value}", 2)
    dirty = output(["git", "status", "--porcelain", "--", "corpus/differential", str(PINS.relative_to(ROOT))],
                   cwd=ROOT)
    if dirty and not a.force:
        die("corpus/differential or the pins have uncommitted changes; commit or stash them so the "
            f"diff shows this bump only (or --force):\n{dirty}", 2)
    docker_ready()
    need("cargo", "git")

    old = pins[key]
    pins[key] = a.value
    write_pins(pins)
    print(f"{C.BOLD_CYAN}bump{C.RESET}    {what}: {old} -> {a.value}")
    try:
        image = build(pins, a.jobs)
    except SystemExit:
        pins[key] = old
        write_pins(pins)
        print(f"{C.YELLOW}restored{C.RESET} {key}={old}", file=sys.stderr)
        raise
    if key in BUILD_ONLY:
        print(f"\n{key} only changes how Credentio is built; re-collecting checks the binary is unchanged.")

    print(f"\n{C.BOLD_CYAN}collect{C.RESET} every stratum, in {image}")
    if run_in(image, ["uv", "run", "scripts/collect.py"]) != 0:
        print(f"{C.RED}collect failed or skipped a stratum{C.RESET}; the pin stays at {a.value} for you to inspect")
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
    print(f"\ndifferential test {verdict}{C.RESET}. Nothing is committed. Review the diff: header-only "
          f"changes are a clean bump;\na changed file entry needs a D-entry. Suggested message:\n"
          f"  refs: {a.tool} {old} -> {a.value}; <no fact changes | D-0NN>")
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

    p = sub.add_parser("bump", help="change one pin, rebuild, re-collect everything, test, show the diff")
    p.add_argument("tool", help=", ".join(TOOLS))
    p.add_argument("value", help="new version or commit")
    p.add_argument("--c2pa-upgrade", action="store_true", help="allow c2patool: this bump is part of a c2pa upgrade")
    p.add_argument("--jobs", type=int, default=4, help="Bazel jobs, as for build")
    p.add_argument("--force", action="store_true", help="start even with uncommitted corpus or pin changes")
    p.set_defaults(fn=cmd_bump)

    a = ap.parse_args()
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
