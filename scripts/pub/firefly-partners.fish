#!/usr/bin/env fish
# Frame for the Firefly partner post: partner-model downloads carry OpenAI's and
# Google's own manifests, untouched, with no Adobe manifest in the store; Firefly's
# own model output carries only a reference to a manifest on Adobe's server.
# Everything here is offline. c2patool is only run live on the two partner files:
# on the remote-only file it would fetch by default, so that row uses the
# collector's offline record instead.
#
#   fish scripts/pub/firefly-partners.fish          # print the frame
#   fish scripts/pub/firefly-partners.fish --png    # render scripts/pub/out/firefly-partners.png
#
# Requires: ht, c2patool, jq, grep; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l g corpus/differential/04-generators
set -l exp $g/expectations.json
set -l out scripts/pub/out/firefly-partners.png
# label:file:kind (kind = partner runs c2patool live; adobe does not)
set -l rows \
    "GPT Image 2.5 Flare, via Firefly:adobe-firefly__gpt-image-2.5-flare__web-download__p1__1.png:partner" \
    "Nano Banana 2, via Firefly:adobe-firefly__nano-banana-2__web-download__p1__1.png:partner" \
    "Firefly Image 5, Adobe's own model:adobe-firefly__firefly-image-5__web-download-thumb__p1__1.png:adobe"

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self" \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 104 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
for tool in ht c2patool jq grep
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

set -l collected (jq -r .collected_at $exp)
echo "$D# Adobe Firefly downloads, inspected offline "(date -u '+%Y-%m-%d')$R

for row in $rows
    set -l kv (string split : $row)
    set -l f $g/$kv[2]
    test -f $f; or begin
        echo "missing $f" >&2
        exit 1
    end

    echo
    echo "$H== $kv[1]$R  $D$kv[2]$R"

    # Halftone, offline (no --fetch-remote-manifests)
    echo -n "ht        "
    ht inspect --batch --json --only manifest $f 2>/dev/null | jq -C -c \
        '.inspections[0].evidence[] | select(.source.name=="c2pa")
         | {status, state: .details.validation_state, issuer: .details.issuer,
            generator: .details.claim_generator, remote: .details.remote_manifest_url}
         | with_entries(select(.value != null))'

    # c2patool as recorded by the collector (offline, vendored trust + EKU config)
    echo -n "c2patool  "
    jq -C -c --arg k $kv[2] '.files[$k].c2patool
        | {state: .validation_state, issuer, generator: .claim_generator, remote: .remote_manifest}
        | with_entries(select(.value != null))' $exp

    # every manifest in the store, not just the active one
    if test $kv[3] = partner
        echo -n "signers   "
        c2patool $f 2>/dev/null | jq -C -c '[.manifests[]?.signature_info.issuer] | unique'
    end

    # does anything in the file's bytes name Adobe or Firefly?
    set -l hits (LC_ALL=C grep -a -o -i -E '.{0,24}(adobe|firefly).{0,24}' $f | string replace -ra '[^[:print:]]' '.')
    echo "adobe/firefly in bytes: "(count $hits)
    for h in $hits[1..3]
        echo "$D  $h$R"
    end
end

echo
echo "$D# c2patool rows: expectations.json, collected $collected$R"
