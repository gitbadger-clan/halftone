#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "PyYAML>=6.0",
#   "json-formula @ git+https://github.com/adobe/json-formula-py.git@327a8d75669673c10db3051da816311ac2df8a32",
# ]
# ///
"""Golden fixtures for the C2PA integrity and conformance rubrics: crJSON inputs run
through the official conformance evaluator (c2pa-org/conformance-public), so the
native port can be held to exactly what the reference does.

    uv run --script scripts/rubric-conformance-golden.py \\
        [--rubrics ../conformance-public] [--samples ../profile-evaluator] \\
        [--out testdata/rubric-conformance]

Inputs (written to <out>/in/):
  adobe-*         Adobe's sample crJSON from adobe/profile-evaluator (Apache-2.0,
                  see <out>/NOTICE.md), unchanged.
  v-*             variants of the capture sample, each driving one path of the
                  rubrics (failures with {{matches}}, the one warning, spec versions).
  edge-*          empty, hostile and malformed documents.
Rubrics: the vendored integrity and conformance-0.2-spec-2.4 rubrics, plus
<out>/rubrics/mechanics.yml, a synthetic rubric for evaluator paths the C2PA rubrics
never take (fan-out to manifests, warning and default texts, every result type,
errors). Expected output: <out>/expected/<case>__<rubric>.json, the evaluator's
stdout unchanged, or <case>__<rubric>.crash when the evaluator rejects the document
outright (its last error line).
"""
import argparse, copy, hashlib, json, subprocess, sys
from pathlib import Path

JSON_FORMULA_PIN = "327a8d75669673c10db3051da816311ac2df8a32"
SAMPLES = {
    "adobe-capture-then-ai-edits": "capture-non-ai-then-ai-edits.json",
    "adobe-gathered-assertions": "gathered-assertions-test.json",
    "adobe-image-to-video": "i2v.json",
}
RUBRICS = {
    "integrity": "asset-rubric-integrity.yml",
    "conformance0.2-spec2.4": "asset-rubric-conformance0.2-spec2.4.yml",
}

MECHANICS = r"""rubric_metadata:
  name: Evaluator mechanics (synthetic, for golden tests)
  version: '0.1'
variables:
  $codes:
  - a
  - b
expressions:
  _first: manifests[0]
---
- id: inception:fan_out_manifests
  expression: manifests
  reportText:
    'true':
      en: 'Signed by {{matches}}'
    'false':
      en: no manifests
- id: inception:fan_out_then_non_object
  expression: '[manifests[0], `"x"`]'
  reportText:
    'true':
      en: fanned out
- id: transformation:fail_if_matched_strings
  failIfMatched: true
  expression: manifests[].label
  reportText:
    'true':
      en: no labels
    'false':
      en: 'labels: {{matches}}'
- id: validation:fail_if_matched_numbers
  failIfMatched: true
  expression: '`[1, 2.5]`'
  reportText:
    'false':
      en: 'numbers stay unreplaced: {{matches}}'
- id: validation:fail_if_matched_empty
  failIfMatched: true
  expression: '`[]`'
  reportText:
    'true':
      en: nothing matched
- id: validation:warn_with_text
  warnIfMatched: true
  expression: '`["w1", "w2"]`'
  reportText:
    'true':
      en: ok
    warn:
      en: 'warned: {{matches}}'
    'false':
      en: not used
- id: validation:warn_without_text
  warnIfMatched: true
  expression: '`["w"]`'
  reportText:
    'false':
      en: 'falls back to false text: {{matches}}'
- id: validation:warn_not_matched
  warnIfMatched: true
  expression: '`[]`'
- id: validation:default_text
  expression: '`0`'
  reportText:
    default:
      en: the default text
- id: validation:plain_string_text
  expression: '`1`'
  reportText:
    'true': a plain string, not a language map
- id: validation:negative_number
  expression: '`-1`'
- id: validation:float_result
  expression: '`0.5`'
- id: validation:string_result
  expression: '"s"'
- id: validation:empty_string_result
  expression: '""'
- id: validation:object_result
  expression: '`{}`'
- id: validation:null_result
  expression: nothing_here
- id: validation:list_of_strings
  expression: '`["x"]`'
- id: validation:empty_list
  expression: '`[]`'
- id: validation:runtime_error
  expression: length(`5`)
- id: validation:syntax_error
  expression: a.(
- id: c2pa:signal_compliance
  expression: '`true`'
  reportText:
    'true':
      en: Contains Example Compliance
- id: c2pa:signal_compliance
  expression: '`true`'
  reportText:
    'true':
      en: Plain Compliance Name
- id: no_category
  expression: '`true`'
- expression: '`true`'
- id: validation:no_expression
- id: custom:other_category
  expression: '`false`'
- id: validation:helper_and_variable
  expression: contains($codes, "a") && _first() != null
- just a string
---
block: this document is skipped
---
id: validation:single_document_statement
expression: '`true`'
"""


