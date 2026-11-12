# scripts/

Tools for maintaining Halftone's corpus, ground truth and published evidence. None of
this ships: `ht` never imports or calls anything here.

## Where a new tool goes

| Destination | When |
|---|---|
| `ht` (Rust) | An end user would run it, it only reads files, and it needs no external tool. |
| `scripts/htc.py` | It brings files from outside the repo into a stratum: phone, browser downloads, generator APIs. |
| `scripts/*.py` | Everything else: reference collectors (oracles), strata built from other corpus files, gates, analysis. |
| `scripts/pub/*.fish` | Frames for posts: real command output, rendered with freeze. These stay fish (see below). |
| `scripts/archive/` | One-shots that have run. Kept for the record, not maintained. |

Oracles (`differential.py`, the rubric golden scripts, `ocsp-extract.py`) produce the
ground truth Halftone is tested against. They never reuse Halftone's code or output.

## Running a task

```sh
uv run scripts/dev.py                  # every task, with one line on what it does
uv run scripts/dev.py collect 04 06    # runs scripts/collect.py 04 06
```

Any `scripts/<name>.py` is a task; there is no list to maintain. Arguments after the
task name go to the script unchanged. Running a script directly works too.

To type `dev collect 04 06` instead, let bootstrap install the `dev` command and its
fish completion:

```sh
uv run scripts/bootstrap.py --shell            # --shell --remove takes them away again
```

It writes a two-line wrapper to `~/.local/bin/dev` that runs this checkout's
`scripts/dev.py` through uv, so no file in the repo needs the executable bit and `dev`
works from any directory. If fish is set up, it also links
`scripts/completions/dev.fish` into `~/.config/fish/completions`. It never replaces a
`dev` or a completion file it did not write, and a plain bootstrap run only reports
whether they are installed. Run it again after moving the checkout.

