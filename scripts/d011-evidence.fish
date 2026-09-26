#!/usr/bin/env fish
# D-011 evidence: one file through c2patool 0.27 (c2pa 0.90) and 0.28 (c2pa 0.91),
# each with and without the vendored EKU trust_config, then through Halftone.
# Same anchors everywhere; 0.27 gets them as the legacy [trust] string, 0.28 as
# typed [[trust.anchors]] (0.27 does not read the typed form).
#
#   scripts/d011-evidence.fish [file]      # default: a Bing Image Creator download
#
# Override the binaries with C2PATOOL_027 / C2PATOOL_028 / HT.

set -l root (git rev-parse --show-toplevel); or exit 1
cd $root

set -l f corpus/differential/04-generators/bing-image__mai-image-2.5-flash__web-download__p1__1.jpg
test (count $argv) -gt 0; and set f $argv[1]
test -f $f; or begin
    echo "no such file: $f" >&2
    exit 1
end

set -q C2PATOOL_027; or set -l C2PATOOL_027 /opt/homebrew/bin/c2patool
set -q C2PATOOL_028; or set -l C2PATOOL_028 ~/.local/c2patool-0.28/bin/c2patool
set -q HT; or set -l HT ./target/debug/ht

set -l t crates/halftone-c2pa/trust
set -l pem (cat $t/C2PA-TRUST-LIST.pem | string collect)
set -l tsa (cat $t/C2PA-TSA-TRUST-LIST.pem | string collect)
set -l eku (cat $t/C2PA-EKU-CONFIG.cfg | string collect)
set -l verify '[verify]
remote_manifest_fetch = false
ocsp_fetch = false
verify_trust = true
'
set -l d (mktemp -d)

# 0.27: legacy string anchors (manifest + TSA in one bundle, as Halftone did on 0.90)
printf '%s\n[trust]\ntrust_anchors = """\n%s\n%s"""\n' $verify $pem $tsa >$d/027.toml
printf '%s\n[trust]\ntrust_config = """\n%s"""\ntrust_anchors = """\n%s\n%s"""\n' $verify $eku $pem $tsa >$d/027-eku.toml
# 0.28: typed anchors
set -l typed (printf '\n[[trust.anchors]]\ntrust_kind = "manifest"\ntrust_anchors = """\n%s"""\n\n[[trust.anchors]]\ntrust_kind = "tsa"\ntrust_anchors = """\n%s"""\n' $pem $tsa | string collect)
printf '%s%s' $verify $typed >$d/028.toml
printf '%s\n[trust]\ntrust_config = """\n%s"""\n%s' $verify $eku $typed >$d/028-eku.toml

set -l q '[.validation_state // "-", ([.validation_status[]?.code] | unique | join(", "))] | @tsv'
printf '%-24s %-9s %s\n' tool state codes
for run in "0.27 (c2pa 0.90)|$C2PATOOL_027|027" "0.27 + eku|$C2PATOOL_027|027-eku" \
    "0.28 (c2pa 0.91)|$C2PATOOL_028|028" "0.28 + eku|$C2PATOOL_028|028-eku"
    set -l p (string split '|' $run)
    if not test -x $p[2]
        printf '%-24s %s\n' $p[1] "missing: $p[2]"
        continue
    end
    set -l v (command $p[2] --version)
    set -l out (command $p[2] --settings $d/$p[3].toml $f 2>/dev/null | jq -r $q)
    test -n "$out"; or set out "error	c2patool gave no JSON"
    set -l cols (string split \t $out)
    printf '%-24s %-9s %s   [%s]\n' $p[1] $cols[1] $cols[2] $v
end

if test -x $HT
    set -l h ($HT inspect --json --only manifest $f | jq -r '.evidence[] | select(.source.name == "c2pa")
        | [.details.validation_state // "-", ([.details.validation_status[]?.code] | unique | join(", ")),
           .status, (.details.trust.eku_config.oids | length | tostring), .rationale] | @tsv')
    set -l c (string split \t $h)
    printf '%-24s %-9s %s\n' "halftone (typed + eku)" $c[1] $c[2]
    printf '\nhalftone status: %s · EKU OIDs in policy: %s\nrationale: %s\n' $c[3] $c[4] $c[5]
else
    printf '%-24s %s\n' halftone "missing: $HT (cargo build -p halftone-cli --features c2pa)"
end

rm -rf $d
