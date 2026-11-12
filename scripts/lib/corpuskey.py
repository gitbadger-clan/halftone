"""The corpus key: what keeps personal values out of the committed ground truth.

The private strata hold files whose metadata names people and accounts. Their
expectations.json is public, so for those strata the collector records, for each
value ExifTool reads from a rights or identifier field, a keyed fingerprint in place
of the value:

    HMAC-SHA256(key, file name NUL field NUL value), first 16 bytes, hex

The differential test computes the same from what Halftone reads and compares. Equal
fingerprints mean equal values; nothing in the repo can be turned back into a value
without the key, and the file name inside the fingerprint hides that one value repeats
across files. A plain hash would not do: names, coordinates and short IDs can be
guessed and checked.

The key is 32 random bytes. It lives outside the repo, in $HALFTONE_CORPUS_KEY (hex)
or in ~/.config/halftone/corpus.key, next to the private files it protects. It is not
precious: fingerprints are derived data, so a lost key costs one re-collection of the
private strata with a new key (scripts/collect.py --new-key). A leaked key is worse,
because the fingerprints already in the git history can then be guessed against;
treat the values as disclosed and rotate.

Standard library only.
"""

from __future__ import annotations

import hashlib
import hmac
import os
import secrets
from pathlib import Path

# Strata anyone can rebuild from public sources; their values are nobody's secret and
# are recorded in the clear. Every other stratum holds private files.
PUBLIC_STRATA = frozenset({"01-c2pa-rs", "02-exiftool", "03-synthetic"})

ENV = "HALFTONE_CORPUS_KEY"
PATH = Path.home() / ".config" / "halftone" / "corpus.key"
ALGORITHM = "HMAC-SHA256/128 over file NUL field NUL value"


def load() -> bytes | None:
    """The key from $HALFTONE_CORPUS_KEY (hex) or the key file; None when there is none.
    A value that is not 64 hex digits is treated as no key at all."""
    text = os.environ.get(ENV) or (PATH.read_text() if PATH.is_file() else "")
    text = text.strip()
    try:
        key = bytes.fromhex(text)
    except ValueError:
        return None
    return key if len(key) == 32 else None


def create() -> bytes:
    """Write a new key file readable by the owner only. Refuses to replace one."""
    if PATH.exists():
        raise FileExistsError(str(PATH))
    key = secrets.token_bytes(32)
    PATH.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(PATH, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as f:
        f.write(key.hex() + "\n")
    return key


def key_id(key: bytes) -> str:
    """Eight hex digits naming a key without revealing it: which key fingerprinted a
    header, so a test with another key says "another key", not "values differ"."""
    return hmac.new(key, b"halftone corpus key id", hashlib.sha256).hexdigest()[:8]


def fingerprint(key: bytes, file: str, field: str, value: str) -> str:
    msg = b"\0".join(s.encode() for s in (file, field, value))
    return hmac.new(key, msg, hashlib.sha256).digest()[:16].hex()
