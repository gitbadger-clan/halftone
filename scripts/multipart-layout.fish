# Shared by scripts/multipart-evidence.fish and scripts/pub/multipart-reply.fish.
# Source it, then call:  multipart_layout FILE; or die "..."
#
# Derives everything from FILE itself; nothing about the layout is typed in:
#   ml_size          file length in bytes (wc -c)
#   ml_off ml_len    per-part byte offset and length   } read from the file's signed
#   ml_opt ml_lab    required/optional, hash assertion } c2pa.hash.multi-asset assertion
#   ml_clip_flip     byte flipped in part 2 (default: middle of part 2)
#   ml_clip_cut      length to cut the file to, inside part 2 (default: middle of part 2)
#   ml_still_flip    byte flipped in part 0 (default: middle of its JPEG scan data)
#   ml_clip_byte     original value (hex) of the byte at ml_clip_flip
#   ml_still_byte    original value (hex) of the byte at ml_still_flip
#
# Override the edit positions with exported CLIP_FLIP, CLIP_CUT, STILL_FLIP.
# The part table is the signer's claim, so it is checked against the bytes:
# parts tile the file, part 0 ends a JPEG, part 1 starts a JPEG, part 2 starts an MP4.
# Expects the Fold motion-photo shape: required JPEG, optional JPEG, optional MP4.
# On any failed check: prints the reason and returns 1.

function __ml_fail
    echo "layout: $argv" >&2
    return 1
end

function multipart_layout --argument-names src
    test -f "$src"; or __ml_fail "no such file: $src"; or return 1

    set -g ml_size (wc -c <$src | string trim)

    # --- the signed part table ---------------------------------------------------
    set -l parts (env -u C2PATOOL_SETTINGS -u C2PATOOL_TRUST_ANCHORS c2patool $src --detailed 2>/dev/null | jq -r '
        first(.. | objects | select(has("c2pa.hash.multi-asset")) | ."c2pa.hash.multi-asset"
              | select(type == "object" and has("parts")))
        | .parts[]
        | "\(.location.byteOffset) \(.location.length) \(.optional // false) \(.hashAssertion.url | split("/") | last)"')
    test (count $parts) -eq 3
    or __ml_fail "expected 3 parts in c2pa.hash.multi-asset, found "(count $parts); or return 1

    set -g ml_off
    set -g ml_len
    set -g ml_opt
    set -g ml_lab
    for p in $parts
        set -l f (string split ' ' -- $p)
        set -a ml_off $f[1]
        set -a ml_len $f[2]
        set -a ml_opt $f[3]
        set -a ml_lab $f[4]
    end

    # --- the claim against the bytes ----------------------------------------------
    test $ml_off[1] -eq 0
    and test (math $ml_off[1] + $ml_len[1]) -eq $ml_off[2]
    and test (math $ml_off[2] + $ml_len[2]) -eq $ml_off[3]
    and test (math $ml_off[3] + $ml_len[3]) -eq $ml_size
    or __ml_fail "parts do not tile the file (0 .. $ml_size)"; or return 1

    test $ml_opt[1] = false; and test $ml_opt[2] = true; and test $ml_opt[3] = true
    or __ml_fail "expected part 0 required, parts 1 and 2 optional (got $ml_opt)"; or return 1

    test (xxd -p -s (math $ml_off[2] - 2) -l 2 $src) = ffd9
    or __ml_fail "byte "(math $ml_off[2] - 2)": part 0 does not end in ffd9 (JPEG EOI)"; or return 1
    string match -q 'ffd8ff*' -- (xxd -p -s $ml_off[2] -l 3 $src)
    or __ml_fail "byte $ml_off[2]: part 1 does not start a JPEG (ffd8ff)"; or return 1
    test (xxd -p -s (math $ml_off[3] + 4) -l 4 $src) = 66747970
    or __ml_fail "byte "(math $ml_off[3] + 4)": part 2 does not start an MP4 (ftyp)"; or return 1

    # --- edit positions ---------------------------------------------------------------
    set -l mid3 (math --scale 0 "$ml_off[3] + floor($ml_len[3] / 2)")
    set -q CLIP_FLIP; and set -g ml_clip_flip $CLIP_FLIP; or set -g ml_clip_flip $mid3
    set -q CLIP_CUT; and set -g ml_clip_cut $CLIP_CUT; or set -g ml_clip_cut $mid3

    # Middle of part 0's entropy-coded data: after the last SOS header, before EOI,
    # moved forward off any 0xFF so the flip cannot touch a marker. The scan data
    # cannot contain ffda (0xFF is always stuffed), so the last ffda is a real SOS.
    if set -q STILL_FLIP
        set -g ml_still_flip $STILL_FLIP
    else
        set -g ml_still_flip (python3 -c '
import sys
d = open(sys.argv[1], "rb").read()[: int(sys.argv[2])]
sos = d.rfind(b"\xff\xda")
if sos < 0:
    sys.exit("no SOS marker in part 0")
start = sos + 2 + int.from_bytes(d[sos + 2 : sos + 4], "big")
stop = len(d) - 2
if stop - start < 64:
    sys.exit("scan data in part 0 too short")
m = (start + stop) // 2
while m < stop - 1 and (d[m] == 0xFF or d[m - 1] == 0xFF):
    m += 1
print(m)' $src $ml_len[1])
        or __ml_fail "could not locate scan data in part 0"; or return 1
    end

    set -l end3 (math $ml_off[3] + $ml_len[3])
    test $ml_clip_flip -ge $ml_off[3]; and test $ml_clip_flip -lt $end3
    or __ml_fail "clip flip $ml_clip_flip is outside part 2"; or return 1
    test $ml_clip_cut -gt $ml_off[3]; and test $ml_clip_cut -lt $end3
    or __ml_fail "clip cut $ml_clip_cut is outside part 2"; or return 1
    test $ml_still_flip -ge 0; and test $ml_still_flip -lt $ml_len[1]
    or __ml_fail "still flip $ml_still_flip is outside part 0"; or return 1
    if string match -qr '^ff|^..ff' -- (xxd -p -s (math $ml_still_flip - 1) -l 2 $src)
        __ml_fail "still flip $ml_still_flip is next to a 0xFF byte (JPEG marker risk)"
        return 1
    end

    set -g ml_clip_byte (xxd -p -s $ml_clip_flip -l 1 $src)
    set -g ml_still_byte (xxd -p -s $ml_still_flip -l 1 $src)
    return 0
end
