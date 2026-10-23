#!/usr/bin/env fish
# Multipart-hash evaluation of the Pixel 9 Pro Fold motion photo
# (follow-up to the 2026-10-22 post and the Credentio reply).
#
#   fish scripts/multipart-evidence.fish
#
# Override the binaries with CREDENTIO / HT, the run location with OUT_ROOT.
# CREDENTIO is expected to be a symlink to a pinned copy named
# c2pa_validate-<commit>; the commit is read from that name.
#
# Never reads the clipboard. Never modifies the corpus file; every test file is a copy.

function die
    echo "error: $argv" >&2
    exit 1
end

set -l root (git rev-parse --show-toplevel); or exit 1
cd $root

# ---- configuration ---------------------------------------------------------

set -q CREDENTIO; or set CREDENTIO ~/oss-bins/c2pa_validate
set -q HT; or set HT ./target/release/ht
set -q OUT_ROOT; or set OUT_ROOT ../credentio-runs

set cred $CREDENTIO
set trust crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem
set tsa crates/halftone-c2pa/trust/C2PA-TSA-TRUST-LIST.pem
set eku crates/halftone-c2pa/trust/C2PA-EKU-CONFIG.cfg

# Layout and edit positions are derived from the file (scripts/multipart-layout.fish).
# Override the edit positions with exported CLIP_FLIP, CLIP_CUT, STILL_FLIP.
source scripts/multipart-layout.fish

# ---- preflight ---------------------------------------------------------------

test -d crates/halftone-c2pa; or die "not the Halftone repo: $root"
for p in $trust $tsa $eku
    test -f $p; or die "missing $p"
end
test -x $cred; or die "Credentio validator not found at $cred (set CREDENTIO)"
test -x $HT; or die "ht not found at $HT (cargo build --release -p halftone-cli --features c2pa, or set HT)"
for c in c2patool jq python3 xxd cmp shasum uv
    command -q $c; or die "$c not on PATH"
end

