"""ExifTool in one long-running process (`-stay_open`), for scripts that call it many
times. Each separate `exiftool` call pays ~90 ms of Perl start-up before doing any
work; through one process, 200 files cost the work alone.

    from exiftool import ExifTool
    with ExifTool() as et:
        out, err = et.run("-q", "-q", "-o", "out.jpg", "-XMP-dc:Title=x", "in.jpg")
        rows = et.json("-MIMEType", *paths)     # -j, one dict per file, in input order

Arguments are passed one per line through ExifTool's argument file on stdin, so no
shell ever sees them; an argument containing a newline is refused. ExifTool reports
no exit status per command in this mode: callers check stderr and their output.
Standard library only.
"""

from __future__ import annotations

import itertools
import json
import os
import selectors
import shutil
import subprocess


class ExifToolError(RuntimeError):
    pass


class ExifTool:
    def __init__(self, exe: str = "exiftool") -> None:
        if shutil.which(exe) is None:
            raise ExifToolError(f"{exe} not found on PATH")
        self._exe = exe
        self._p: subprocess.Popen[bytes] | None = None
        self._seq = itertools.count(1)

    def __enter__(self) -> ExifTool:
        self._p = subprocess.Popen(
            [self._exe, "-stay_open", "True", "-@", "-"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        return self

    def __exit__(self, *exc) -> None:
        p = self._p
        self._p = None
        if p is None:
            return
        try:
            assert p.stdin is not None
            p.stdin.write(b"-stay_open\nFalse\n")
            p.stdin.close()
            p.wait(timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            p.kill()
            p.wait()

    def version(self) -> str:
        return self.run("-ver")[0].strip()

    def run(self, *args: str) -> tuple[str, str]:
        """One ExifTool command; returns its (stdout, stderr)."""
        p = self._p
        if p is None or p.poll() is not None:
            raise ExifToolError("ExifTool is not running (use `with ExifTool() as et:`)")
        if any("\n" in a for a in args):
            raise ValueError("ExifTool arguments cannot contain a newline")
        n = next(self._seq)
        marker = f"{{ready{n}}}\n".encode()
        lines = [*args, "-echo4", f"{{ready{n}}}", f"-execute{n}"]
        assert p.stdin is not None and p.stdout is not None and p.stderr is not None
        p.stdin.write(("\n".join(lines) + "\n").encode())
        p.stdin.flush()
        # Both streams are drained together, so a chatty stderr can never fill its
        # pipe and stall ExifTool while we wait on stdout.
        bufs = {p.stdout: b"", p.stderr: b""}
        sel = selectors.DefaultSelector()
        for f in bufs:
            sel.register(f, selectors.EVENT_READ)
        try:
            while sel.get_map():
                for key, _ in sel.select():
                    chunk = os.read(key.fd, 65536)
                    if not chunk:
                        raise ExifToolError("ExifTool exited during a command")
                    bufs[key.fileobj] += chunk
                    if bufs[key.fileobj].endswith(marker):
                        sel.unregister(key.fileobj)
        finally:
            sel.close()
        out = bufs[p.stdout][: -len(marker)].decode("utf-8", "replace")
        err = bufs[p.stderr][: -len(marker)].decode("utf-8", "replace")
        return out, err

    def json(self, *args: str) -> list[dict]:
        """`exiftool -j ARGS`: one dict per file, in the order ExifTool read them.
        A file it could not read comes back with an "Error" key, or not at all."""
        out, err = self.run("-j", *args)
        if not out.strip():
            return []
        rows = json.loads(out)
        return rows if isinstance(rows, list) else []