(bash, zsh: `alias dev='uv run scripts/dev.py'`; there is no completion for them yet.
The completion only ever calls `dev`, so a fish function works as well. A fish `alias`
does too, but it makes `dev` inherit uv's completions on top of its own.)

Then Tab completes the task name, and after it the task's own subcommands, options,
choices and file arguments, each with its help line:

```
dev <Tab>                  →  api-gen  collect  htc  versiondiff  …
dev htc android <Tab>      →  grab  mark  new  pull  range  send  type  …
dev collect <Tab>          →  00-base  01-c2pa-rs  …
dev references bump --<Tab>  →  --cooldown  --dry-run  --c2pa-upgrade  …
```

The candidates come from the scripts themselves (`lib/complete.py` reads the real
argparse parser or Typer app), so they cannot go stale. A Tab never downloads
anything: before `bootstrap` has run, a script with dependencies offers nothing.

## Collecting into a stratum: `htc`

`htc.py` brings phone captures and browser downloads into `corpus/differential/<stratum>`
(generator APIs have their own script, `api-gen.py`).

```fish
set -gx HT_STRATUM 06-phones
dev htc android use fold | source  # Android over adb, one phone per terminal
dev htc android mark               # then save on the phone
dev htc android grab <stem>        # the one new image, sha256-checked, inspected
dev htc ios mark                   # iPhone / iPad over AFC
dev htc ios grab <stem>
dev htc desk mark                  # then download in the browser
dev htc desk take <stem>
dev htc --help                     # everything else: type, send, latest, range, tmp, layout
```

## Setting up

Once per machine, and again after pulling a change to a script's dependencies:

```sh
uv run scripts/bootstrap.py
```

`dev` offers this itself when it is needed: in an interactive terminal outside CI it
explains why and asks once, and remembers a "no" until a dependency changes.

It installs Python 3.14, locks and builds every script's environment in uv's cache,
and checks the external tools (ExifTool, c2patool, Credentio, …). After that a script
starts in a fraction of a second without downloading anything, offline too
(`UV_OFFLINE=1`). `--upgrade` re-resolves every lock; `--tools` only checks the tools.

Every script with dependencies declares `exclude-newer = "14 days"` under `[tool.uv]`,
so a lock never takes a release less than two weeks old: long enough for a broken or
malicious upload to be noticed and yanked. The cutoff lives in the script, not on the
command line, so `uv run --locked` (and `UV_LOCKED=1` in CI) accepts the lock without
any extra flag. To move to newer versions, run `--upgrade`; it still respects the cutoff.

## Writing a script

Python 3.14, run with `uv`, dependencies declared inline (PEP 723). No virtualenv.

```python
#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.14"
# dependencies = ["typer>=0.27"]
#
# [tool.uv]
# exclude-newer = "14 days"  # only releases at least two weeks old
# ///
"""One line saying what it does.

Usage examples, what it needs on PATH, which hosts it contacts (or that it is offline),
what it writes, and its exit codes.
"""
```

Allowed dependencies: typer (with click under it), inquirer, rich, pydantic, httpx,
cryptography, cbor2, PyYAML, numpy, Pillow. Add one where it does a job, not to every
script. Every script with dependencies has a `.lock` file next to it (bootstrap writes
it); commit it so every contributor runs the same versions.

Heavy libraries (numpy, Pillow) are imported inside the function that needs them,
and a script that uses them processes all its files in one run: import costs are
paid per process, so never call such a script once per file. The same goes for
ExifTool: use `lib/exiftool.py`, which keeps one ExifTool process for the whole run
instead of paying Perl's start-up (~90 ms) per file.

Every script answers tab completion: call `serve` from `lib/complete.py` on the
argparse parser or Typer app right before parsing. It returns at once unless the shell
is asking, and then prints the candidates and exits without running anything, so
nothing with a side effect may come before it.

```python
from complete import serve

serve(ap)                 # argparse: before ap.parse_args()
serve(app)                # Typer: before app()
serve(app, values={"strata": lambda: [d.name for d in strata_dirs()]})   # own values for an argument
```

Exit codes: 0 done, 1 the check or task found a problem, 2 usage or setup error.

Anything that runs longer than a few seconds shows progress with `lib/progress.py`.
It draws on stderr, only on an interactive terminal outside CI, so stdout stays the
script's own output and CI logs get one summary line per block. A script that streams
a child process's output prints numbered headers and durations instead of a live line.

Typer apps are created with `add_completion=False` and
`pretty_exceptions_show_locals=False`. Typer up to at least 0.20 prints every local
variable in a traceback by default, API keys included.

Inquirer only fills in arguments that were not given, and only when stdin is a
terminal; otherwise the script fails and names the missing option. Frames never
prompt: freeze runs them in a pseudo-terminal, where a prompt hangs the render.

Scripts never read the clipboard. Writing to it happens only behind an explicit flag
(e.g. `provenance-judge.py --copy-prompt`).

## Shared code: `scripts/lib/`

```python
sys.path.insert(0, str(Path(__file__).resolve().parent / "lib"))
from repo import ROOT, CORPUS, die, need
```

Modules in `lib/` use the standard library only, because a PEP 723 script cannot
inherit dependencies from a module it imports. The exception is `lib/models.py`
(pydantic): import it only from scripts that declare `pydantic`.

| Module | Holds |
|---|---|
| `repo.py` | repo root, corpus and trust paths, `die`, `need`, terminal colours |
| `c2p.py` | c2patool isolated from this machine's config, with the collector's settings |
| `exiftool.py` | one long-running ExifTool process (`-stay_open`) for many commands |
| `pins.py` | the reference pins and how each tool's version is read; `drift()` lists where this machine differs |
| `corpuskey.py` | the corpus key and the keyed fingerprint that stands in for personal values in the private strata's ground truth |
| `complete.py` | tab completion answered by the script itself: `serve(parser_or_app)`; self-test: `python scripts/lib/complete.py` |
| `envstate.py` | whether bootstrap has prepared the scripts as they are now (read by `dev.py`) |
| `progress.py` | `Progress` (count, percent, time left) and `step` (spinner) for long runs |
| `models.py` | Pydantic models of `expectations.json` (`Expectations`, `Tools`); needs `pydantic` |

## Reference tools in Docker

The references the collector runs (ExifTool, OpenSSL, c2patool, Credentio, the
conformance-public rubrics) are pinned in `docker/references.env` and built into one
image by `docker/references.dockerfile`. Collecting in it gives every machine the
same reference versions, and Credentio no longer has to be built by hand.

```sh
uv run scripts/references.py build                 # image for the current pins (first build 20–40 min)
uv run scripts/references.py show                  # pins vs this machine vs the image
uv run scripts/references.py collect 04 06         # collect.py, inside the image
uv run scripts/references.py bump credentio       # the manual tool update: newest commit
uv run scripts/references.py bump c2patool --dry-run   # what it would pick, change nothing
uv run scripts/references.py bump credentio c2patool   # several tools, one build and collection
uv run scripts/references.py bump all --dry-run        # the newest of every tool, change nothing
uv run scripts/references.py hold openssl <why>        # keep a pin put; bump passes over it
```

`collect` bind-mounts the checkout and runs as your user; ht and `cargo test` stay on
the host. Collect committed strata this way: run directly, `collect.py` checks this
machine's ExifTool, OpenSSL, c2patool, Credentio and rubrics against the pins and
collects nothing if one differs (`--allow-unpinned` overrides, for a comparison or
the nightly), so a committed header can never record a tool the pins do not name.

### Personal values in the ground truth

`expectations.json` is committed and the repo is public, but the private strata hold
files whose metadata names people and accounts. For those strata the collector records
a keyed fingerprint of every rights and identifier value ExifTool reads, never the
value: `HMAC-SHA256(key, file NUL field NUL value)`, 16 bytes. The differential test
computes the same from what `ht inspect --reveal` reads and compares. The public
strata (01, 02, 03) record values in the clear: theirs are published or invented.

The key is 32 random bytes in `~/.config/halftone/corpus.key` (or
`$HALFTONE_CORPUS_KEY`), outside the repo. `references.py collect` hands it to the
container by environment.

```sh
uv run scripts/collect.py --new-key          # once per machine that holds the private files
```

- Without the key the test still compares the field names of a private stratum and
  says that the values were not compared.
- A lost key costs one re-collection: make a new key, `references.py collect` the
  private strata, commit. Nothing else depends on it.
- A leaked key is worse than a lost one: fingerprints already in the git history can
  then be guessed against. Treat those values as disclosed, then rotate the same way.
- Preflight step 8 fails if a private stratum's `expectations.json` holds a value in
  the clear, so one cannot be committed by accident.

`bump` edits the pins it is given, builds, re-collects every stratum, runs the
differential test and prints the diff. It never commits, and refuses to start with
uncommitted corpus or pin changes. Without a value, a tool takes its newest release
at least 14 days old (`--cooldown`), never an older one than the pin; `all` does that
for every tool except llvm, which follows the Clang version Credentio asks for.
OpenSSL stays on its pinned series (3.5.x) unless given a version. Lookups use git
itself (`ls-remote` and a shallow fetch without file contents), so no API token is
needed; c2patool's come from crates.io.

A hold keeps a pin where it is, with the reason written next to it in
`docker/references.env` (holds are not build inputs and never change the image tag).
`bump all` passes over a held tool and says why, `bump <tool>` alone refuses, and a
value given for it moves it and keeps the hold. `unhold` lifts it. A move to an older
version is marked as a downgrade in the summary and the commit message. c2patool moves with Halftone's c2pa: `bump` reads the c2pa version in each
c2patool's published Cargo.lock and takes one on another c2pa line (0.92 when
Halftone has 0.91) only with `--c2pa-upgrade`. Fixture pins move separately, with
`refresh-public-strata.py`, never in the same change. Commits are pinned in full.
Several tools can move in one bump (`credentio=<sha> c2patool` mixes a fixed value
and a lookup): every fact in `expectations.json` belongs to one tool's field, so the
diff still says which tool changed what, and a changed fact gets that tool's D-entry.

