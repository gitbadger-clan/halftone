#!/usr/bin/env fish
# Frame for the screenshot post (11-screenshots, Pixel 9 Pro Fold, collected
# 2026-11-16): the status bar was in System UI demo mode, so the picture shows
# Android's placeholder clock. The file's EXIF block holds the real capture time,
# the system build and a per-screenshot ID.
#
#   fish scripts/pub/screenshot-time.fish               # print the frame
#   fish scripts/pub/screenshot-time.fish --png         # render scripts/pub/out/screenshot-time.png
#   fish scripts/pub/screenshot-time.fish --mask-time   # hide the clock digits, keep date, precision and offset
#   fish scripts/pub/screenshot-time.fish --file NAME   # use another Fold screenshot as the example
#
# The capture time is read with exiftool: no Halftone source reads it. `ht` supplies
# the writer verdict and the identifier listing. The ID's value is never printed, by
# either tool: the frame shows its length, counts distinct values, and stops if ht's
# output holds an unmasked identifier or if the two tools disagree on the length.
# What was on screen cannot be computed from here: that one line is quoted from
# 11-screenshots/SOURCE.md, and the frame stops if the notes no longer say it.
#
# Override the binary with an exported HT (set -x, so freeze's re-run sees it).
# Requires: ht, exiftool, jq; freeze for --png
#   brew install charmbracelet/tap/freeze

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

argparse png mask-time 'file=' -- $argv
or begin
    echo "usage: screenshot-time.fish [--png] [--mask-time] [--file NAME]" >&2
    exit 2
end
test (count $argv) -eq 0; or begin
    echo "unexpected argument $argv[1] (use --png, --mask-time, --file NAME)" >&2
    exit 2
end

set -q HT; or set -g HT ./target/release/ht
# Not with a binary older than the source: the frame would show the old code's
# output (scripts/lib/htbin.py). `HT_STALE_OK=1 <this script>` runs anyway. Checked
# here, before the render mode, so --png stops too and does not draw the error.
uv run scripts/ht-check.py -q --ht $HT; or exit 2

set -l one pixel9fold__app__screenshot__s02__1.png
set -q _flag_file; and set one (path basename $_flag_file)
set -l mask 0
set -l pass --file $one
set -l out scripts/pub/out/screenshot-time.png
if set -q _flag_mask_time
    set mask 1
    set -a pass --mask-time
end

# --- render mode: re-run this script under freeze and stop ------------------
if set -q _flag_png
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self $pass" \
        --execute.timeout 2m \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 104 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    or exit $status
    echo "wrote $out"
    exit 0
end

# --- setup --------------------------------------------------------------------
set -l s corpus/differential/11-screenshots

function die
    echo "error: $argv" >&2
    exit 1
end

for tool in jq exiftool
    type -q $tool; or die "$tool not found on PATH"
end
test -x $HT; or die "ht not executable at $HT (cargo build --release -p halftone-cli --features c2pa, or set -x HT)"

set -l files
for f in $s/pixel9fold__*__screenshot__*.png
    set -a files $f
end
test (count $files) -gt 0; or die "no Fold screenshots in $s (the files are not committed: collect them first)"
test -f $s/$one; or die "missing $s/$one"
contains -- $s/$one $files; or die "$one is not a Fold screenshot (pixel9fold__*__screenshot__*.png)"

# The one fact this frame cannot measure. Quoted from the collection notes, and only
# while they still say it.
grep -q 'System UI demo mode' $s/SOURCE.md
and grep -q 'clock (12:00) and battery (100)' $s/SOURCE.md
or die "$s/SOURCE.md no longer says the status bar was in demo mode at 12:00: fix the frame's first note"
set -l shown 12:00

set -l tmp (mktemp -d)
function cleanup --on-event fish_exit --inherit-variable tmp
    rm -rf $tmp
end

# `ht inspect` exits 2 when any source reports Present (its CI convention), so 2 is
# a normal result here; anything else is a failure.
$HT inspect --batch --json $files >$tmp/ht.json 2>$tmp/ht.err
set -l rc $status
set -l err (head -n 3 $tmp/ht.err | string join ' | ')
contains -- $rc 0 2; or die "ht inspect exited $rc: $err"
jq -e '.inspections | type == "array"' $tmp/ht.json >/dev/null 2>&1; or die "ht inspect did not print a batch document"
test (jq '.inspections | length' $tmp/ht.json) -eq (count $files); or die "ht did not report on every file"

