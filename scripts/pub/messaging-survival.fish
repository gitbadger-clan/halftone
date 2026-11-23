#!/usr/bin/env fish
# Frame for the messaging post (10-messaging, collected 2026-11-12 and 2026-11-13):
# five inputs, one trip each through WhatsApp, Telegram and Signal, per send mode.
# For each route: how many delivered files are the input's own bytes, and how many
# still carry what the input carried (a C2PA manifest; an AI label in XMP).
#
#   fish scripts/pub/messaging-survival.fish          # print the frame
#   fish scripts/pub/messaging-survival.fish --png    # render scripts/pub/out/messaging-survival.png
#   fish scripts/pub/messaging-survival.fish --identifiers [--png]
#       the same table with one more column: the inputs' embedded identifiers
#       (account, tracking, person, device, document, location fields), kept or not.
#       Renders scripts/pub/out/label-vs-identifiers.png. Presence only: no field
#       name and no value is printed.
#
# Every count is computed at run time from one `ht inspect --batch --json` over the
# delivered files and their inputs; nothing is typed in. A delivered file whose
# name matches no route or no input stops the run.
#
# Override the binary with an exported HT (set -x, so freeze's re-run sees it).
# Requires: ht (built with c2pa), jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -q HT; or set -g HT ./target/release/ht
# Not with a binary older than the source: the frame would show the old code's
# output (scripts/lib/htbin.py). `HT_STALE_OK=1 <this script>` runs anyway. Checked
# here, before the render mode, so --png stops too and does not draw the error.
uv run scripts/ht-check.py -q --ht $HT; or exit 2

for a in $argv
    contains -- $a --png --identifiers; or begin
        echo "unknown option $a (use --png, --identifiers)" >&2
        exit 2
    end
end
set -l out scripts/pub/out/messaging-survival.png
set -l ids
if contains -- --identifiers $argv
    set ids --identifiers
    set out scripts/pub/out/label-vs-identifiers.png
end

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self $ids" \
        --execute.timeout 2m \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 132 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    or exit $status
    echo "wrote $out"
    exit 0
end

# --- setup --------------------------------------------------------------------
set -l c corpus/differential
set -l m $c/10-messaging

function die
    echo "error: $argv" >&2
    exit 1
end

type -q jq; or die "jq not found on PATH"
test -x $HT; or die "ht not executable at $HT (cargo build --release -p halftone-cli --features c2pa, or set -x HT)"

# token__subject in a delivered name | input file | label   (10-messaging/SOURCE.md, "Inputs")
set -l inputs \
    "gemini-app-nano-banana__p1|04-generators/gemini-app__nano-banana__web-download__p1__1.jpg|Gemini app" \
    "chatgpt-base__p1|04-generators/chatgpt__base__web-download__p1__1.png|ChatGPT" \
    "meta-ai-instant__p1|04-generators/meta-ai__instant__web-download-thumb__p1__1.jpg|Meta AI" \
    "canva-canva-ai__p1|04-generators/canva__canva-ai__web-download__p1__2.png|Canva AI" \
    "pixel9fold-default__s01|06-phones/pixel9fold__default__adb-pull__s01__1.jpg|Pixel 9 Pro Fold camera"

# app__mode in a delivered name | app | "sent as"   (the table's row order)
set -l routes \
    "whatsapp__document|WhatsApp|document" \
    "whatsapp__photo|WhatsApp|photo" \
    "whatsapp__photo-hd|WhatsApp|photo, HD" \
    "telegram__file|Telegram|file" \
    "telegram__photo|Telegram|photo" \
    "telegram__photo-hd|Telegram|photo, HD" \
    "signal__file|Signal|file" \
    "signal__file-android|Signal|file, from the phone" \
    "signal__photo|Signal|photo" \
    "signal__photo-hd|Signal|photo, HD"

set -l in_files
for i in $inputs
    set -l p (string split '|' -- $i)
    test -f $c/$p[2]; or die "missing input $c/$p[2]"
    set -a in_files $c/$p[2]
end

