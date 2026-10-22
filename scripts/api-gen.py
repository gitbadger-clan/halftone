#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.12", "inquirer>=3.4", "httpx>=0.27", "rich>=13.7"]
# ///
"""Generate and edit corpus images through the Gemini and OpenAI APIs.

    uv run scripts/api-gen.py prompts                         # the prompt set, from PROMPTS.md
    uv run scripts/api-gen.py models gemini                   # image models this key can use
    uv run scripts/api-gen.py gen gemini -m gemini-3.1-flash-image -p p2 -p p3
    uv run scripts/api-gen.py edit gemini -m gemini-3.1-flash-image -p e1
    uv run scripts/api-gen.py gen openai -m chatgpt-image-latest -p p1 -f png -f jpeg -f webp
    uv run scripts/api-gen.py gen openai -m gpt-image-1.5 -p p3 --background transparent
    uv run scripts/api-gen.py edit openai -m chatgpt-image-latest -p e1 -p e2 -p e3
    uv run scripts/api-gen.py gen                             # no arguments: asks for everything

What it guarantees, so the files can stand in the corpus:
  - Prompts are read verbatim from corpus/differential/PROMPTS.md, never typed.
  - The file is the image exactly as the API delivered it: the base64 payload,
    decoded, checked against its declared type, written once. Nothing re-encodes it.
  - Names follow 04-generators/SOURCE.md:
    <gemini-api|openai-api>__<model>[-suffix]__api-<png|jpeg|webp>__<p1…e3>__<n>.<ext>,
    where n is the next free generation number for that generator, variant and
    prompt, starting at the prompt number (every API call is a new generation,
    so three formats of p1 become p1__1, p1__2, p1__3). --n pins a number; an
    existing file is then never overwritten unless --force, which moves it to
    /tmp/ht-replaced first, like ht-take -f.
  - Edits send the base photo's bytes unchanged (e1 photo_bowl, e2 photo_flask,
    e3 photo_path from 00-base), so the provider sees the camera's C2PA record.
  - Progress while it runs: a bar over all calls, a spinner and timer on the call
    in flight, the time of every call as it lands, and a summary table at the end
    (also after a failure, so what was written is never in doubt).
  - Every call is logged to target/api-gen/log.jsonl: time, duration, endpoint, model,
    prompt, parameters, the response's non-image parts (text, usage, finish
    reason, request id), the file written and its sha256. No key, no image data.
  - Defaults stay defaults, as PROMPTS.md asks: size, quality, compression and
    input fidelity are only sent when given, and then a --suffix is required so
    the non-default setting shows in the variant token.
  - The key comes from $GEMINI_API_KEY / $OPENAI_API_KEY, or else from the macOS
    Keychain (services gemini-api / openai-api). It is never printed, and the
    clipboard is never read.
"""

from __future__ import annotations

import base64
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Annotated

import httpx
import typer
from rich.console import Console
from rich.progress import BarColumn, Progress, SpinnerColumn, TextColumn, TimeElapsedColumn
from rich.table import Table

CONSOLE = Console(highlight=False)

APP = typer.Typer(add_completion=False, no_args_is_help=False, help=__doc__.splitlines()[0])

ROOT = Path(
    subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True).stdout.strip()
    or "."
)
DIFF = ROOT / "corpus/differential"
PROMPTS_MD = DIFF / "PROMPTS.md"
BASE = DIFF / "00-base"
LOG = ROOT / "target/api-gen/log.jsonl"
REPLACED = Path("/tmp/ht-replaced")
TIMEOUT = httpx.Timeout(300.0, connect=20.0)

EDIT_PHOTO = {"e1": "photo_bowl.jpg", "e2": "photo_flask.jpg", "e3": "photo_path.jpg"}
GENERATOR = {"gemini": "gemini-api", "openai": "openai-api"}
KEY_ENV = {"gemini": "GEMINI_API_KEY", "openai": "OPENAI_API_KEY"}
KEY_SERVICE = {"gemini": "gemini-api", "openai": "openai-api"}
EXT = {"image/png": "png", "image/jpeg": "jpg", "image/webp": "webp"}
PATH_TOKEN = {"png": "api-png", "jpg": "api-jpeg", "webp": "api-webp"}
MAGIC = {"png": b"\x89PNG\r\n\x1a\n", "jpg": b"\xff\xd8\xff", "webp": b"RIFF"}
OPENAI_FORMATS = {"png": "image/png", "jpeg": "image/jpeg", "webp": "image/webp"}
# OpenAI's reference: gpt-image-2 (and its dated snapshot) and gpt-image-1-mini do not
# support transparent backgrounds; transparency needs png or webp output.
OPENAI_NO_TRANSPARENT = ("gpt-image-2", "gpt-image-1-mini")


