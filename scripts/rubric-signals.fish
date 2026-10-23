#!/usr/bin/env fish
# C2PA signals rubric over one or more files: Credentio exports crJSON, the C2PA
# conformance repo's evaluator applies asset-rubric-signals-local.yml, and this
# prints one line per manifest node, then whether each actions assertion sets
# allActionsIncluded (D-013 addendum, D-016).
#
#   fish scripts/rubric-signals.fish FILE...
#   fish scripts/rubric-signals.fish corpus/differential/04-generators/google-flow__*__e2__2.jpg
#
# Override with CREDENTIO (default ~/oss-bins/c2pa_validate), RUBRICS (a checkout of
# github.com/c2pa-org/conformance-public, default ../conformance-public) and OUT_ROOT
# (default ../rubric-runs). Each run writes <name>.crjson, <name>.signals.json and the
# evaluator's log per file, plus VERSIONS.txt, to OUT_ROOT/<timestamp>/.
# Requires: uv, jq, git. Never modifies the inputs; never reads the clipboard.

function die
    echo "error: $argv" >&2
    exit 1
end

set -l root (git rev-parse --show-toplevel); or exit 1
cd $root

set -q CREDENTIO; or set CREDENTIO ~/oss-bins/c2pa_validate
set -q RUBRICS; or set RUBRICS ../conformance-public
set -q OUT_ROOT; or set OUT_ROOT ../rubric-runs

# json-formula is not on PyPI; pinned to the commit the D-013 addendum was produced with.
set -l json_formula 'git+https://github.com/adobe/json-formula-py.git@327a8d75669673c10db3051da816311ac2df8a32'

set -l trust crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem
set -l tsa crates/halftone-c2pa/trust/C2PA-TSA-TRUST-LIST.pem
set -l rub $RUBRICS/asset-rubrics
set -l evaluator $rub/c2pa_signals_rubric_evaluator.py
set -l rubric $rub/asset-rubric-signals-local.yml

test (count $argv) -ge 1; or die "usage: fish scripts/rubric-signals.fish FILE..."
for c in uv jq git
    command -q $c; or die "$c not on PATH"
end
test -x $CREDENTIO; or die "Credentio not executable at $CREDENTIO (set CREDENTIO)"
test -f $evaluator; and test -f $rubric
or die "no rubric evaluator under $rub (git clone https://github.com/c2pa-org/conformance-public $RUBRICS)"
for p in $trust $tsa
    test -f $p; or die "missing $p"
end
for f in $argv
    test -f $f; or die "no such file: $f"
end

mkdir -p $OUT_ROOT; or die "cannot create $OUT_ROOT"
set -l run (realpath $OUT_ROOT)/(date +%Y-%m-%dT%H%M)
mkdir -p $run; or die "cannot create $run"

begin
    echo "date:        "(date -u +%FT%TZ)
    echo "credentio:   "(string replace -r '^c2pa_validate-' '' (basename (realpath $CREDENTIO)))" ("(realpath $CREDENTIO)")"
    echo "rubrics:     conformance-public "(git -C $RUBRICS rev-parse --short HEAD)", rubric version "(string trim <$rub/VERSION)
    echo "json-formula: $json_formula"
    echo "trust:       $trust"
    echo "tsa:         $tsa"
    for f in $argv
        echo "input:       "(shasum -a 256 $f | string sub -l 64)"  $f"
    end
end >$run/VERSIONS.txt

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

for f in $argv
    set -l n (basename $f | string replace -r '\.[^.]+$' '')
    set -l cj $run/$n.crjson
    set -l sj $run/$n.signals.json

    $CREDENTIO --asset=$f --claim_signer_trust=$trust --tsa_trust=$tsa \
        --output_format=crjson --output_file=$cj 2>$run/$n.credentio.log
    test -s $cj; or die "Credentio wrote no crJSON for $f (see $run/$n.credentio.log)"

    # The evaluator prints its result to stdout and progress lines to stderr.
    uv run --quiet --with PyYAML --with $json_formula python $evaluator $rubric $cj >$sj 2>$run/$n.evaluator.log
    or die "evaluator failed on $cj (see $run/$n.evaluator.log)"
    jq -e '.manifests' $sj >/dev/null 2>&1; or die "evaluator output is not the expected JSON: $sj"

    echo
    echo "$H== $n$R"
    echo "$D""node  signer             inception           transformations                             ingredients$R"
    jq -r '.manifests | to_entries[] |
        [ (.key | tostring),
          (.value.assertedBy.subject.O // "-"),
          ([.value.localInceptions[].trait | sub("inception:signal_"; "")] | join(",") | if . == "" then "-" else . end),
          ([.value.localTransformations[].trait | sub("transformation:signal_"; "")] | join(",") | if . == "" then "-" else . end),
          ([.value.ingredients[] | "\(.index) \(.relationship)"] | join(", ") | if . == "" then "-" else . end)
        ] | @tsv' $sj | while read -l -d \t i o inc tr ing
        printf '%-5s %-18s %-19s %-43s %s\n' $i $o $inc $tr $ing
    end

    # allActionsIncluded per actions assertion, read from the crJSON itself (D-016).
    echo "$D""manifest (label tail)  allActionsIncluded$R"
    jq -r '.manifests[] | [(.label // "-" | .[-12:]),
        (.assertions."c2pa.actions.v2" | if . == null then "no actions.v2"
         elif has("allActionsIncluded") then (.allActionsIncluded | tostring) else "absent" end)] | @tsv' $cj \
        | while read -l -d \t l a
        printf '%-22s %s\n' $l $a
    end
end

echo
echo "$D""raw outputs and VERSIONS.txt in $run$R"
