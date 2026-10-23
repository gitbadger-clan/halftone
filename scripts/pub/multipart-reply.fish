#!/usr/bin/env fish
# Frame for the reply to the multipart-hashing comment on the motion-photo post
# (2026-10-22): the same Pixel 9 Pro Fold motion photo as six copies (untouched,
# optional parts removed, clip altered or cut short, still altered) through ht,
# c2patool and Credentio. The part table is read from the file's own signed
# c2pa.hash.multi-asset assertion; every cut and flip is checked against it.
#
#   fish scripts/pub/multipart-reply.fish          # print the frame
#   fish scripts/pub/multipart-reply.fish --png    # render scripts/pub/out/multipart-reply.png
#   fish scripts/pub/multipart-reply.fish --skip partial-clip --png   # leave a row out
#
# Every value is computed at run time from the tools' output; nothing is typed in.
# Copies go to a temp dir outside the repo and are deleted on exit. The raw archive
# for the same run lives in scripts/multipart-evidence.fish.
#
# Override binaries with exported HT / CREDENTIO (set -x, so freeze's re-run sees them).
# Requires: ht, c2patool, Credentio's c2pa_validate, jq, exiftool, python3, xxd, shasum;
#           freeze for --png (brew install charmbracelet/tap/freeze)

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l out scripts/pub/out/multipart-reply.png

# --- render mode: re-run this script under freeze and stop ------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    set -l pass (string match -v -- --png $argv)
    freeze --execute "fish $self $pass" \
        --execute.timeout 2m \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 130 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    or exit $status
    echo "wrote $out"
    exit 0
end

# --- setup --------------------------------------------------------------------
set -q CREDENTIO; or set -g CREDENTIO ~/oss-bins/c2pa_validate
set -q HT; or set -g HT ./target/release/ht
set -g trust crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem
set -g tsa crates/halftone-c2pa/trust/C2PA-TSA-TRUST-LIST.pem

set -l skip
set -l a $argv
while set -q a[1]
    switch $a[1]
        case --skip
            set -a skip $a[2]
            set -e a[1]
        case '*'
            echo "unknown argument: $a[1]" >&2
            exit 2
    end
    set -e a[1]
end

# Layout and edit positions are derived from the file (scripts/multipart-layout.fish).
# Override the edit positions with exported CLIP_FLIP, CLIP_CUT, STILL_FLIP.
source scripts/multipart-layout.fish

function die
    echo "error: $argv" >&2
    exit 1
end

for tool in c2patool jq exiftool python3 xxd shasum
    type -q $tool; or die "$tool not found on PATH"
end
test -x $HT; or die "ht not executable at $HT (cargo build --release -p halftone-cli --features c2pa, or set -x HT)"
test -x $CREDENTIO; or die "Credentio not executable at $CREDENTIO"
test -f $trust; and test -f $tsa; or die "trust lists missing under crates/halftone-c2pa/trust"

