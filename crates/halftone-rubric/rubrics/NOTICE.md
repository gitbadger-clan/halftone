# Vendored C2PA asset rubrics

`asset-rubric-signals-local.yml` is the **C2PA Asset Signals Rubric (Local), version
1.0**, issued by the C2PA Conformance Task Force and published in
[c2pa-org/conformance-public](https://github.com/c2pa-org/conformance-public)
(`asset-rubrics/`, commit `da71a72`, 2026-10-22), licensed under
[Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/).

It is included **unmodified**: SHA-256
`0502e989e025b32a0b253645991abf56d2cd14860a1e7cda10e6921bc5ba19a8`, checked by
`tests/pin.rs` against `SIGNALS_LOCAL_SHA256`. To update it, copy the new file from
the conformance repo, update the constant, and regenerate the golden fixtures
(`uv run --script scripts/rubric-golden.py`) so the tests compare against the
reference evaluator's behaviour for the new version.

`asset-rubric-integrity.yml` (**C2PA Asset Integrity Rubric**, version 1.0, SHA-256
`f87ece6ff2c232540eb9fee5b3249d9961939e432c0456d353e45f88b77bba76`) and
`asset-rubric-conformance0.2-spec2.4.yml` (**C2PA Asset Conformance 0.2 Spec 2.4
Rubric**, version 1.0, SHA-256
`63e9dd777fbb9d1295efe086864c88475f8cae8d944ecb7b1ff21b718c4bade9`) come from the
same repository and directory, under the same licence, and are likewise included
unmodified (checked by `tests/pin.rs`). Their golden fixtures are regenerated with
`uv run --script scripts/rubric-conformance-golden.py`.