def variants(capture):
    """Variants of the capture sample, each driving one rubric path."""
    out = {}

    def v(name, edit):
        d = copy.deepcopy(capture)
        edit(d)
        out[name] = d

    def m0(d):
        return d["manifests"][0]

    def failures(d):
        m0(d)["validationResults"]["failure"] = [
            {"code": "assertion.action.malformed"},      # well-formed class
            {"code": "assertion.dataHash.mismatch"},     # valid class
            {"code": "signingCredential.untrusted"},     # trust class
        ]
    v("v-integrity-failures", failures)
    v("v-no-validation-results", lambda d: m0(d).pop("validationResults"))

    def conformant(d):
        for k, a in m0(d)["assertions"].items():
            if k.startswith("c2pa.actions"):
                a["allActionsIncluded"] = True
        m0(d)["claim.v2"]["claim_generator_info"]["specVersion"] = "2.4"
    v("v-conformant", conformant)

    def spec(value):
        return lambda d: m0(d)["claim.v2"]["claim_generator_info"].__setitem__("specVersion", value)
    v("v-spec-2.2", spec("2.2"))
    v("v-spec-number", spec(2.4))
    v("v-spec-array-form", lambda d: m0(d)["claim.v2"].__setitem__(
        "claim_generator_info", [dict(m0(d)["claim.v2"]["claim_generator_info"], specVersion="2.4")]))

    def legacy(d):
        a = m0(d)["assertions"]["c2pa.actions.v2"]["actions"][0]
        a.setdefault("parameters", {})["description"] = "legacy placement"
    v("v-legacy-parameters", legacy)
    v("v-label-not-urn", lambda d: m0(d).__setitem__("label", "not-a-urn"))
    v("v-actions-gathered", lambda d: m0(d)["claim.v2"].__setitem__(
        "gathered_assertions", m0(d)["claim.v2"].get("created_assertions", [])))
    return out


EDGES = {
    "edge-empty-document": {},
    "edge-no-manifests": {"manifests": []},
    "edge-manifest-not-an-object": {"manifests": ["x"]},
    "edge-hostile-types": {"manifests": [{
        "label": 5,
        "claim.v2": {"created_assertions": 5, "claim_generator_info": 7},
        "assertions": {"c2pa.actions.v2": {"actions": "not a list"}},
        "validationResults": {"failure": "not a list"},
    }]},
}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--rubrics", default="../conformance-public")
    ap.add_argument("--samples", default="../profile-evaluator")
    ap.add_argument("--out", default="testdata/rubric-conformance")
    a = ap.parse_args()
    rub = Path(a.rubrics) / "asset-rubrics"
    evaluator = rub / "c2pa_conformance_rubric_evaluator.py"
    samples = Path(a.samples) / "testfiles" / "asset-rubrics"
    if not evaluator.is_file():
        sys.exit(f"no evaluator under {rub} (git clone https://github.com/c2pa-org/conformance-public {a.rubrics})")
    if not samples.is_dir():
        sys.exit(f"no samples under {samples} (git clone https://github.com/adobe/profile-evaluator {a.samples})")
    out = Path(a.out)
    for d in ("in", "expected", "rubrics"):
        (out / d).mkdir(parents=True, exist_ok=True)

    cases = {name: json.loads((samples / f).read_text()) for name, f in SAMPLES.items()}
    cases.update(variants(cases["adobe-capture-then-ai-edits"]))
    cases.update(EDGES)
    mechanics = out / "rubrics" / "mechanics.yml"
    mechanics.write_text(MECHANICS)
    rubrics = {k: rub / f for k, f in RUBRICS.items()}
    rubrics["mechanics"] = mechanics

    # The pruning step was not ported because it is inactive (`cut_off_traits = []`).
    # Refuse to produce fixtures for an evaluator where it could run.
    if "cut_off_traits = []" not in evaluator.read_text():
        sys.exit("the evaluator's pruning step is active (cut_off_traits is not empty): port it first")
    # How PyYAML (the reference's YAML library) reads each vendored rubric.
    import yaml
    for k in RUBRICS:
        docs = list(yaml.safe_load_all(rubrics[k].read_text()))
        (out / "rubrics" / f"{k}.parsed.json").write_text(
            json.dumps(docs, indent=1, default=str, ensure_ascii=False) + "\n")

    for name, doc in cases.items():
        inp = out / "in" / f"{name}.json"
        inp.write_text(json.dumps(doc, indent=1, ensure_ascii=False) + "\n")
        for rname, rpath in rubrics.items():
            r = subprocess.run([sys.executable, str(evaluator), str(rpath), str(inp)],
                               capture_output=True, text=True)
            stem = out / "expected" / f"{name}__{rname}"
            if r.returncode != 0 or not r.stdout.strip():
                # The reference rejects the document outright (an exception outside
                # its per-statement handling): record that, so the port must reject
                # it too rather than invent a result.
                last = (r.stderr.strip().splitlines() or ["(no output)"])[-1]
                Path(f"{stem}.crash").write_text(last + "\n")
                continue
            Path(f"{stem}.json").write_text(r.stdout)

    git = lambda p: subprocess.run(["git", "-C", str(p), "rev-parse", "--short", "HEAD"],
                                   capture_output=True, text=True).stdout.strip()
    sha = lambda p: hashlib.sha256(Path(p).read_bytes()).hexdigest()
    (out / "VERSIONS.txt").write_text(
        f"conformance-public: {git(a.rubrics)}\n"
        f"profile-evaluator: {git(a.samples)}\n"
        + "".join(f"{k} sha256: {sha(p)}\n" for k, p in rubrics.items())
        + f"json-formula-py: {JSON_FORMULA_PIN}\n"
        f"cases: {len(cases)}, rubrics: {len(rubrics)}\n")
    (out / "NOTICE.md").write_text(
        "# Third-party test inputs\n\n"
        "`in/adobe-*.json` are sample crJSON documents from\n"
        "[adobe/profile-evaluator](https://github.com/adobe/profile-evaluator)\n"
        f"(`testfiles/asset-rubrics/`, commit `{git(a.samples)}`), Copyright Adobe, licensed\n"
        "under the Apache License, Version 2.0. The `v-*` inputs are modified copies of\n"
        "`adobe-capture-then-ai-edits.json`, each changed as described in\n"
        "`scripts/rubric-conformance-golden.py`.\n")
    print(f"{len(cases)} cases x {len(rubrics)} rubrics written to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
