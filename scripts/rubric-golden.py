#!/usr/bin/env -S uv run --script
# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "PyYAML>=6.0",
#   "json-formula @ git+https://github.com/adobe/json-formula-py.git@327a8d75669673c10db3051da816311ac2df8a32",
# ]
# ///
"""Golden fixtures for the C2PA signals rubric: minimal crJSON edge cases, each run
through the official evaluator (c2pa-org/conformance-public), so a native
implementation can be held to exactly what the reference does.

    uv run --script scripts/rubric-golden.py [--rubrics ../conformance-public] [--out testdata/rubric-signals]

Writes <out>/in/<case>.json (the crJSON), <out>/expected/<case>.json (the
evaluator's stdout, unchanged), <out>/formula-probes.json (json-formula-py's result
for each probe expression), <out>/rubric-parsed.json (PyYAML's reading of the
rubric) and <out>/VERSIONS.txt. Rerun after updating the
rubrics checkout; a diff in expected/ means the reference changed behaviour.
"""
import argparse, hashlib, json, os, subprocess, sys
from pathlib import Path

D = "http://cv.iptc.org/newscodes/digitalsourcetype/"
def m(label, actions=None, aai=None, extra=None, gathered=False, raw_actions=None):
    a = {}
    if raw_actions is not None:
        a["c2pa.actions.v2"] = raw_actions
    elif actions is not None:
        v = {"actions": actions}
        if aai is not None: v["allActionsIncluded"] = aai
        a["c2pa.actions.v2"] = v
    a.update(extra or {})
    url = "self#jumbf=c2pa.assertions/c2pa.actions.v2"
    claim = {"created_assertions": [] if gathered else [{"url": url}],
             "gathered_assertions": [{"url": url}] if gathered else []}
    return {"label": f"urn:c2pa:{label}", "claim.v2": claim, "assertions": a,
            "signature": {"certificateInfo": {"subject": {"CN": "t", "O": "Test"}}}}
cap = {"action": "c2pa.created", "digitalSourceType": D + "computationalCapture"}
C = {
 "01-capture-complete":        [m("a", [cap], True)],
 "02-capture-no-aai":          [m("a", [cap])],
 "03-aai-false":               [m("a", [cap], False)],
 "04-aai-string-true":         [m("a", [cap], "true")],
 "05-no-actions-v2":           [m("a", extra={"c2pa.actions": {"actions": [cap]}})],
 "06-empty-actions":           [m("a", [], True)],
 "07-actions-key-missing":     [m("a", raw_actions={"allActionsIncluded": True})],
 "08-custom-action":           [m("a", [cap, {"action": "com.example.filter"}], True)],
 "09-c2pa-unknown":            [m("a", [cap, {"action": "c2pa.unknown"}], True)],
 "10-action-field-missing":    [m("a", [cap, {"digitalSourceType": D + "digitalArt"}], True)],
 "11-actions-gathered":        [m("a", [cap], True, gathered=True)],
 "12-opened-capture":          [m("a", [{"action": "c2pa.opened", "digitalSourceType": D + "digitalCapture"}], True)],
 "13-genai-created-and-edited":[m("a", [{"action": "c2pa.created", "digitalSourceType": D + "trainedAlgorithmicMedia"},
                                       {"action": "c2pa.edited", "digitalSourceType": D + "trainedAlgorithmicMedia"}], True)],
 "14-edit-no-dst":             [m("a", [cap, {"action": "c2pa.color_adjustments"}], True)],
 "15-edit-nongenai-dst":       [m("a", [cap, {"action": "c2pa.cropped", "digitalSourceType": D + "algorithmicallyEnhanced"}], True)],
 "16-edit-composite-genai":    [m("a", [cap, {"action": "c2pa.edited", "digitalSourceType": D + "compositeWithTrainedAlgorithmicMedia"}], True)],
 "17-edit-digitalart":         [m("a", [cap, {"action": "c2pa.edited", "digitalSourceType": D + "digitalArt"}], True)],
 "18-transcoded-only":         [m("a", [{"action": "c2pa.transcoded"}], True)],
 "19-short-dst":               [m("a", [{"action": "c2pa.created", "digitalSourceType": "trainedAlgorithmicMedia"}], True)],
 "20-https-dst":               [m("a", [{"action": "c2pa.created", "digitalSourceType": "https://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia"}], True)],
 "21-ingredient-v3-no-active": [m("a", [cap], True, extra={"c2pa.ingredient.v3": {"relationship": "parentOf"}})],
 "22-ingredient-v3-suffixed":  [m("a", [cap], True, extra={"c2pa.ingredient.v3__1": {"relationship": "parentOf"}})],
 "23-ingredient-v2-no-active": [m("a", [cap], True, extra={"c2pa.ingredient.v2": {"relationship": "parentOf"}})],
 "24-genai-dst-on-created-composite": [m("a", [{"action": "c2pa.created", "digitalSourceType": D + "compositeWithTrainedAlgorithmicMedia"}], True)],
 "25-placed-genai":            [m("a", [cap, {"action": "c2pa.placed", "digitalSourceType": D + "trainedAlgorithmicMedia"}], True)],
 "26-two-created":             [m("a", [cap, {"action": "c2pa.created", "digitalSourceType": D + "trainedAlgorithmicMedia"}], True)],
 # chains
 "30-chain-edit-of-capture":   [m("top", [{"action": "c2pa.opened"}, {"action": "c2pa.edited", "digitalSourceType": D + "trainedAlgorithmicMedia"}], True,
                                  extra={"c2pa.ingredient.v3": {"relationship": "parentOf", "activeManifest": {"url": "self#jumbf=/c2pa/urn:c2pa:cam/c2pa.assertions/c2pa.claim.v2"}}}),
                                m("cam", [cap], True)],
 "31-flat-ingredient-dst":     [m("top", [{"action": "c2pa.opened"}], True,
                                  extra={"c2pa.ingredient.v3": {"relationship": "parentOf", "dc:title": "photo.jpg", "dc:format": "image/jpeg",
                                                                "digitalSourceType": D + "digitalCapture"}})],
 "32-flat-ingredient-opened-dst": [m("top", [{"action": "c2pa.opened", "digitalSourceType": D + "digitalCapture"}], True,
                                  extra={"c2pa.ingredient.v3": {"relationship": "parentOf", "dc:title": "x y.jpg"}})],
 "33-flat-ingredient-no-dst":  [m("top", [{"action": "c2pa.opened"}], True,
                                  extra={"c2pa.ingredient.v3": {"relationship": "parentOf", "dc:title": "x.jpg"}})],
 "34-two-ingredients-suffixed":[m("top", [{"action": "c2pa.placed"}], True,
                                  extra={"c2pa.ingredient.v3": {"relationship": "componentOf", "activeManifest": {"url": "self#jumbf=/c2pa/urn:c2pa:a"}},
                                         "c2pa.ingredient.v3__1": {"relationship": "componentOf", "activeManifest": {"url": "self#jumbf=/c2pa/urn:c2pa:b"}}}),
                                m("a", [cap], True), m("b", [{"action": "c2pa.created", "digitalSourceType": D + "trainedAlgorithmicMedia"}], True)],
}