The image's tag hashes the pins and the Dockerfile: the same pins give the same
recorded versions, but the base images, apt packages and Python patch release float,
so a rebuild is not bit-for-bit identical.

## c2patool

Always through `lib/c2p.py` (Python) or `pub/c2p.fish` (frames). A `c2pa.toml` under
`$XDG_CONFIG_HOME` or a `$C2PATOOL_SETTINGS` on the machine silently changes
c2patool's verdicts (DIFFERENTIAL.md D-008); `c2p` removes both and builds its
settings with `differential.py`'s own `c2patool_settings()`, so nothing here can drift
from the ground truth. `c2patool-settings.py` prints those settings for shell callers.
`pub/d008` and the first command of `pub/remote-fetch` run c2patool with its defaults
on purpose: those defaults are what the posts are about.

## Types

Pydantic models are strict for files this repo writes (`expectations.json`,
`cases.json`, corpus rules, snapshots) and permissive (`extra="allow"`) for what
external tools print. A reference tool that adds a field must never cost the record
of what it said. Validate when reading ground truth, not inside a collector's capture.

## Frames: `scripts/pub/`

Frames make the images and text blocks for posts. They are records of what was
published, not tools, so they stay fish and are not converted. New frames are fish
too. Each one prints the frame by default and renders it with `--png` (freeze), and
every value in it comes from a command run at render time, never typed in.

A frame that calls c2patool sources `pub/c2p.fish` after `cd $repo` and calls `c2p`.
With the default mode its output is identical on any machine with no c2patool config;
check a changed frame by diffing its same-day output before and after.

## The other fish scripts

The `.fish` scripts outside `pub/` are being converted to Python. Convert one when you
next change or run it, and check the conversion against the original before deleting
it: the same-day output must match apart from date lines, and anything that writes
corpus files must produce the same sha256s.
