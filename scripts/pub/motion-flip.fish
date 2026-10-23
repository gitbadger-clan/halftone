#!/usr/bin/env fish
# Frame for the motion-photo post (06-phones): a Pixel 9 Pro Fold motion photo is
# one JPEG file with an MP4 clip appended, and its C2PA whole-file hash covers the
# clip. Flip one bit in the middle of the MP4 on a throwaway copy and both tools
# read Invalid, although no byte of the still image changed. Repeats the
# 2026-10-08 tamper test from 06 SOURCE.md. The corpus file is never modified.
# Both tools here are c2pa-rs. A validator that implements the multi-asset hash
# fallback (C2PA 2.4 §15.12.4) also rejects this copy, but accepts one with the
# clip removed: see D-014 and scripts/pub/multipart-reply.fish.
#
#   fish scripts/pub/motion-flip.fish          # print the frame
#   fish scripts/pub/motion-flip.fish --png    # render scripts/pub/out/motion-flip.png
#
# Requires: ht, c2patool, exiftool, jq, grep, od, dd, cmp; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo
set -l out scripts/pub/out/motion-flip.png

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
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
for tool in ht c2patool exiftool jq grep od dd cmp
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

# the Fold's s02 motion capture (the Pixel 5's motion rows are s04 and s02 raw-jpeg)
set -l src corpus/differential/06-phones/*__motion__*__s02__*
test (count $src) -eq 1; or begin
    echo "expected exactly one 06-phones motion s02 file, found "(count $src) >&2
    exit 1
end

# throwaway copy, removed however the script ends
set -g mf_tmp (mktemp -d)
function _mf_cleanup --on-event fish_exit
    rm -rf $mf_tmp
end
set -l before $mf_tmp/before.jpg
set -l after $mf_tmp/after.jpg
cp $src[1] $before
cp $src[1] $after

# The MP4 is the last item in the file: find its ftyp box (last match, so a
# stray "ftyp" in the JPEG data can't win) and step back over the 4-byte size.
set -l ftyp (LC_ALL=C grep -obUa ftyp $before | tail -1 | string split -f1 :)
test -n "$ftyp"; or begin
    echo "no MP4 found in $src[1]" >&2
    exit 1
end
set -l size (wc -c < $before | string trim)
set -l mp4 (math $ftyp - 4)
set -l mp4_len (math $size - $mp4)
set -l at (math "$mp4 + floor($mp4_len / 2)") # middle of the clip

function ht_view
    ht inspect --batch --json --only manifest $argv[1] 2>/dev/null | jq -C -c \
        '.inspections[0].evidence[] | select(.source.name=="c2pa")
         | {status, state: .details.validation_state, issuer: .details.issuer}
         | with_entries(select(.value != null))'
end

function tool_view
    c2patool $argv[1] 2>/dev/null | jq -C -c '{state: .validation_state, codes: [.validation_status[]?.code]}'
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

echo "$D# One bit in a motion photo's video, "(date -u '+%Y-%m-%d')$R
echo "device: "(exiftool -s3 -Make -Model $before | string join ' ')
printf 'file:   %s bytes, MP4 clip from byte %s (%s bytes, %s%% of the file)\n' \
    $size $mp4 $mp4_len (math -s0 "100 * $mp4_len / $size")

echo
echo "$H== untouched$R"
echo -n "ht        "
ht_view $before
echo -n "c2patool  "
tool_view $before

# flip the lowest bit of one byte in the middle of the clip
# macOS od can end with an empty line: keep only the number
set -l old (od -An -tu1 -j $at -N1 $after | string trim | string match -r '^[0-9]+$')[1]
set -l new (math "bitxor($old, 1)")
printf (printf '\\\\x%02x' $new) | dd of=$after bs=1 seek=$at conv=notrunc 2>/dev/null

echo
printf "$H== flip one bit at byte %s, inside the clip:$R  0x%02x -> 0x%02x\n" $at $old $new

echo
echo "$H== after$R"
echo -n "ht        "
ht_view $after
echo -n "c2patool  "
tool_view $after
set -l before_codes (c2patool $before 2>/dev/null | jq -c '[.validation_status[]?.code]')
echo "$D  new since untouched:$R"
c2patool $after 2>/dev/null | jq -r --argjson b "$before_codes" \
    '.validation_status[]? | select(.code as $c | $b | index($c) | not) | "  \(.code): \(.explanation)"'

# what actually differs between the two files (cmp -l counts from 1)
set -l diff (cmp -l $before $after | string trim)
echo
echo "$D# bytes that differ: "(count $diff)", at offset "(math (string split -n ' ' $diff[1])[1] - 1)", inside the clip; everything before byte $mp4 is identical$R"
