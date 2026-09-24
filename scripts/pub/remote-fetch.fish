#!/usr/bin/env fish
# Frame for the remote-manifest post: one file, three verdicts, the only
# difference being whether the verifier is allowed to call out.
#
#   fish scripts/pub/remote-fetch.fish          # print the frame
#   fish scripts/pub/remote-fetch.fish --png    # render scripts/pub/out/remote-fetch.png
#
# Note: the first run makes a real request to cai-manifests.adobe.com. That
# request is the point of the post. If it fails, the frame shows the error,
# which also makes the point.

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l f corpus/differential/01-c2pa-rs/cloud.jpg
set -l out scripts/pub/out/remote-fetch.png

# --- render mode ------------------------------------------------------------
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
        --wrap 96 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
for tool in ht c2patool jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
test -f $f; or begin
    echo "missing $f" >&2
    exit 1
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

# Settings identical to the differential collector's, fetching off.
# mktemp creates the base file; the .toml copy is what c2patool reads. Remove both.
set -l tmp (mktemp -t c2pa-offline)
set -l offline $tmp.toml
printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\n' >$offline

echo "$D# "(basename $f)"  sha256 "(shasum -a 256 $f | cut -c1-12)"  "(date -u +%Y-%m-%d)$R

# 1. Default settings: the file's link is fetched.
echo "$H== c2patool, default settings (fetch on)$R"
c2patool $f 2>&1 | jq -C -c '
    .active_manifest as $a
    | {validation_state, issuer: .manifests[$a].signature_info.issuer}' 2>/dev/null
or echo "(error: "(c2patool $f 2>&1 | tail -1 | string sub -l 80)")"

# 2. Same tool, fetch off: nothing inside the file to verify.
echo "$H== c2patool, fetch off$R"
c2patool --settings $offline $f 2>&1 \
    | string match -r 'must fetch remote manifests from url \S+' \
    | string replace -r 'manifests/.*' 'manifests/…'

# 3. Halftone: offline by construction.
echo "$H== ht inspect$R"
ht inspect --batch --json --only manifest $f | jq -C -c '
    .inspections[0].evidence[] | select(.source.name=="c2pa")
    | {status, remote: (.details.remote_manifest_url | sub("/manifests/.*"; "/manifests/…"))}'

rm -f $tmp $offline
