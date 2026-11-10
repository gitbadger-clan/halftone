"""Tab completion for every script, argparse and Typer alike, answered by the script
itself so it can never drift from the real options.

A script calls `serve(parser_or_app)` right before it parses its arguments:

    a = ap.parse_args()          ->    serve(ap); a = ap.parse_args()
    app()                        ->    serve(app); app()

Normally `serve` returns at once (one environment lookup). When the shell asks for
completions it sets three variables, and `serve` prints the candidates and exits 0
without running the script:

    HT_COMPLETE=1
    HT_COMPLETE_WORDS   the finished arguments before the cursor, joined by \\x1f
    HT_COMPLETE_CUR     the word being typed (may be empty)

Output: one candidate per line, `value<TAB>description`. A line `__files__` asks the
shell to add its own file-name completion. scripts/completions/dev.fish is the fish
side; nothing here is fish-specific.

What is completed: subcommands, option names (not ones already given, unless
repeatable), an option's choices, positional choices, and file names where the
argument is a path. A script can add its own values, keyed by argument name:

    serve(app, values={"strata": lambda: ["00-base", "01-c2pa-rs", ...]})

Standard library only. Typer is imported only when a Typer app is passed, which means
the calling script already depends on it.
"""

from __future__ import annotations

import argparse
import os
import re
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

FILES = "__files__"
SEP = "\x1f"
PATHISH = re.compile(r"file|path|dir|folder|pem|json|bin\b|image|out\b|dest|bundle|checkout", re.I)
MAX_HELP = 110

Values = dict[str, Callable[[], list[str]]]


@dataclass
class Opt:
    names: list[str]
    takes: int | str  # 0, a count, "?", "*" or "+"
    help: str = ""
    choices: list[str] = field(default_factory=list)
    files: bool = False
    key: str = ""
    repeatable: bool = False


@dataclass
class Pos:
    nargs: int | str  # a count, "?", "*" or "+"
    choices: list[str] = field(default_factory=list)
    files: bool = False
    key: str = ""


@dataclass
class Node:
    options: list[Opt] = field(default_factory=list)
    positionals: list[Pos] = field(default_factory=list)
    subs: dict[str, tuple[str, Callable[[], Node]]] = field(default_factory=dict)


def tidy(text: str | None) -> str:
    """One line, no tabs, cut to a width a completion pager shows whole."""
    s = " ".join((text or "").split())
    return s if len(s) <= MAX_HELP else s[: MAX_HELP - 1].rstrip() + "…"


# ---- argparse -------------------------------------------------------------------


def _ap_pathish(a: argparse.Action) -> bool:
    if a.type is Path or isinstance(a.type, argparse.FileType):
        return True
    if a.type not in (None, str):
        return False
    return bool(PATHISH.search(f"{a.dest} {a.metavar or ''}"))


def _ap_help(parser: argparse.ArgumentParser, a: argparse.Action) -> str:
    if not a.help or a.help is argparse.SUPPRESS:
        return ""
    try:
        return tidy(parser._get_formatter()._expand_help(a))
    except Exception:
        return tidy(a.help)


def _ap_takes(a: argparse.Action) -> int | str:
    if a.nargs is None:
        return 1
    if a.nargs in ("?", "*", "+"):
        return a.nargs
    if isinstance(a.nargs, int):
        return a.nargs
    return "*"  # REMAINDER, PARSER


def from_argparse(parser: argparse.ArgumentParser) -> Node:
    node = Node()
    for a in parser._actions:
        if a.help is argparse.SUPPRESS:
            continue
        choices = [str(c) for c in a.choices] if a.choices and not isinstance(a, argparse._SubParsersAction) else []
        if isinstance(a, argparse._SubParsersAction):
            helps = {c.dest: c.help for c in a._choices_actions}
            for name, sub in a.choices.items():
                node.subs[name] = (tidy(helps.get(name) or sub.description), lambda sub=sub: from_argparse(sub))
        elif a.option_strings:
            node.options.append(Opt(
                names=list(a.option_strings), takes=_ap_takes(a), help=_ap_help(parser, a), choices=choices,
                files=_ap_pathish(a) and a.nargs != 0, key=a.dest,
                repeatable=isinstance(a, (argparse._AppendAction, argparse._CountAction)),
            ))
        else:
            node.positionals.append(Pos(nargs=_ap_takes(a), choices=choices, files=_ap_pathish(a), key=a.dest))
    return node