class Fail(Exception):
    """An error with a message meant for the user; printed without a traceback."""


# ---- prompts -------------------------------------------------------------------------
def load_prompts() -> dict[str, str]:
    """p1…p5 and e1…e3 from PROMPTS.md: a "P1 · …" / "E1 · …" line, then the prompt
    in the next fenced block, verbatim."""
    if not PROMPTS_MD.exists():
        raise Fail(f"{PROMPTS_MD} not found (run from inside the halftone repo)")
    text = PROMPTS_MD.read_text()
    found = re.findall(r"^([PE][1-9]) · [^\n]*\n```[^\n]*\n(.*?)\n```", text, re.M | re.S)
    prompts = {pid.lower(): body.strip() for pid, body in found}
    missing = [p for p in ("p1", "p2", "p3", "p4", "p5", "e1", "e2", "e3") if p not in prompts]
    if missing:
        raise Fail(f"PROMPTS.md: no prompt found for {', '.join(missing)}")
    return prompts


# ---- keys ----------------------------------------------------------------------------
def api_key(provider: str) -> str:
    key = os.environ.get(KEY_ENV[provider], "").strip()
    if not key and shutil.which("security"):
        r = subprocess.run(
            ["security", "find-generic-password", "-a", os.environ.get("USER", ""),
             "-s", KEY_SERVICE[provider], "-w"],
            capture_output=True, text=True,
        )
        key = r.stdout.strip()
    if not key:
        raise Fail(
            f"no {provider} key: set ${KEY_ENV[provider]}, or store it with\n"
            f"  security add-generic-password -a $USER -s {KEY_SERVICE[provider]} -w"
        )
    return key


# ---- naming and writing --------------------------------------------------------------
def slug(text: str) -> str:
    return re.sub(r"[^a-z0-9.]+", "-", text.lower()).strip("-")


def target_name(provider: str, model: str, suffix: str, ext: str, pid: str, n: int) -> str:
    variant = slug(model) + (f"-{slug(suffix)}" if suffix else "")
    return f"{GENERATOR[provider]}__{variant}__{PATH_TOKEN[ext]}__{pid}__{n}.{ext}"


def default_n(pid: str) -> int:
    return int(pid[1:])


def next_free_n(dest_dir: Path, provider: str, model: str, suffix: str, pid: str) -> int:
    """The SOURCE.md rule: n starts at the prompt number; a further generation of
    the same prompt (any path) takes the next free number."""
    variant = slug(model) + (f"-{slug(suffix)}" if suffix else "")
    used = set()
    for f in dest_dir.glob(f"{GENERATOR[provider]}__{variant}__*__{pid}__*"):
        m = re.search(rf"__{pid}__(\d+)\.[a-z]+$", f.name)
        if m:
            used.add(int(m.group(1)))
    n = default_n(pid)
    while n in used:
        n += 1
    return n


def write_exact(data: bytes, mime: str, dest_dir: Path, name: str, force: bool) -> Path:
    """Check the bytes against the declared type, then write them once, atomically."""
    ext = EXT.get(mime)
    if ext is None:
        raise Fail(f"unexpected image type {mime!r}; nothing written")
    if not data.startswith(MAGIC[ext]):
        raise Fail(f"payload declared {mime} but does not start like one; nothing written")
    dest = dest_dir / name
    for old in dest_dir.glob(Path(name).stem + ".*"):
        if not force:
            raise Fail(f"exists: {old.name} (use --force to replace it; the old copy goes to {REPLACED})")
        REPLACED.mkdir(parents=True, exist_ok=True)
        kept = REPLACED / f"{datetime.now():%Y%m%d-%H%M%S}-{old.name}"
        old.rename(kept)
        CONSOLE.print(f"  replaced: {old.name} (old copy: {kept})")
    fd, tmp = tempfile.mkstemp(dir=dest_dir, prefix=".api-gen-")
    with os.fdopen(fd, "wb") as f:
        f.write(data)
    os.replace(tmp, dest)
    return dest


def log_call(entry: dict) -> None:
    LOG.parent.mkdir(parents=True, exist_ok=True)
    with LOG.open("a") as f:
        f.write(json.dumps(entry, ensure_ascii=False) + "\n")


