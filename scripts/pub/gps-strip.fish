#!/usr/bin/env fish
# Frame for the GPS-removal post (07-editors): strip location from a signed camera
# photo with the same ExifTool command as 07 and show the signature break.
# Works on copies in a temp dir that is deleted on exit: the input is never
# modified, and GPS values are never printed (tag counts only), so the frame is
# safe to publish. The photo itself never appears in the frame.
#
#   fish scripts/pub/gps-strip.fish <signed-photo.jpg>          # print the frame
#   fish scripts/pub/gps-strip.fish <signed-photo.jpg> --png    # render scripts/pub/out/gps-strip.png
#
# Requires: ht, c2patool, exiftool, jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l args (string match -v -- --png $argv)
test (count $args) -eq 1; or begin
    echo "usage: fish scripts/pub/gps-strip.fish <signed-photo.jpg> [--png]" >&2
    exit 2
end
set -l photo (realpath $args[1] 2>/dev/null); and test -f "$photo"; or begin
    echo "no such file: $args[1]" >&2
    exit 2
end
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo
set -l out scripts/pub/out/gps-strip.png

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self '$photo'" \
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
for tool in ht c2patool exiftool jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

# throwaway copies, removed however the script ends
set -g gps_tmp (mktemp -d)
function _gps_cleanup --on-event fish_exit
    rm -rf $gps_tmp
end
cp $photo $gps_tmp/before.jpg
cp $photo $gps_tmp/after.jpg
set -l before $gps_tmp/before.jpg
set -l after $gps_tmp/after.jpg

# number of GPS tags (EXIF GPS group + XMP GPS), never their values
function gps_count
    exiftool -j -gps:all '-xmp:gps*' $argv[1] 2>/dev/null | jq '.[0] | del(.SourceFile) | length'
end

function ht_view
    ht inspect --batch --json --only manifest $argv[1] 2>/dev/null | jq -C -c \
        '.inspections[0].evidence[] | select(.source.name=="c2pa")
         | {status, state: .details.validation_state, issuer: .details.issuer}
         | with_entries(select(.value != null))'
end

function tool_view
    c2patool $argv[1] 2>/dev/null | jq -C -c '{state: .validation_state, codes: [.validation_status[]?.code]}'
end

function bytes
    wc -c <$argv[1] | string trim
end

set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

set -l n_before (gps_count $before)
test "$n_before" -gt 0 2>/dev/null; or begin
    echo "the input has no GPS tags, so there is nothing to strip: shoot with location on" >&2
    exit 1
end

echo "$D# Removing location from a signed camera photo, "(date -u '+%Y-%m-%d')$R
echo "device: "(exiftool -s3 -Make -Model $before | string join ' ')

echo
echo "$H== before$R"
echo "GPS tags: $n_before"
echo -n "ht        "
ht_view $before
echo -n "c2patool  "
tool_view $before

echo
echo "$H== exiftool -gps:all= '-xmp:gps*=' -overwrite_original$R  $D(ExifTool "(exiftool -ver)")$R"
exiftool -q -gps:all= '-xmp:gps*=' -overwrite_original $after

echo
echo "$H== after$R"
set -l b0 (bytes $before)
set -l b1 (bytes $after)
printf 'GPS tags: %s   size: %s -> %s bytes (%+d)\n' (gps_count $after) $b0 $b1 (math $b1 - $b0)
echo -n "ht        "
ht_view $after
echo -n "c2patool  "
tool_view $after
# explanations only for what the strip added; codes already on the untouched file stay on the line above
set -l before_codes (c2patool $before 2>/dev/null | jq -c '[.validation_status[]?.code]')
echo "$D  new since before:$R"
c2patool $after 2>/dev/null | jq -r --argjson b "$before_codes" \
    '.validation_status[]? | select(.code as $c | $b | index($c) | not) | "  \(.code): \(.explanation)"'

echo
echo "$D# c2patool runs without a trust list here, so an intact file reads Valid, not Trusted$R"
