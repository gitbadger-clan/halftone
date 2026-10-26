#!/usr/bin/env fish
# Build corpus/differential/08-multipart: the D-014 copies of the Fold s02 motion
# photo (optional parts removed, clip or still altered, clip cut short), so that
# Halftone's multi-asset fallback (C2PA 2.4 §15.12.4) is compared with Credentio on
# every differential run. The files derive from a private 06 capture and are not
# committed; SOURCE.md and expectations.json are.
#
#   fish scripts/make-multipart-stratum.fish
#
# Byte positions come from scripts/multipart-layout.fish, read from the file's own
# signed part table (CLIP_FLIP, CLIP_CUT and STILL_FLIP overrides apply). Running it
# again writes identical bytes, so the sha256s in expectations.json stay valid.
# Never modifies the 06 file; never reads the clipboard.

function die
    echo "error: $argv" >&2
    exit 1
end

set -l root (git rev-parse --show-toplevel); or exit 1
cd $root
source scripts/multipart-layout.fish

for c in c2patool jq python3 xxd cmp shasum
    command -q $c; or die "$c not on PATH"
end

set -l src (realpath corpus/differential/06-phones/*__motion__*__s02__* 2>/dev/null)
test (count $src) -eq 1; or die "expected one s02 motion file in 06-phones, found "(count $src)
multipart_layout $src; or die "layout checks failed (see above)"

set -l out corpus/differential/08-multipart
mkdir -p $out; or die "cannot create $out"

# <editor>__<operation>__<input device>-<input variant>__<subject>__<n>, as in 07.
set -l stem pixel9fold-motion__s02__1.jpg
set -l f_noclip $out/bytes__remove-clip__$stem
set -l f_still $out/bytes__remove-gainmap-and-clip__$stem
set -l f_flipclip $out/bytes__flip-clip__$stem
set -l f_cut $out/bytes__cut-clip__$stem
set -l f_flipstill $out/bytes__flip-still__$stem

set -l flip 'import sys; p,o=sys.argv[1],int(sys.argv[2]); d=bytearray(open(p,"rb").read()); d[o]^=1; open(p,"wb").write(d)'
head -c $ml_off[3] $src >$f_noclip
head -c $ml_off[2] $src >$f_still
cp -f $src $f_flipclip; and python3 -c $flip $f_flipclip $ml_clip_flip
head -c $ml_clip_cut $src >$f_cut
cp -f $src $f_flipstill; and python3 -c $flip $f_flipstill $ml_still_flip

# Each file must be exactly what its name says.
function bytes_of
    wc -c <$argv[1] | string trim
end
function first_diff
    cmp -l $argv[1] $argv[2] | string trim | string split -n ' '
end
test (bytes_of $f_noclip) -eq $ml_off[3]; or die "remove-clip is not cut at $ml_off[3]"
test (bytes_of $f_still) -eq $ml_off[2]; or die "remove-gainmap-and-clip is not cut at $ml_off[2]"
test (bytes_of $f_cut) -eq $ml_clip_cut; or die "cut-clip is not cut at $ml_clip_cut"
test (cmp -l $src $f_flipclip | count) -eq 1; or die "flip-clip differs in more than one byte"
test (cmp -l $src $f_flipstill | count) -eq 1; or die "flip-still differs in more than one byte"
test (first_diff $src $f_flipclip)[1] -eq (math $ml_clip_flip + 1); or die "flip-clip changed the wrong byte"
test (first_diff $src $f_flipstill)[1] -eq (math $ml_still_flip + 1); or die "flip-still changed the wrong byte"

echo "source:      $src ($ml_size bytes)"
echo "parts:       0 [0, $ml_off[2]) required; 1 [$ml_off[2], $ml_off[3]) optional; 2 [$ml_off[3], $ml_size) optional"
echo "clip flip:   byte $ml_clip_flip (0x$ml_clip_byte)"
echo "clip cut:    at byte $ml_clip_cut"
echo "still flip:  byte $ml_still_flip (0x$ml_still_byte)"
echo
pushd $out >/dev/null
shasum -a 256 bytes__*.jpg
popd >/dev/null