def strip_images(obj):
    """The response without image payloads, for the log."""
    if isinstance(obj, dict):
        return {
            k: ("<image data omitted>" if k in ("data", "b64_json") and isinstance(v, str) and len(v) > 200
                else strip_images(v))
            for k, v in obj.items()
        }
    if isinstance(obj, list):
        return [strip_images(v) for v in obj]
    return obj


# ---- providers -----------------------------------------------------------------------
GEMINI_URL = "https://generativelanguage.googleapis.com/v1beta"
OPENAI_URL = "https://api.openai.com/v1"


def gemini_call(key: str, model: str, prompt: str, photo: Path | None) -> tuple[list[tuple[bytes, str]], dict, dict]:
    parts: list[dict] = []
    if photo is not None:
        parts.append({"inlineData": {"mimeType": "image/jpeg",
                                     "data": base64.b64encode(photo.read_bytes()).decode()}})
    parts.append({"text": prompt})
    body = {"contents": [{"parts": parts}], "generationConfig": {"responseModalities": ["IMAGE"]}}
    r = httpx.post(f"{GEMINI_URL}/models/{model}:generateContent", json=body,
                   headers={"x-goog-api-key": key}, timeout=TIMEOUT)
    js = r.json() if r.headers.get("content-type", "").startswith("application/json") else {}
    if r.status_code != 200:
        raise Fail(f"Gemini API {r.status_code}: {json.dumps(js.get('error', r.text[:300]))}")
    images = []
    for cand in js.get("candidates", []):
        for part in (cand.get("content") or {}).get("parts", []):
            inline = part.get("inlineData")
            if inline and inline.get("data"):
                images.append((base64.b64decode(inline["data"]), inline.get("mimeType", "")))
    params = {"responseModalities": ["IMAGE"], "photo": photo.name if photo else None}
    return images, params, {"status": r.status_code, "headers": {k: v for k, v in r.headers.items()
                                                                 if k.lower().startswith("x-")},
                            "body": strip_images(js)}


def openai_call(key: str, model: str, prompt: str, photo: Path | None, fmt: str,
                background: str | None, extra: dict) -> tuple[list[tuple[bytes, str]], dict, dict]:
    headers = {"Authorization": f"Bearer {key}"}
    params: dict = {"model": model, "output_format": fmt, **extra}
    if background:
        params["background"] = background
    if photo is None:
        r = httpx.post(f"{OPENAI_URL}/images/generations", json={**params, "prompt": prompt},
                       headers=headers, timeout=TIMEOUT)
    else:
        files = {"image": (photo.name, photo.read_bytes(), "image/jpeg")}
        form = {k: str(v) for k, v in {**params, "prompt": prompt}.items()}
        r = httpx.post(f"{OPENAI_URL}/images/edits", data=form,
                       files=files, headers=headers, timeout=TIMEOUT)
    js = r.json() if r.headers.get("content-type", "").startswith("application/json") else {}
    if r.status_code != 200:
        raise Fail(f"OpenAI API {r.status_code}: {json.dumps(js.get('error', r.text[:300]))}")
    images = [(base64.b64decode(d["b64_json"]), OPENAI_FORMATS[fmt])
              for d in js.get("data", []) if d.get("b64_json")]
    return images, {**params, "photo": photo.name if photo else None}, {
        "status": r.status_code,
        "headers": {k: v for k, v in r.headers.items() if k.lower().startswith(("x-", "openai-"))},
        "body": strip_images(js),
    }


def list_models(provider: str, key: str) -> list[str]:
    if provider == "gemini":
        r = httpx.get(f"{GEMINI_URL}/models", params={"pageSize": 200},
                      headers={"x-goog-api-key": key}, timeout=TIMEOUT)
    else:
        r = httpx.get(f"{OPENAI_URL}/models", headers={"Authorization": f"Bearer {key}"}, timeout=TIMEOUT)
    if r.status_code != 200:
        # the providers' own error text (OpenAI masks the key in it, e.g. sk-proj-***abcd)
        try:
            err = r.json().get("error", {})
            detail = err.get("message", "") if isinstance(err, dict) else str(err)
        except ValueError:
            detail = r.text[:200]
        raise Fail(f"{provider} model list: HTTP {r.status_code}: {detail}")
    if provider == "gemini":
        names = [m["name"].removeprefix("models/") for m in r.json().get("models", [])]
    else:
        names = [m["id"] for m in r.json().get("data", [])]
    return sorted(n for n in names if "image" in n)


