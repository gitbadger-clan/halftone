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