set src (realpath corpus/differential/06-phones/*__motion__*__s02__* 2>/dev/null)
test (count $src) -eq 1; or die "expected exactly one s02 motion file, found "(count $src)

mkdir -p $OUT_ROOT; or die "cannot create $OUT_ROOT"
set t (realpath $OUT_ROOT)/(date +%Y-%m-%dT%H%M)
set in $t/inputs
set raw $t/raw
mkdir -p $in $raw; or die "cannot create $t"
echo "run folder: $t"

# ---- 1. layout, read from the file and checked against its bytes ---------------

multipart_layout $src; or die "layout checks failed (see above)"
set part1_start $ml_off[2]
set part2_start $ml_off[3]
set clip_flip $ml_clip_flip
set clip_cut $ml_clip_cut
set still_flip $ml_still_flip

echo
echo "-- layout (from the signed part table, checked against the bytes) --"
for i in 1 2 3
    printf 'part %d  %9s + %9s bytes  optional=%-5s  %s\n' (math $i - 1) $ml_off[$i] $ml_len[$i] $ml_opt[$i] $ml_lab[$i]
end
echo "file size:   $ml_size"
echo "clip flip:   byte $clip_flip (0x$ml_clip_byte), middle of part 2 unless CLIP_FLIP is set"
echo "clip cut:    at byte $clip_cut, middle of part 2 unless CLIP_CUT is set"
echo "still flip:  byte $still_flip (0x$ml_still_byte), middle of part 0's scan data unless STILL_FLIP is set"

# ---- 2. build the six inputs ------------------------------------------------

set flip 'import sys; p,o=sys.argv[1],int(sys.argv[2]); d=bytearray(open(p,"rb").read()); d[o]^=1; open(p,"wb").write(d)'

cp $src $in/untouched.jpg
cp $src $in/flip-clip.jpg
python3 -c $flip $in/flip-clip.jpg $clip_flip
cp $src $in/flip-still.jpg
python3 -c $flip $in/flip-still.jpg $still_flip
head -c $part2_start $src >$in/no-clip.jpg
head -c $part1_start $src >$in/still-only.jpg
head -c $clip_cut $src >$in/partial-clip.jpg

cmp -s $src $in/untouched.jpg; or die "untouched copy differs from the source"
test (cmp -l $in/untouched.jpg $in/flip-clip.jpg  | count) -eq 1; or die "flip-clip differs in more than one byte"
test (cmp -l $in/untouched.jpg $in/flip-still.jpg | count) -eq 1; or die "flip-still differs in more than one byte"
test (cmp -l $src $in/flip-clip.jpg  | string trim | string split -n ' ')[1] -eq (math $clip_flip + 1); or die "flip-clip changed the wrong byte"
test (cmp -l $src $in/flip-still.jpg | string trim | string split -n ' ')[1] -eq (math $still_flip + 1); or die "flip-still changed the wrong byte"
test (wc -c < $in/no-clip.jpg    | string trim) -eq $part2_start; or die "no-clip has the wrong size"
test (wc -c < $in/still-only.jpg | string trim) -eq $part1_start; or die "still-only has the wrong size"
test (wc -c < $in/partial-clip.jpg | string trim) -eq $clip_cut; or die "partial-clip has the wrong size"

pushd $in
shasum -a 256 *.jpg >$t/SHA256SUMS
popd
echo
echo "-- inputs --"
cat $t/SHA256SUMS

# ---- 3. versions -------------------------------------------------------------

begin
    echo "date:       "(date -u +%FT%TZ)
    echo "source:     $src"
    echo "ht:         "($HT --version 2>&1 | string join ' ')" ($HT, halftone "(git rev-parse --short HEAD)")"
    echo "c2patool:   "(c2patool --version 2>&1 | string join ' ')
    set -l cred_real (realpath $cred)
    echo "credentio:  "(string replace -r '^c2pa_validate-' '' (basename $cred_real))" ($cred_real)"
    echo "trust:      $trust"
    echo "tsa:        $tsa"
    echo "eku:        $eku"
    echo "edits:      clip flip $clip_flip (0x$ml_clip_byte), clip cut $clip_cut, still flip $still_flip (0x$ml_still_byte)"
end >$t/VERSIONS.txt

# ---- 4. run the tools --------------------------------------------------------

set names untouched no-clip still-only flip-clip partial-clip flip-still

# c2patool exactly as in the original post: no trust anchors, network off, and an
# empty config dir so no hidden settings file adds anchors (DIFFERENTIAL.md D-008).
set xdg (mktemp -d)
printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\n' >$t/c2patool-settings.toml

for n in $names
    set x $in/$n.jpg

    $HT inspect --json --only manifest --trust-list vendored $x >$raw/$n.ht.json 2>$raw/$n.ht.err

    env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS XDG_CONFIG_HOME=$xdg \
        c2patool --settings $t/c2patool-settings.toml $x >$raw/$n.c2patool.json 2>$raw/$n.c2patool.err

    for fmt in txtpb crjson
        $cred --asset=$x --claim_signer_trust=$trust --tsa_trust=$tsa \
            --output_format=$fmt --output_file=$raw/$n.credentio.$fmt 2>$raw/$n.credentio.$fmt.err
    end
end

# c2patool with the same anchors and EKU config as ht, through the vetted collector.
uv run scripts/differential.py $in --out $raw/c2patool-trusted.expectations.json \
    --trust-anchors $trust --tsa-anchors $tsa --trust-config $eku >$raw/collector.log 2>&1
or echo "warning: collector failed, see $raw/collector.log"

# ---- 5. summary (an eyeballing aid; the table comes from the raw files) -------

set pat '(assertion|claimSignature|signingCredential|timeStamp)\.[A-Za-z]+(\.[A-Za-z]+)*'
echo
echo "-- status codes per file and tool --"
for n in $names
    echo "== $n"
    for tool in ht c2patool credentio.txtpb
        set codes (cat $raw/$n.$tool* 2>/dev/null | grep -oE $pat | sort -u)
        printf '  %-16s %s\n' $tool (string join ' ' $codes)
    end
end

printf '%s\n' \
    "# Multipart-hash evaluation, "(date +%F) \
    "" \
    "See VERSIONS.txt and SHA256SUMS. Fill every cell from raw/, not from the summary." \
    "" \
    "| File | ht | c2patool | Credentio (overall + per-part codes) |" \
    "|---|---|---|---|" \
    "| untouched | | | |" \
    "| flip-clip (1 bit in optional MP4) | | | |" \
    "| no-clip (MP4 removed) | | | |" \
    "| still-only (gain map + MP4 removed) | | | |" \
    "| partial-clip (MP4 cut short at byte $clip_cut) | | | |" \
    "| flip-still (control, 1 bit in required part) | | | |" >$t/RESULTS.md

echo
echo "done. raw outputs in $raw, table skeleton in $t/RESULTS.md"