# ---- interactive fill-in --------------------------------------------------------------
def ask(question):
    import inquirer  # only needed when something is missing

    if not sys.stdin.isatty():
        raise Fail("missing options and no terminal to ask in; pass them as arguments")
    answer = inquirer.prompt([question])
    if not answer:
        raise typer.Exit(1)
    return next(iter(answer.values()))


def fill_in(provider: str | None, model: str | None, prompt_ids: list[str] | None,
            kind: str) -> tuple[str, str, list[str]]:
    import inquirer

    if not provider:
        provider = ask(inquirer.List("provider", message="Provider", choices=["gemini", "openai"]))
    if not model:
        models = list_models(provider, api_key(provider))
        model = ask(inquirer.List("model", message="Model", choices=models))
    if not prompt_ids:
        prompts = load_prompts()
        ids = [p for p in prompts if p.startswith("p" if kind == "gen" else "e")]
        prompt_ids = ask(inquirer.Checkbox(
            "prompts", message="Prompts (space to select, enter to run)",
            choices=[(f"{p}: {prompts[p][:70]}", p) for p in ids]))
        if not prompt_ids:
            raise Fail("no prompt selected")
    return provider, model, prompt_ids


# ---- the run ----------------------------------------------------------------------------
def run(kind: str, provider: str, model: str, prompt_ids: list[str], formats: list[str],
        background: str | None, suffix: str, n: int | None, stratum: str, force: bool,
        dry_run: bool, inspect: bool, extra: dict | None = None) -> None:
    extra = {k: v for k, v in (extra or {}).items() if v is not None}
    prompts = load_prompts()
    want = "p" if kind == "gen" else "e"
    bad = [p for p in prompt_ids if p not in prompts or not p.startswith(want)]
    if bad:
        raise Fail(f"{kind} takes {'p1–p5' if kind == 'gen' else 'e1–e3'}; got {', '.join(bad)}")
    if provider == "gemini" and (background or formats or extra):
        raise Fail("--format, --background, --quality, --size, --compression and --input-fidelity "
                   "apply to openai only (Gemini picks its own format)")
    if extra and not suffix:
        raise Fail(f"non-default settings ({', '.join(f'{k}={v}' for k, v in extra.items())}) need a "
                   "--suffix, so the variant token says so (PROMPTS.md: defaults unless noted)")
    if kind == "gen" and "input_fidelity" in extra:
        raise Fail("--input-fidelity applies to edits only")
    if provider == "openai":
        formats = formats or ["png"]
        bad_fmt = [f for f in formats if f not in OPENAI_FORMATS]
        if bad_fmt:
            raise Fail(f"--format takes png, jpeg, webp; got {', '.join(bad_fmt)}")
        if background == "transparent":
            if model.startswith(OPENAI_NO_TRANSPARENT):
                raise Fail(f"{model} does not support transparent backgrounds (OpenAI's reference); "
                           "use gpt-image-1.5 or gpt-image-1 for the transparent p3")
            if "jpeg" in formats:
                raise Fail("a transparent background needs png or webp, not jpeg")
        if "output_compression" in extra and set(formats) == {"png"}:
            raise Fail("--compression applies to jpeg and webp only")
    if background == "transparent" and not suffix:
        suffix = "transparent"
    dest_dir = DIFF / stratum
    if not dest_dir.is_dir():
        raise Fail(f"no stratum folder {dest_dir}")
    key = "" if dry_run else api_key(provider)
    runs = [(pid, fmt) for pid in prompt_ids for fmt in (formats if provider == "openai" else [None])]
    for pid, _ in runs:
        if kind == "edit" and not (BASE / EDIT_PHOTO[pid]).exists():
            raise Fail(f"base photo missing: {BASE / EDIT_PHOTO[pid]}")

    if dry_run:
        for pid, fmt in runs:
            number = n if n is not None else next_free_n(dest_dir, provider, model, suffix, pid)
            ext = {"png": "png", "jpeg": "jpg", "webp": "webp"}.get(fmt or "")
            name = (target_name(provider, model, suffix, ext, pid, number) if ext
                    else f"{GENERATOR[provider]}__{slug(model)}{'-' + slug(suffix) if suffix else ''}"
                         f"__api-<type from response>__{pid}__{number}")
            CONSOLE.print(f"would write  {name}")
        return

    results: list[dict] = []
    t_all = time.monotonic()
    progress = Progress(
        SpinnerColumn(), TextColumn("{task.description}"), BarColumn(bar_width=24),
        TextColumn("{task.fields[count]}"), TimeElapsedColumn(), console=CONSOLE,
    )
    try:
        with progress:
            overall = progress.add_task(f"[bold]{kind} {provider} {model}", total=len(runs),
                                        count=f"0/{len(runs)}")
            for done, (pid, fmt) in enumerate(runs, 1):
                photo = BASE / EDIT_PHOTO[pid] if kind == "edit" else None
                number = n if n is not None else next_free_n(dest_dir, provider, model, suffix, pid)
                label = pid + (f" {fmt}" if fmt else "") + (f" ({photo.name})" if photo else "")
                row = {"prompt": pid, "format": fmt or "", "file": "", "type": "", "bytes": 0,
                       "seconds": 0.0, "result": ""}
                results.append(row)
                current = progress.add_task(f"  {label}: waiting for {provider}", total=None, count="")
                started = datetime.now(timezone.utc)
                t = time.monotonic()
                try:
                    if provider == "gemini":
                        images, params, meta = gemini_call(key, model, prompts[pid], photo)
                        endpoint = f"{GEMINI_URL}/models/{model}:generateContent"
                    else:
                        images, params, meta = openai_call(key, model, prompts[pid], photo, fmt, background, extra)
                        endpoint = f"{OPENAI_URL}/images/{'edits' if photo else 'generations'}"
                except (Fail, httpx.HTTPError) as e:
                    row["seconds"] = time.monotonic() - t
                    row["result"] = "failed"
                    # a refusal comes back as an API error: it is a row too (PROMPTS.md), so log it
                    log_call({"at": started.isoformat(timespec="seconds"),
                              "duration_s": round(row["seconds"], 2), "provider": provider,
                              "model": model, "prompt_id": pid, "prompt": prompts[pid],
                              "photo": photo.name if photo else None, "format": fmt, "error": str(e)})
                    raise Fail(f"{label}: {e}") from e
                finally:
                    progress.remove_task(current)
                row["seconds"] = elapsed = time.monotonic() - t
                entry = {"at": started.isoformat(timespec="seconds"), "duration_s": round(elapsed, 2),
                         "provider": provider, "endpoint": endpoint, "model": model, "prompt_id": pid,
                         "prompt": prompts[pid], "params": params, "response": meta, "files": []}
                if not images:
                    log_call(entry)
                    row["result"] = "no image"
                    raise Fail(f"{label}: the response holds no image (refusal or filter?); "
                               f"see the last line of {LOG.relative_to(ROOT)}")
                for i, (data, mime) in enumerate(images):
                    ext = EXT.get(mime, "bin")
                    if i > 0:  # several images in one response: each is its own generation
                        number = n + i if n is not None else next_free_n(dest_dir, provider, model, suffix, pid)
                        row = {**row, "file": "", "result": ""}
                        results.append(row)
                    name = target_name(provider, model, suffix, ext, pid, number) if ext in PATH_TOKEN else f"unknown-{pid}"
                    path = write_exact(data, mime, dest_dir, name, force)
                    digest = hashlib.sha256(data).hexdigest()
                    entry["files"].append({"file": path.name, "mime": mime, "bytes": len(data), "sha256": digest})
                    row.update(file=path.name, type=mime, bytes=len(data), result="ok")
                    progress.console.print(f"  [green]ok[/]  {path.name}  {mime}  {len(data):,} B  "
                                           f"{elapsed:.1f} s  sha256 {digest[:12]}…")
                log_call(entry)
                progress.update(overall, advance=1, count=f"{done}/{len(runs)}")
    finally:
        summary(results, time.monotonic() - t_all)
    if inspect and shutil.which("ht"):
        for row in results:
            if row["result"] == "ok":
                CONSOLE.rule(row["file"], style="dim")
                subprocess.run(["ht", "inspect", "--only", "manifest", "--provenance", str(dest_dir / row["file"])])