set -l src (realpath corpus/differential/06-phones/*__motion__*__s02__* 2>/dev/null)
test (count $src) -eq 1; or die "expected one s02 motion file, found "(count $src)

set -g tmp (mktemp -d)
function cleanup --on-event fish_exit
    rm -rf $tmp
end

# c2patool as in the original post: no anchors, network off, no hidden settings (D-008).
set -g xdg $tmp/xdg
mkdir -p $xdg
set -g settings $tmp/c2patool.toml
printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\n' >$settings

function c2patool_clean
    env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS XDG_CONFIG_HOME=$xdg \
        c2patool --settings $settings $argv 2>/dev/null
end

function ht_state
    $HT inspect --batch --json --only manifest --trust-list vendored $argv[1] 2>/dev/null | jq -r \
        '.inspections[0].evidence[] | select(.source.name=="c2pa") | .details.validation_state // "-"'
end

function c2patool_state
    c2patool_clean $argv[1] | jq -r '.validation_state // "-"'
end

# Credentio's dataHash / multiAssetHash results as "code assertion-label" lines.
function credentio_codes
    set -l o $tmp/(basename $argv[1]).txtpb
    $CREDENTIO --asset=$argv[1] --claim_signer_trust=$trust --tsa_trust=$tsa \
        --output_format=txtpb --output_file=$o >/dev/null 2>&1
    test -f $o; or return
    awk '
        /^[ \t]*(failures|successes|informationals)[ \t]*\{/ { kind = $1 }
        /^[ \t]*code:/ { c = $2; gsub(/"/, "", c) }
        /^[ \t]*url:/ {
            u = $2; gsub(/"/, "", u); n = split(u, a, "/")
            if (kind != "informationals" && c ~ /^assertion\.(dataHash|multiAssetHash)\./) print c, a[n]
        }' $o
end

# Result for one assertion label: a failure wins over a match; no code at all -> "—".
function result_for
    set -l label $argv[1]
    set -l hits
    for l in $argv[2..-1]
        set -l p (string split ' ' -- $l)
        test "$p[2]" = "$label"; and set -a hits $p[1]
    end
    set -l bad
    for h in $hits
        switch $h
            case '*.mismatch' '*.missingPart' '*.malformed'
                set -a bad (string replace -r '.*\.' '' -- $h)
        end
    end
    if set -q bad[1]
        string join + -- $bad
        return
    end
    for h in $hits
        string match -q '*.match' -- $h; and begin
            echo match
            return
        end
    end
    echo —
end

# Pad first, colour after, so columns stay aligned.
function paint
    switch $argv[1]
        case Valid Trusted match
            set -f c brgreen
        case Invalid mismatch missingPart malformed '*+*'
            set -f c brred
        case — -
            set -f c brblack
        case '*'
            set -f c yellow
    end
    echo -n (set_color $c)(string pad -r -w $argv[2] -- $argv[1])(set_color normal)
end

# --- the signed part table, checked against the bytes ------------------------------
multipart_layout $src; or die "layout checks failed (see above)"
set -l off $ml_off
set -l len $ml_len
set -l opt $ml_opt
set -l lab $ml_lab
set -l size $ml_size
set -l clip_flip $ml_clip_flip
set -l clip_cut $ml_clip_cut
set -l still_flip $ml_still_flip

# --- the six copies -------------------------------------------------------------
set -l flip 'import sys; p,o=sys.argv[1],int(sys.argv[2]); d=bytearray(open(p,"rb").read()); d[o]^=1; open(p,"wb").write(d)'
cp $src $tmp/untouched.jpg
head -c $off[3] $src >$tmp/no-clip.jpg
head -c $off[2] $src >$tmp/still-only.jpg
cp $src $tmp/flip-clip.jpg
python3 -c $flip $tmp/flip-clip.jpg $clip_flip
head -c $clip_cut $src >$tmp/partial-clip.jpg
cp $src $tmp/flip-still.jpg
python3 -c $flip $tmp/flip-still.jpg $still_flip

# Each copy must be exactly what its row label says.
function bytes_of
    wc -c <$argv[1] | string trim
end
function diffs
    cmp -l $argv[1] $argv[2] | count
end
cmp -s $src $tmp/untouched.jpg; or die "untouched copy differs from the source"
test (bytes_of $tmp/no-clip.jpg) -eq $off[3]; or die "no-clip is not cut at $off[3]"
test (bytes_of $tmp/still-only.jpg) -eq $off[2]; or die "still-only is not cut at $off[2]"
test (bytes_of $tmp/partial-clip.jpg) -eq $clip_cut; or die "partial-clip is not cut at $clip_cut"
test (diffs $src $tmp/flip-clip.jpg) -eq 1; or die "flip-clip differs in more than one byte"
test (diffs $src $tmp/flip-still.jpg) -eq 1; or die "flip-still differs in more than one byte"
test (cmp -l $src $tmp/flip-clip.jpg | string trim | string split -n ' ')[1] -eq (math $clip_flip + 1); or die "flip-clip changed the wrong byte"
test (cmp -l $src $tmp/flip-still.jpg | string trim | string split -n ' ')[1] -eq (math $still_flip + 1); or die "flip-still changed the wrong byte"

# What is physically left of part i in file f: present / absent / cut short.
function geometry --argument-names f o l
    set -l sz (wc -c <$f | string trim)
    if test $sz -le $o
        echo absent
    else if test $sz -lt (math $o + $l)
        echo "cut short"
    else
        echo —
    end
end

set -l names untouched no-clip still-only flip-clip partial-clip flip-still
set -l changes nothing "part 2 removed" "parts 1+2 removed" "1 bit flipped, part 2" "part 2 cut short" "1 bit flipped, part 0"

# --- frame ------------------------------------------------------------------------
set -l H (set_color --bold brcyan)
set -l D (set_color brblack)
set -l R (set_color normal)

set -l device (exiftool -s3 -Make -Model $src | string join ' ')
set -l sha (shasum -a 256 $src | string sub -l 64)
set -l cred_ver (string replace -r '^c2pa_validate-' '' (basename (realpath $CREDENTIO)))

echo "$D# Removing vs altering parts of a motion photo, checked "(date -u '+%Y-%m-%d %H:%M')" UTC$R"
echo "device: $device"
echo "file:   $size bytes, sha256 "(string sub -l 8 $sha)"…"(string sub -s -8 $sha)
echo "$D# "($HT --version 2>&1 | string join ' ')"  ·  "(c2patool --version 2>&1 | string join ' ')"  ·  credentio $cred_ver$R"

echo
echo "$H== parts, as signed in the file's c2pa.hash.multi-asset assertion$R"
for i in 1 2 3
    set -l kind required
    test $opt[$i] = true; and set kind optional
    printf 'part %d  bytes %9s – %-9s %9s bytes  %-9s %s\n' (math $i - 1) $off[$i] (math $off[$i] + $len[$i] - 1) $len[$i] $kind $lab[$i]
end

echo
set -l shown
for n in $names
    contains -- $n $skip; or set -a shown $n
end
echo "$H== "(count $shown)" copies of that file, two libraries: c2pa-rs (ht, c2patool) and Credentio$R"
printf "$D%-13s %-22s %-9s %-10s %-10s %-10s %-10s %-22s %s$R\n" \
    file change ht c2patool "part 0" "part 1" "part 2" "credentio result" "decided by"
for i in (seq (count $names))
    contains -- $names[$i] $skip; and continue
    set -l f $tmp/$names[$i].jpg
    set -l codes (credentio_codes $f)
    set -l pr
    for j in 1 2 3
        set -l v (result_for $lab[$j] $codes)
        test "$v" = —; and set v (geometry $f $off[$j] $len[$j])
        set -a pr $v
    end
    set -l multi (result_for c2pa.hash.multi-asset $codes)
    set -l res $multi
    set -l via parts
    if test $multi = —
        set res (result_for c2pa.hash.data $codes)
        set via "whole file"
    end
    printf '%-13s %-22s ' $names[$i] $changes[$i]
    paint (ht_state $f) 10
    paint (c2patool_state $f) 11
    paint $pr[1] 11
    paint $pr[2] 11
    paint $pr[3] 11
    paint $res 23
    echo "$D$via$R"
end

echo
if contains -- partial-clip $shown
    echo "$D# part columns: Credentio's hash result per part; with no result: absent / cut short (file length), — = not needed$R"
else
    echo "$D# part columns: Credentio's hash result per part; absent = not in the file; — = not needed$R"
end
set -l where "1-bit flips at byte $clip_flip (0x$ml_clip_byte, middle of part 2) and byte $still_flip (0x$ml_still_byte, part 0 scan data)"
contains -- partial-clip $shown; and set where "$where; part 2 cut at byte $clip_cut"
echo "$D# $where$R"
if contains -- partial-clip $shown
    echo "$D# C2PA 2.4 §15.12.4: an optional part may be missing;$R"
    echo "$D#   a part that is present or cut short must match (else multiAssetHash.mismatch)$R"
else
    echo "$D# C2PA 2.4 §15.12.4: an optional part may be missing; a part that is present must match$R"
end
