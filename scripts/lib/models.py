"""Pydantic models of the structured files this repo writes.

Strict on what the repo writes (a wrong type or a missing required field fails
loudly), permissive about fields it does not know yet (`extra="allow"`), so a new
collector field never breaks a reader. Reference-tool output inside `files` stays
plain JSON: a tool that adds a field must never cost the record of what it said.

The one module in `lib/` with a dependency: import it only from scripts that
declare `pydantic`.
"""

from __future__ import annotations

import json
from datetime import datetime
from pathlib import Path
from typing import Any, Literal

from pydantic import BaseModel, ConfigDict, Field, ValidationError


class Tools(BaseModel):
    """Versions of the reference tools a stratum was collected with."""

    model_config = ConfigDict(extra="allow")
    exiftool: str | None = None
    c2patool: str | None = None
    openssl: str | None = None
    credentio: str | None = None


class Expectations(BaseModel):
    """A stratum's `expectations.json`, as `scripts/differential.py` writes it."""

    model_config = ConfigDict(extra="allow", populate_by_name=True)

    format: Literal["halftone-differential-expectations/1"] = Field(alias="schema")
    collected_at: datetime
    corpus: str
    tools: Tools
    trust_anchors: str | None = None
    tsa_anchors: str | None = None
    trust_config: str | None = None
    trust_config_sha256: str | None = None
    signals_rubric: dict[str, Any] | None = None
    conformance_rubrics: dict[str, str] | None = None
    files: dict[str, dict[str, Any]]

    def full_trust(self, eku_sha256: str) -> bool:
        """Collected with the vendored EKU policy (by hash) and both anchor lists, as
        DIFFERENTIAL.md D-008 and D-011 require; trivially true when c2patool did not run."""
        if self.tools.c2patool is None:
            return True
        return (
            self.trust_config_sha256 == eku_sha256
            and self.trust_anchors is not None
            and self.tsa_anchors is not None
        )


class ModelError(ValueError):
    pass


def load_expectations(path: Path) -> Expectations:
    """Parse and validate one expectations.json; the error names the file and fields."""
    try:
        return Expectations.model_validate(json.loads(path.read_text()))
    except (OSError, json.JSONDecodeError) as e:
        raise ModelError(f"{path}: {e}") from e
    except ValidationError as e:
        fields = "; ".join(f"{'.'.join(map(str, err['loc']))}: {err['msg']}" for err in e.errors())
        raise ModelError(f"{path}: {fields}") from e