def summary(results: list[dict], total: float) -> None:
    """What was written, and how long each call took, whether the run finished or not."""
    if not results:
        return
    table = Table(title=f"api-gen · {len(results)} call(s) · {total:.1f} s", title_justify="left")
    for col, justify in (("prompt", "left"), ("format", "left"), ("file", "left"), ("type", "left"),
                         ("bytes", "right"), ("seconds", "right"), ("result", "left")):
        # the file name is the point of the table: wrap it rather than cut it
        table.add_column(col, justify=justify, overflow="fold" if col == "file" else "ellipsis",
                         no_wrap=col != "file")
    for r in results:
        style = {"ok": "green", "failed": "red", "no image": "yellow"}.get(r["result"], "dim")
        table.add_row(r["prompt"], r["format"], r["file"] or "-", r["type"] or "-",
                      f"{r['bytes']:,}" if r["bytes"] else "-", f"{r['seconds']:.1f}",
                      f"[{style}]{r['result'] or 'not run'}[/]")
    ok = [r for r in results if r["result"] == "ok"]
    timed = [r["seconds"] for r in results if r["seconds"]]
    table.caption = (f"{len(ok)} file(s) written, {sum(r['bytes'] for r in ok):,} B; "
                     f"per call: min {min(timed):.1f} s, max {max(timed):.1f} s" if timed else "")
    CONSOLE.print(table)


