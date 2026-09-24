#!/usr/bin/env fish
# Frame for the cert-expiry post: one Canva file, before (git record) and after
# (today), plus the certificate window and c2patool's second opinion.
#
#   fish scripts/pub/cert-expiry.fish          # print the frame to the terminal
#   fish scripts/pub/cert-expiry.fish --png    # render scripts/pub/out/cert-expiry.png
#
# Requires: ht, c2patool, jq, openssl, git; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l exp corpus/differential/04-generators/expectations.json
set -l name canva__canva-ai__web-download__p1__2.png
set -l f corpus/differential/04-generators/$name
set -l before aaf1c79 # last commit where the file read Valid
set -l out scripts/pub/out/cert-expiry.png

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
        --wrap 96 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
for tool in ht c2patool jq openssl
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
test -f $f; or begin
    echo "missing $f" >&2
    exit 1
end

# jq -C forces colour even when stdout is not a terminal (freeze's pty is, but
# be explicit). Bold headers via set_color.
set -l H (set_color --bold brcyan)
set -l R (set_color normal)

set -l before_date (git log -1 --format=%cs $before)
echo "$H== $exp @ $before  $before_date$R"
git show $before:$exp | jq -C -c --arg k $name \
    '.files[$k] | {sha256: .sha256[0:12], state: .c2patool.validation_state, codes: .c2patool.validation_codes}'

echo "$H== ht inspect "(date -u +%Y-%m-%d)"  sha256 "(shasum -a 256 $f | cut -c1-12)"$R"
ht inspect --batch --json --only manifest $f | jq -C -c \
    '.inspections[0].evidence[] | select(.source.name=="c2pa")
     | {status, state: .details.validation_state, codes: [.details.validation_status[].code], issuer: .details.issuer}'

echo "$H== signing certificate$R"
c2patool $f --certs | openssl x509 -noout -subject -dates

echo "$H== c2patool$R"
c2patool $f | jq -C -c '{validation_state, codes: [.validation_status[]?.code]}'