# Delivered files: everything in the stratum that is named like a delivery.
set -l delivered
for f in $m/*__*__*__*__*
    set -a delivered $f
end
test (count $delivered) -gt 0; or die "no delivered files under $m"

# Every delivered file must belong to a listed route and a listed input.
set -l route_keys (string replace -r '\|.*' '' -- $routes)
set -l input_keys (string replace -r '\|.*' '' -- $inputs)
for f in $delivered
    set -l p (string split __ -- (path basename $f))
    contains -- "$p[1]__$p[2]" $route_keys; or die "no route listed for "(path basename $f)
    contains -- "$p[3]__$p[4]" $input_keys; or die "no input listed for "(path basename $f)
end

# One pass over inputs and delivered files:  name <tab> sha256 <tab> manifest <tab> label <tab> identifiers
#   manifest    = the c2pa source found one (any state other than absent)
#   label       = IPTC DigitalSourceType says AI (marking_metadata) or a vendor AI flag says yes
#   identifiers = embedded_identifiers lists at least one (values stay masked: ht's default)
set -g facts ($HT inspect --batch --json --trust-list vendored $in_files $delivered 2>/dev/null | jq -r '
    .inspections[]
    | (.evidence | map({key: .source.name, value: .status}) | from_entries) as $s
    | [ (.asset.path | split("/") | last),
        .asset.sha256,
        (if ($s.c2pa // "absent") != "absent" and $s.c2pa != "not_applicable" then 1 else 0 end),
        (if $s.marking_metadata == "present" or $s.vendor_ai_flag == "present" then 1 else 0 end),
        (if $s.embedded_identifiers == "present" then 1 else 0 end) ]
    | @tsv')
test (count $facts) -eq (math (count $in_files) + (count $delivered)); or die "ht did not report on every file ("(count $facts)" of "(math (count $in_files) + (count $delivered))")"

# fact <file name> <column>: 2 = sha256, 3 = manifest, 4 = label, 5 = identifiers
function fact --argument-names name col
    for l in $facts
        set -l p (string split \t -- $l)
        if test "$p[1]" = "$name"
            echo $p[$col]
            return 0
        end
    end
    die "no ht result for $name"
end

# A build without the c2pa feature would report no manifest anywhere.
set -l signed 0
for f in $in_files
    test (fact (path basename $f) 3) -eq 1; and set signed (math $signed + 1)
end
test $signed -gt 0; or die "no input reads as carrying a manifest: is ht built with --features c2pa?"

# "k of n", padded first and coloured after so columns stay aligned.
# With a fourth argument the count is printed uncoloured: for a column where
# "kept" is neither good nor bad news.
function kept --argument-names k n width plain
    if test $n -eq 0
        echo -n (set_color brblack)(string pad -r -w $width -- —)(set_color normal)
        return
    end
    set -l colour yellow
    test $k -eq $n; and set colour brgreen
    test $k -eq 0; and set colour brred
    test -n "$plain"; and set colour normal
    echo -n (set_color $colour)(string pad -r -w $width -- "$k of $n")(set_color normal)
end

# --- frame ------------------------------------------------------------------------
set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

echo "$D# One trip through a messaging app: "(count $in_files)" inputs, "(count $delivered)" delivered files, checked "(date -u '+%Y-%m-%d %H:%M')" UTC$R"
echo "$D# "($HT --version 2>&1 | string join ' ')"$R"

echo
echo "$H== the inputs, and what each carries$R"
for i in $inputs
    set -l p (string split '|' -- $i)
    set -l name (path basename $p[2])
    set -l has
    test (fact $name 3) -eq 1; and set -a has "C2PA manifest"
    test (fact $name 4) -eq 1; and set -a has "AI label in XMP"
    test -n "$ids"; and test (fact $name 5) -eq 1; and set -a has identifiers
    set -q has[1]; or set has nothing
    printf '%-26s %s\n' $p[3] (string join ' + ' -- $has)
end

echo
echo "$H== what arrived$R"
if test -n "$ids"
    printf "$D%-10s %-25s %-7s %-12s %-15s %-15s %s$R\n" app "sent as" files "same bytes" "manifest kept" "AI label kept" "identifiers kept"
else
    printf "$D%-10s %-25s %-7s %-12s %-15s %s$R\n" app "sent as" files "same bytes" "manifest kept" "AI label kept"
end
for r in $routes
    set -l p (string split '|' -- $r)
    set -l n 0
    set -l same 0
    set -l man_in 0
    set -l man_out 0
    set -l lab_in 0
    set -l lab_out 0
    set -l id_in 0
    set -l id_out 0
    for f in $delivered
        set -l name (path basename $f)
        set -l q (string split __ -- $name)
        test "$q[1]__$q[2]" = $p[1]; or continue
        set -l src
        for i in $inputs
            set -l ip (string split '|' -- $i)
            test "$ip[1]" = "$q[3]__$q[4]"; and set src (path basename $ip[2])
        end
        set n (math $n + 1)
        test (fact $name 2) = (fact $src 2); and set same (math $same + 1)
        if test (fact $src 3) -eq 1
            set man_in (math $man_in + 1)
            test (fact $name 3) -eq 1; and set man_out (math $man_out + 1)
        end
        if test (fact $src 4) -eq 1
            set lab_in (math $lab_in + 1)
            test (fact $name 4) -eq 1; and set lab_out (math $lab_out + 1)
        end
        if test (fact $src 5) -eq 1
            set id_in (math $id_in + 1)
            test (fact $name 5) -eq 1; and set id_out (math $id_out + 1)
        end
    end
    test $n -gt 0; or continue
    printf '%-10s %-25s %-7s ' $p[2] $p[3] $n
    kept $same $n 13
    kept $man_out $man_in 16
    if test -n "$ids"
        kept $lab_out $lab_in 16
        kept $id_out $id_in 13 plain
    else
        kept $lab_out $lab_in 13
    end
    echo
end

echo
echo "$D# same bytes: the delivered file's sha256 equals its input's$R"
echo "$D# kept: still in the delivered file, out of the inputs on that route that had it; — = none had it$R"
echo "$D# AI label: IPTC DigitalSourceType or a vendor AI flag in XMP (both unsigned)$R"
test -n "$ids"; and echo "$D# identifiers: account, tracking, person, device, document or location fields in the metadata (unsigned)$R"
echo "$D# each row: desktop app to my own chat, read back from a phone; \"from the phone\": Android to desktop$R"