# ---- click (what a Typer app builds) --------------------------------------------


def _click_kind(t: Any) -> tuple[list[str], bool]:
    """(choices, is a path) for a click parameter type, by duck typing."""
    name = getattr(t, "name", "")
    choices = [str(getattr(c, "value", c)) for c in getattr(t, "choices", [])] if name == "choice" else []
    return choices, name in ("path", "file", "filename", "directory")


def from_click(cmd: Any) -> Node:
    node = Node()
    for p in getattr(cmd, "params", []):
        choices, files = _click_kind(getattr(p, "type", None))
        if getattr(p, "param_type_name", "") == "option":
            if getattr(p, "hidden", False):
                continue
            flag = getattr(p, "is_flag", False) or getattr(p, "count", False)
            names = [*p.opts, *getattr(p, "secondary_opts", [])]
            node.options.append(Opt(
                names=names, takes=0 if flag else (p.nargs if p.nargs > 0 else "*"), help=tidy(getattr(p, "help", "")),
                choices=choices, files=files, key=p.name or "",
                repeatable=bool(getattr(p, "multiple", False) or getattr(p, "count", False)),
            ))
        else:
            node.positionals.append(Pos(nargs=p.nargs if p.nargs > 0 else "*", choices=choices, files=files,
                                        key=p.name or ""))
    node.options.append(Opt(names=["--help"], takes=0, help="Show this message and exit."))
    for name, sub in sorted(getattr(cmd, "commands", {}).items()):
        if getattr(sub, "hidden", False):
            continue
        short = sub.get_short_help_str(limit=MAX_HELP) if hasattr(sub, "get_short_help_str") else ""
        node.subs[name] = (tidy(short), lambda sub=sub: from_click(sub))
    return node


# ---- the walk -------------------------------------------------------------------


def _value_candidates(choices: list[str], files: bool, key: str, values: Values) -> list[tuple[str, str]]:
    out = [(c, "") for c in choices]
    if key in values:
        try:
            out += [(v, "") for v in values[key]()]
        except Exception:
            pass
    if files and not out:
        out.append((FILES, ""))
    return out


def candidates(node: Node, words: list[str], cur: str, values: Values | None = None) -> list[tuple[str, str]]:
    """What can stand at the cursor, given the finished words before it."""
    values = values or {}
    used: set[int] = set()
    pending: Opt | None = None  # an option still owed values
    owed: int | str = 0
    pos_i = 0  # index into node.positionals
    pos_taken = 0  # values the current positional has received
    only_positionals = False  # after a bare "--"

    def find(name: str) -> Opt | None:
        return next((o for o in node.options if name in o.names), None)

    for w in words:
        if pending is not None:
            if isinstance(owed, int):
                owed -= 1
                if owed <= 0:
                    pending = None
                continue
            if not w.startswith("-") or only_positionals:  # "?", "*", "+": values until the next option
                if owed == "?":
                    pending = None
                continue
            pending = None
        if w == "--" and not only_positionals:
            only_positionals = True
            continue
        if w.startswith("-") and len(w) > 1 and not only_positionals:
            name = w.split("=", 1)[0]
            o = find(name)
            if o is not None:
                used.add(id(o))
                if o.takes != 0 and "=" not in w:
                    pending, owed = o, o.takes
            continue
        if w in node.subs and pos_i >= len(node.positionals):
            node = node.subs[w][1]()
            used, pos_i, pos_taken = set(), 0, 0
            continue
        if pos_i < len(node.positionals):
            pos_taken += 1
            n = node.positionals[pos_i].nargs
            if isinstance(n, int) and pos_taken >= n or n == "?":
                pos_i, pos_taken = pos_i + 1, 0

    out: list[tuple[str, str]] = []
    if pending is not None:
        out += _value_candidates(pending.choices, pending.files, pending.key, values)
        if isinstance(owed, int):  # a value is required here; nothing else can stand
            return _filter(out, cur)
    if cur.startswith("-") and not only_positionals:
        if "=" in cur:  # --opt=value
            name, _, _ = cur.partition("=")
            o = find(name)
            if o is not None:
                vals = _value_candidates(o.choices, o.files, o.key, values)
                return _filter([(f"{name}={v}", d) for v, d in vals if v != FILES], cur)
            return []
        for o in node.options:
            if id(o) in used and not o.repeatable:
                continue
            out += [(n, o.help) for n in o.names]
        return _filter(out, cur)
    if pos_i < len(node.positionals):
        p = node.positionals[pos_i]
        out += _value_candidates(p.choices, p.files, p.key, values)
    else:
        out += [(name, desc) for name, (desc, _) in node.subs.items()]
    return _filter(out, cur)