# Never publish a value: stop if any identifier in ht's output is not masked.
set -l open (jq '[.inspections[].evidence[] | select(.source.name == "embedded_identifiers")
                  | .details.identifiers[]? | select(has("value"))] | length' $tmp/ht.json)
test "$open" = 0; or die "$open identifier values are not masked in ht's output: refusing to print"

# The ID's value stays inside this file, which is removed on exit.
exiftool -j -Software -SubSecDateTimeOriginal -DateTimeOriginal -SubSecTimeOriginal \
    -OffsetTimeOriginal -ImageUniqueID $files >$tmp/exif.json 2>$tmp/exif.err
or die "exiftool failed: "(head -n 3 $tmp/exif.err | string join ' | ')
test (jq 'length' $tmp/exif.json) -eq (count $files); or die "exiftool did not report on every file"

# One row per file: what each tool read, with the ID reduced to its length.
jq -n --slurpfile ht $tmp/ht.json --slurpfile ex $tmp/exif.json '
    def base: split("/") | last;
    ($ex[0] | map({key: (.SourceFile | base), value: .}) | from_entries) as $exif
    | [ $ht[0].inspections[]
        | (.asset.path | base) as $name
        | (.evidence | map({key: .source.name, value: .}) | from_entries) as $e
        | $exif[$name] as $x
        # The composite tag when ExifTool builds it, else its three parts.
        | ($x.SubSecDateTimeOriginal
           // (if $x.DateTimeOriginal then
                 ($x.DateTimeOriginal | tostring)
                 + (if $x.SubSecTimeOriginal != null then "." + ($x.SubSecTimeOriginal | tostring) else "" end)
                 + (($x.OffsetTimeOriginal // "") | tostring)
               else null end)) as $raw
        | ($raw // "" | tostring
           # No backslashes: fish halves them inside single quotes.
           | capture("^(?<y>[0-9]{4}):(?<m>[0-9]{2}):(?<d>[0-9]{2}) (?<clock>[0-9]{2}:[0-9]{2}:[0-9]{2})(?:[.](?<frac>[0-9]+))?(?<off>(?:[+-][0-9]{2}:[0-9]{2}|Z))?$")
             // null) as $t
        | { name: $name,
            what: ($name | split("__")[1]),
            sha: (.asset.sha256 // "" | .[0:12]),
            software: ($x.Software // null),
            date: (if $t then "\($t.y)-\($t.m)-\($t.d)" else null end),
            clock: ($t.clock // null),
            frac: ($t.frac // null),
            off: ($t.off // null),
            id_len: (if $x.ImageUniqueID != null then ($x.ImageUniqueID | tostring | length) else null end),
            id_key: (if $x.ImageUniqueID != null then ($x.ImageUniqueID | tostring) else null end),
            writer: ($e.png_writer.details.writer_rule.name // null),
            ht_software: ($e.png_writer.details.exif_software // null),
            ht_ids: [ $e.embedded_identifiers.details.identifiers[]?
                      | select(.field == "exif:ImageUniqueID") | {kind, field, length} ] } ]
    # In collection order (s01, s02, ...), the order of SOURCE.md.
    | sort_by(.name | split("__")[3])
    | { rows: map(del(.id_key)),
        distinct_ids: ([.[].id_key | select(. != null)] | unique | length) }
' >$tmp/rows.json; or die "could not join ht's and exiftool's output"

# The two tools read the same file: they have to agree on what they both report.
set -l bad (jq -r '.rows[] | select((.ht_ids | length) > 0 and .ht_ids[0].length != .id_len) | .name' $tmp/rows.json)
test -z "$bad"; or die "ht and exiftool disagree on the ID's length in: $bad"
set -l bad (jq -r '.rows[] | select(.ht_software != .software) | .name' $tmp/rows.json)
test -z "$bad"; or die "ht and exiftool disagree on the Software tag in: $bad"
set -l bad (jq -r '.rows[] | select(.id_len != null and (.ht_ids | length) == 0) | .name' $tmp/rows.json)
test -z "$bad"; or die "exiftool finds an ImageUniqueID that ht does not list in: $bad"

# --- frame ------------------------------------------------------------------------
set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l Y (set_color yellow)
set -l R (set_color normal)

echo "$D# Android screenshots: the picture and the file, checked "(date -u '+%Y-%m-%d %H:%M')" UTC$R"
echo "$D# "($HT --version 2>&1 | string join ' ')"  ·  ExifTool "(exiftool -ver)"  ·  ID values masked: length only$R"
echo "$D# on screen (11-screenshots/SOURCE.md): status bar in System UI demo mode, clock $R$Y$shown$R$D, battery 100$R"

echo
set -l size (wc -c <$s/$one | string trim)
echo "$H== one screenshot, as the phone saved it$R  $D$one  $size B$R"
jq -r --arg one $one --argjson mask $mask '
    def pad(n): tostring as $s | $s + (if n > ($s | length) then " " * (n - ($s | length)) else "" end);
    def clockof: if $mask == 1 then (.clock | gsub("[0-9]"; "·")) else .clock end;
    def fracof: if .frac == null then "" elif $mask == 1 then "." + (.frac | gsub("[0-9]"; "·")) else "." + .frac end;
    def when: if .date == null then "none" else "\(.date) \(clockof)\(fracof)" + (if .off then " " + .off else "" end) end;
    def masked(n): "[masked, \(n) character" + (if n == 1 then "" else "s" end) + "]";
    .rows[] | select(.name == $one)
    | "exiftool",
      "  " + ("Software" | pad(22)) + (.software // "none"),
      "T " + ("capture time" | pad(22)) + when,
      "  " + ("ImageUniqueID" | pad(22)) + (if .id_len then masked(.id_len) else "none" end),
      "ht",
      "  " + ("png_writer" | pad(22)) + ((.writer // "no rule matched") | split(" (")[0]),
      "  " + ("embedded_identifiers" | pad(22))
           + (if (.ht_ids | length) > 0 then (.ht_ids[0] | "\(.field) (\(.kind))  " + masked(.length)) else "none" end)
' $tmp/rows.json | while read -l line
    switch $line
        case 'T *'
            # The line the post is about.
            set -l kv (string match -r '^T (\s*capture time\s+)(.*)$' -- $line)
            echo "  $kv[2]$Y$kv[3]$R"
        case exiftool ht
            echo "$D$line$R"
        case '*'
            echo $line
    end
end

echo
echo "$H== all "(count $files)" Fold screenshots in 11-screenshots$R"
jq -r --argjson mask $mask --arg shown $shown '
    def pad(n): tostring as $s | $s + (if n > ($s | length) then " " * (n - ($s | length)) else "" end);
    def clockof: if $mask == 1 then (.clock | gsub("[0-9]"; "·")) else .clock end;
    def fracof: if .frac == null then "" elif $mask == 1 then "." + (.frac | gsub("[0-9]"; "·")) else "." + .frac end;
    def when: if .date == null then "none" else "\(.date) \(clockof)\(fracof)" + (if .off then " " + .off else "" end) end;
    def n(f): map(select(f)) | length;
    .distinct_ids as $distinct | .rows as $r | ($r | length) as $all
    | ([$r[] | .what | length] | max) as $w
    | ("captured" | pad($w + 2)) + ("capture time in the file" | pad(33)) + ("unique ID" | pad(16)) + "Software",
      ($r[] | (.what | pad($w + 2)) + (when | pad(33))
              + ((if .id_len then "\(.id_len) characters" else "none" end) | pad(16)) + (.software // "none")),
      "",
      "\($r | n(.clock != null)) of \($all) hold a capture time, \($r | n(.frac != null and (.frac | length) == 3)) to the millisecond, \($r | n(.off != null)) with a UTC offset; \($r | n(.clock != null and (.clock | startswith($shown + ":")))) of \($all) read \($shown)",
      "\($distinct) distinct IDs in \($r | n(.id_len != null)) files; \([$r[] | .software | select(. != null)] | unique | length | if . == 1 then "1 Android build" else "\(.) Android builds" end)",
      "# unsigned EXIF, read as written: what the file says about itself, which anyone could rewrite"
' $tmp/rows.json | while read -l line
    switch $line
        case 'captured*' '#*'
            echo "$D$line$R"
        case '*'
            echo $line
    end
end