# ---- commands ---------------------------------------------------------------------------
Provider = Annotated[str | None, typer.Argument(help="gemini or openai; asked if left out")]
Model = Annotated[str | None, typer.Option("--model", "-m", help="model id as the API lists it; asked if left out")]
Prompt = Annotated[list[str] | None, typer.Option("--prompt", "-p", help="prompt id, repeatable; asked if left out")]
Format = Annotated[list[str] | None, typer.Option("--format", "-f", help="openai only: png, jpeg, webp; repeatable")]
Background = Annotated[str | None, typer.Option(help="openai only: transparent (adds the variant suffix)")]
Suffix = Annotated[str, typer.Option(help="extra variant token, e.g. 1x1")]
N = Annotated[int | None, typer.Option("--n", help="pin the generation number; default: the next free one")]
Stratum = Annotated[str, typer.Option(help="stratum folder under corpus/differential")]
Force = Annotated[bool, typer.Option(help="replace an existing file (old copy to /tmp/ht-replaced)")]
DryRun = Annotated[bool, typer.Option(help="show the names, call nothing")]
Inspect = Annotated[bool, typer.Option(help="run ht inspect --provenance on each new file")]
Quality = Annotated[str | None, typer.Option(help="openai only: low, medium, high, auto (needs --suffix)")]
Size = Annotated[str | None, typer.Option(help="openai only: e.g. 1024x1024, 1536x1024 (needs --suffix)")]
Compression = Annotated[int | None, typer.Option(help="openai only, jpeg/webp: 0-100 (needs --suffix)")]
Fidelity = Annotated[str | None, typer.Option("--input-fidelity", help="openai edits only: high or low (needs --suffix)")]


def guarded(fn):
    try:
        fn()
    except Fail as e:
        typer.secho(str(e), fg="red", err=True)
        raise typer.Exit(1)


@APP.command()
def gen(provider: Provider = None, model: Model = None, prompt: Prompt = None, format: Format = None,
        background: Background = None, suffix: Suffix = "", n: N = None, stratum: Stratum = "04-generators",
        force: Force = False, dry_run: DryRun = False, inspect: Inspect = True, quality: Quality = None,
        size: Size = None, compression: Compression = None) -> None:
    """Text-to-image: p1–p5."""
    def go():
        p, m, ids = fill_in(provider, model, [x.lower() for x in prompt] if prompt else None, "gen")
        run("gen", p, m, ids, format or [], background, suffix, n, stratum, force, dry_run, inspect,
            {"quality": quality, "size": size, "output_compression": compression})
    guarded(go)


@APP.command()
def edit(provider: Provider = None, model: Model = None, prompt: Prompt = None, format: Format = None,
         suffix: Suffix = "", n: N = None, stratum: Stratum = "04-generators", force: Force = False,
         dry_run: DryRun = False, inspect: Inspect = True, quality: Quality = None, size: Size = None,
         compression: Compression = None, input_fidelity: Fidelity = None) -> None:
    """Image edit on the base photos: e1 (bowl), e2 (flask), e3 (path)."""
    def go():
        p, m, ids = fill_in(provider, model, [x.lower() for x in prompt] if prompt else None, "edit")
        run("edit", p, m, ids, format or [], None, suffix, n, stratum, force, dry_run, inspect,
            {"quality": quality, "size": size, "output_compression": compression,
             "input_fidelity": input_fidelity})
    guarded(go)


@APP.command()
def prompts() -> None:
    """Show the prompt set exactly as it will be sent."""
    guarded(lambda: [typer.echo(f"{k}: {v}") for k, v in load_prompts().items()])


@APP.command()
def models(provider: Annotated[str, typer.Argument(help="gemini or openai")]) -> None:
    """Image models the stored key can use."""
    guarded(lambda: [typer.echo(m) for m in list_models(provider, api_key(provider))])


if __name__ == "__main__":
    APP()