def _filter(items: list[tuple[str, str]], cur: str) -> list[tuple[str, str]]:
    seen, out = set(), []
    for v, d in items:
        if v not in seen and (v == FILES or v.startswith(cur)):
            seen.add(v)
            out.append((v, d))
    return out


def model(obj: Any) -> Node:
    if isinstance(obj, argparse.ArgumentParser):
        return from_argparse(obj)
    if hasattr(obj, "registered_commands"):  # a typer.Typer
        import typer.main

        obj = typer.main.get_command(obj)
    return from_click(obj)


def serve(obj: Any, values: Values | None = None) -> None:
    """Answer a completion request and exit, or return at once if there is none."""
    if not os.environ.get("HT_COMPLETE"):
        return
    raw = os.environ.get("HT_COMPLETE_WORDS", "")
    words = raw.split(SEP) if raw else []
    cur = os.environ.get("HT_COMPLETE_CUR", "")
    try:
        for value, desc in candidates(model(obj), words, cur, values):
            print(f"{value}\t{desc}" if desc else value)
    except Exception as e:  # a completion must never print a traceback into the prompt
        if os.environ.get("HT_COMPLETE_DEBUG"):
            print(f"complete: {type(e).__name__}: {e}", file=sys.stderr)
    sys.exit(0)


def _selftest() -> None:
    """`python scripts/lib/complete.py`: the walk on a small argparse parser."""
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", choices=["fast", "full"], help="how thorough")
    ap.add_argument("-v", "--verbose", action="count", help="more output")
    sub = ap.add_subparsers(dest="cmd")
    run = sub.add_parser("run", help="run it")
    run.add_argument("target", help="a tool")
    run.add_argument("--out", type=Path, help="report file")
    run.add_argument("--dry-run", action="store_true")
    sub.add_parser("show", help="show it")
    node = from_argparse(ap)

    def names(words: list[str], cur: str, values: Values | None = None) -> list[str]:
        return [v for v, _ in candidates(node, words, cur, values)]

    assert names([], "") == ["run", "show"]
    assert names([], "s") == ["show"]
    assert names([], "--m") == ["--mode"]
    assert names(["--mode"], "") == ["fast", "full"]  # a required value: nothing else
    assert names(["--mode", "fast"], "") == ["run", "show"]
    assert names(["--mode=fast"], "-") == ["-h", "--help", "-v", "--verbose"]  # --mode used, -v repeatable
    assert names([], "--mode=fa") == ["--mode=fast"]
    assert names(["run"], "") == []  # free-form positional
    assert names(["run"], "", {"target": lambda: ["a", "b"]}) == ["a", "b"]
    assert names(["run", "x"], "--") == ["--help", "--out", "--dry-run"]
    assert names(["run", "x", "--out"], "") == [FILES]
    assert names(["run", "x", "--dry-run"], "--d") == []
    assert names(["run", "--", "x"], "-") == []  # after --, nothing is an option
    print("complete.py: ok")


if __name__ == "__main__":
    _selftest()