PROBES = [
 # literals, fields, quoting
 ('"a\\"b"', None), ("`[1, 2.5, \"x\", null, true]`", None), ("1.5e3", None), (".5", None), ("12", None),
 ("a.b", {"a": {"b": 1}}), ("a.'b.c'", {"a": {"b.c": 2}}), ("'a b'", {"a b": 3}), ("a.b", {"a": [1]}),
 ("a.'0'", {"a": [9, 8]}), ("null", {"null": 7}), ("null", {}), ("$x", {"$x": 5}), ("@", 4), ("a.b.c", {"a": None}),
 # comparisons
 ("`1` == `1.0`", None), ("`true` == `1`", None), ("`[1]` == `[1.0]`", None), ("`{\"a\":1}` == `{\"a\":true}`", None),
 ("x = 1", {"x": 1}), ("x <> 1", {"x": 2}), ('"a" < "b"', None), ('`2` < "10"', None), ('"x" < `1`', None),
 ("`[1]` < `2`", None), ("`null` < `1`", None), ('`true` >= `1`', None), ('"" < `1`', None), ('" 12 " > `11`', None),
 # logic
 ("a || b", {"a": 0, "b": "x"}), ("a && b", {"a": [], "b": 1}), ("a && b", {"a": 1, "b": "y"}), ('!""', None), ("!`{}`", None),
 ("!a || b", {"a": 1, "b": 2}), ("a == `1` && b == `2` || c", {"a": 1, "b": 3, "c": "z"}),
 # filters and projections
 ("a[? b == `1`]", {"a": {"b": 1}}), ("a[? b == `1`].c", {"a": [{"b": 1, "c": "x"}, {"b": 2, "c": "y"}]}),
 ("a[? b]", {"a": [{"b": 0}, {"b": []}, {"b": "s"}]}), ("a[*].b", {"a": [{"b": 1}, {}, {"b": 2}]}), ("a[]", {"a": [[1], 2, [3, [4]]]}),
 ("a.*", {"a": {"x": 1, "y": 2}}), ("a[1:]", {"a": [1, 2, 3]}), ("a[::-1]", {"a": [1, 2, 3]}), ("a[-1]", {"a": [1, 2, 3]}),
 ("a[5]", {"a": [1]}), ("[a, b]", {"a": 1, "b": 2}), ("{x: a, y: b}", {"a": 1, "b": 2}), ("a | b", {"a": {"b": 5}}),
 # functions and coercion
 ("length(null)", None), ("length(`5`)", None), ("length(`true`)", None), ('length("héllo")', None), ('length(`{"a":1}`)', None),
 ("length(a)", {"a": [1, 2]}), ("length(a, b)", {}), ('contains(null, "x")', None), ('contains(null, "")', None),
 ('contains("abc", `1`)', None), ('contains(`5`, "5")', None), ("contains(`[1, 1.0]`, `1.0`)", None), ('contains("abc", "b")', None),
 ("contains(`[\"a\"]`, null)", None), ("contains(`[null]`, null)", None), ('contains(`{"a":1}`, "a")', None),
 ('startsWith(`["c2pa.a", "x"]`, "c2pa.")', None), ('startsWith(`5`, "5")', None), ('startsWith(`1.0`, "1.0")', None),
 ('startsWith(`1e20`, "1e+20")', None), ('startsWith(`0.0001`, "0.0001")', None), ('startsWith(`0.00001`, "1e-05")', None),
 ('startsWith(`123456789012345680000`, "1.2345678901234568e+20")', None), ('startsWith(null, "")', None),
 ('startsWith(`true`, "true")', None), ('startsWith(`[1, 2]`, "1")', None), ('startsWith(`[]`, "x")', None), ('startsWith(`{}`, "x")', None),
 ('startsWith(`[[1, 2], "x"]`, "1")', None), ("nosuch(a)", {}),
 # arithmetic, concatenation, union
 ("`1` + `2`", None), ("`1` + `2.0`", None), ("`7` / `2`", None), ("`1` / `0`", None), ('"2" * `3`', None), ('"x" * `3`', None),
 ("`[1, 2]` + `[10]`", None), ("-a", {"a": 3}), ('"a" & "b"', None), ("`1` & `true`", None), ("`[1, 2]` & \"x\"", None),
 ("a ~ b", {"a": [1], "b": None}), ("`1.5` * `2`", None),
 # syntax errors
 ('a "b"', {}), ("(a, b)", {}), ("()", {}), ("a.", {}), ("[1, ]", {}), ("{}", {}), ("'unterminated", {}), ("a\r", {}),
 ("'f'(1)", {}), ("a[? b", {}),
]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--rubrics", default="../conformance-public")
    ap.add_argument("--out", default="testdata/rubric-signals")
    a = ap.parse_args()
    rub = Path(a.rubrics) / "asset-rubrics"
    evaluator, rubric = rub / "c2pa_signals_rubric_evaluator.py", rub / "asset-rubric-signals-local.yml"
    if not evaluator.is_file() or not rubric.is_file():
        sys.exit(f"no evaluator under {rub} (git clone https://github.com/c2pa-org/conformance-public {a.rubrics})")
    out = Path(a.out)
    (out / "in").mkdir(parents=True, exist_ok=True)
    (out / "expected").mkdir(parents=True, exist_ok=True)
    for name, manifests in C.items():
        inp = out / "in" / f"{name}.json"
        inp.write_text(json.dumps({"manifests": manifests}, indent=1) + "\n")
        r = subprocess.run([sys.executable, str(evaluator), str(rubric), str(inp)],
                           capture_output=True, text=True)
        errors = [l for l in r.stderr.splitlines() if l.startswith("Error evaluating")]
        if r.returncode != 0 or errors:
            sys.exit(f"{name}: evaluator failed or reported errors:\n{r.stderr}")
        (out / "expected" / f"{name}.json").write_text(r.stdout)
    # Expression-level probes: json-formula-py's result for each, or the fact that it failed.
    from json_formula import JsonFormula
    probes = []
    for expr, data in PROBES:
        try:
            result = JsonFormula().search(expr, data)
            json.dumps(result)
            probes.append({"expr": expr, "data": data, "result": result})
        except Exception as exc:  # the rubric evaluators treat every failure alike
            probes.append({"expr": expr, "data": data, "error": type(exc).__name__})
    (out / "formula-probes.json").write_text(json.dumps(probes, indent=1, ensure_ascii=False) + "\n")
    # How PyYAML (the reference evaluator's YAML library) reads the rubric.
    import yaml
    docs = list(yaml.safe_load_all(rubric.read_text()))
    (out / "rubric-parsed.json").write_text(json.dumps(docs, indent=1, default=str, ensure_ascii=False) + "\n")
    commit = subprocess.run(["git", "-C", a.rubrics, "rev-parse", "--short", "HEAD"],
                            capture_output=True, text=True).stdout.strip()
    (out / "VERSIONS.txt").write_text(
        f"conformance-public: {commit}\n"
        f"rubric version: {(rub / 'VERSION').read_text().strip()}\n"
        f"rubric sha256: {hashlib.sha256(rubric.read_bytes()).hexdigest()}\n"
        f"json-formula-py: 327a8d75669673c10db3051da816311ac2df8a32\n"
        f"cases: {len(C)}\n"
        f"probes: {len(PROBES)}\n")
    print(f"{len(C)} cases and {len(PROBES)} probes written to {out}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
