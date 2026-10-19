#!/usr/bin/env fish
# Frame for the Canva prediction post: the October exports read Valid, sit on two
# 8-day signing certificates and carry no time-stamp, so a verifier checking after
# not_after reports them expired. September's certificate already did this (D-010).
# Re-run after the flip for the follow-up: the `ht` and `c2patool` columns change,
# the `recorded` column (git) and the `bytes` check (sha256 vs the record) do not.
#
#   fish scripts/pub/canva-expiry.fish          # print the frame
#   fish scripts/pub/canva-expiry.fish --png    # render scripts/pub/out/canva-expiry.png
#
# Requires: ht, c2patool, jq, openssl, git, shasum; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l g corpus/differential/04-generators
set -l exp $g/expectations.json
set -l recorded_at 2026-10-10T19:42:03Z # the collection that recorded the October files
set -l sept canva__canva-ai__web-download__p1__2.png # September precedent (cert-expiry post)
set -l out scripts/pub/out/canva-expiry.png

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    # the frame runs ht/c2patool per file, well past freeze's default 10s
    freeze --execute "fish $self" \
        --execute.timeout 120s \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 120 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    or begin
        echo "freeze failed, $out not written" >&2
        exit 1
    end
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
for tool in ht c2patool jq openssl git shasum
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

# The October exports: every edit-photo row plus the two signed free-tier files
# (free p1__1 carries no manifest). Unmatched globs expand to nothing in `set`.
set -l files $g/canva__*__edit-photo__* \
    $g/canva__canva-ai-free__web-download__p1__3.png \
    $g/canva__canva-ai-free__web-download__p2__2.png
for f in $files $g/$sept
    test -f $f; or begin
        echo "missing $f" >&2
        exit 1
    end
end

# The record: the commit whose expectations.json was collected at $recorded_at.
# --reverse + [1] picks the commit that introduced it, even after a re-collect.
set -l before (git log --reverse --format=%h -S$recorded_at -- $exp)[1]
test -n "$before"; or begin
    echo "no commit in $exp history recorded $recorded_at" >&2
    exit 1
end
set -l rec (git show $before:$exp | string collect)

# serial, not_before, not_after of the signing (leaf) certificate; GMT dropped
function cert_info
    c2patool $argv[1] --certs 2>/dev/null \
        | openssl x509 -noout -serial -startdate -enddate 2>/dev/null \
        | string replace -r '^[^=]+=' '' | string replace ' GMT' ''
end

# any timeStamp.* code, from c2patool's full report
function ts_codes
    set -l ts (c2patool $argv[1] 2>/dev/null | jq -r \
        '[.. | objects | .code? // empty | select(startswith("timeStamp"))] | unique | join(",")')
    test -n "$ts"; and echo $ts; or echo none
end

function short_serial
    echo (string sub -l 6 $argv[1])..(string sub -s -6 $argv[1])
end

# canva__<variant>__<route>__<prompt>__<take>.png -> "<variant> <route> <prompt>/<take>"
function label
    set -l p (string split __ (string replace -r '\.\w+$' '' (basename $argv[1])))
    echo (string replace canva- '' $p[2]) $p[3] $p[4]/$p[5]
end

# state padded to a width, coloured; padding before colour keeps columns aligned
function paint
    switch $argv[1]
        case Valid Trusted
            set -f c brgreen
        case Invalid
            set -f c brred
        case '*'
            set -f c yellow
    end
    echo (set_color $c)(string pad -r -w $argv[2] -- $argv[1])(set_color normal)
end

function ht_state
    ht inspect --batch --json --only manifest $argv[1] 2>/dev/null | jq -r \
        '.inspections[0].evidence[] | select(.source.name=="c2pa") | .details.validation_state // "-"'
end

function tool_state
    c2patool $argv[1] 2>/dev/null | jq -r '.validation_state // "-"'
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

echo "$D# Canva exports, checked "(date -u '+%Y-%m-%d %H:%M')" UTC$R"

# --- September: the precedent -------------------------------------------------
set -l c (cert_info $g/$sept)
echo
echo "$H== September  $D$sept$R"
echo "cert "(short_serial $c[1])"  $c[2] -> $c[3] GMT  time-stamp: "(ts_codes $g/$sept)
c2patool $g/$sept 2>/dev/null | jq -C -c '{validation_state, codes: [.validation_status[]?.code]}'

# --- October: one row per file ------------------------------------------------
echo
echo "$H== October ("(count $files)" files)$R"
printf "$D%-32s %-14s %-21s %-5s %-7s %-9s %-8s %s$R\n" \
    file cert "valid until (GMT)" ts bytes recorded ht c2patool
set -l certs
for f in $files
    set -l name (basename $f)
    set -l c (cert_info $f)
    set -l ts (ts_codes $f)
    set -l r (printf '%s' $rec | jq -r --arg k $name '.files[$k].c2patool.validation_state // "-"')
    # same bytes as when the state was recorded?
    set -l bytes same
    test (shasum -a 256 $f | cut -d' ' -f1) = (printf '%s' $rec | jq -r --arg k $name '.files[$k].sha256'); or set bytes CHANGED
    printf '%-32s %-14s %-21s %-5s %-7s ' (label $f) (short_serial $c[1]) $c[3] $ts $bytes
    echo (paint $r 9)" "(paint (ht_state $f) 8)" "(paint (tool_state $f) 8)
    set -a certs "$c[1]|$c[2]|$c[3]|$ts"
end
echo "$D# recorded = expectations.json @ $before, collected $recorded_at$R"

# --- per certificate: the batch view in miniature -----------------------------
echo
echo "$H== per signing certificate$R"
for line in (printf '%s\n' $certs | sort | uniq -c | string trim)
    set -l n (string split -m1 ' ' $line)
    set -l p (string split '|' (string trim $n[2]))
    echo (short_serial $p[1])"  $n[1] files  $p[2] -> $p[3] GMT  time-stamp: $p[4]"
end
