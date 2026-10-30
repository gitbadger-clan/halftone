"""Progress for long-running scripts, standard library only.

    from progress import Progress, step

    bar = Progress(len(files), "collect")          # a known number of items
    for f in bar.track(files, lambda f: f.name):   # advances after each item
        bar.write(f"{f}: ok")                      # a result line, printed above the live line

    with step("clone c2pa-rs"):                    # one step of unknown length: a spinner
        run_git()

The live line goes to stderr and only appears on an interactive terminal outside CI
($CI unset); it shows count, percent, elapsed and an estimate of the time left. In CI,
under freeze or piped, nothing live is drawn, `write` is a plain print, and each block
ends with one summary line on stderr (`collect: 200 in 3:12`), so logs stay readable.
stdout always carries only the script's own output. Safe to call from worker threads.
"""

from __future__ import annotations

import itertools
import os
import shutil
import sys
import threading
import time
from contextlib import contextmanager
from typing import Callable, Iterable, Iterator, TypeVar

T = TypeVar("T")

SPIN = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
DIM, RESET = ("\x1b[90m", "\x1b[0m") if "NO_COLOR" not in os.environ else ("", "")


def live() -> bool:
    return not os.environ.get("CI") and sys.stderr.isatty()


def clock(seconds: float) -> str:
    s = int(seconds)
    return f"{s // 3600}:{s // 60 % 60:02}:{s % 60:02}" if s >= 3600 else f"{s // 60}:{s % 60:02}"


def _fit(text: str) -> str:
    width = shutil.get_terminal_size((100, 20)).columns - 1
    return text if len(text) <= width else text[: width - 1] + "…"


class Progress:
    def __init__(self, total: int, title: str = "") -> None:
        self.total, self.title, self.done = total, title, 0
        self._label = ""
        self._live = live()
        self._lock = threading.Lock()
        self._start = time.monotonic()
        self._drawn = 0.0

    def __enter__(self) -> Progress:
        self._start = time.monotonic()
        self._draw(force=True)
        return self

    def __exit__(self, *exc) -> None:
        with self._lock:
            self._clear()
            took = clock(time.monotonic() - self._start)
            prefix = f"{self.title}: " if self.title else ""
            print(f"{DIM}{prefix}{self.done} of {self.total} in {took}{RESET}", file=sys.stderr, flush=True)

    def track(self, items: Iterable[T], label: Callable[[T], str] = str) -> Iterator[T]:
        """Yield the items, advancing after each one is done; ends the block when they
        run out (or the loop breaks). `for p in bar.track(paths, lambda p: p.name):`"""
        with self:
            for x in items:
                yield x
                self.advance(label(x))

    def advance(self, label: str = "", n: int = 1) -> None:
        with self._lock:
            self.done += n
            self._label = label
            self._draw()

    def write(self, line: str) -> None:
        """Print a result line to stdout without tearing the live line."""
        with self._lock:
            self._clear()
            print(line, flush=True)
            self._draw(force=True)

    def _line(self) -> str:
        elapsed = time.monotonic() - self._start
        pct = 100 * self.done // self.total if self.total else 100
        left = ""
        if 0 < self.done < self.total:
            left = f" · ~{clock(elapsed / self.done * (self.total - self.done))} left"
        width = len(str(self.total))
        head = f"{self.title} " if self.title else ""
        tail = f" · {self._label}" if self._label else ""
        return f"{head}[{self.done:>{width}}/{self.total}] {pct:>3}% · {clock(elapsed)}{left}{tail}"

    def _draw(self, force: bool = False) -> None:
        if not self._live:
            return
        now = time.monotonic()
        if not force and now - self._drawn < 0.1 and self.done < self.total:
            return
        self._drawn = now
        sys.stderr.write("\r\x1b[2K" + DIM + _fit(self._line()) + RESET)
        sys.stderr.flush()

    def _clear(self) -> None:
        if self._live:
            sys.stderr.write("\r\x1b[2K")
            sys.stderr.flush()


@contextmanager
def step(title: str, summary: bool = True) -> Iterator[None]:
    """A spinner with elapsed time while the block runs (interactive terminals only),
    then `title: 0:12` on stderr. summary=False leaves the closing line to the caller."""
    start = time.monotonic()
    stop = threading.Event()

    def spin() -> None:
        for ch in itertools.cycle(SPIN):
            if stop.wait(0.1):
                return
            line = _fit(f"{ch} {title} · {clock(time.monotonic() - start)}")
            sys.stderr.write("\r\x1b[2K" + DIM + line + RESET)
            sys.stderr.flush()

    t = threading.Thread(target=spin, daemon=True) if live() else None
    if t:
        t.start()
    ok = False
    try:
        yield
        ok = True
    finally:
        if t:
            stop.set()
            t.join()
            sys.stderr.write("\r\x1b[2K")
            sys.stderr.flush()
        if summary:
            word = "" if ok else " (failed)"
            print(f"{DIM}{title}: {clock(time.monotonic() - start)}{word}{RESET}", file=sys.stderr, flush=True)
