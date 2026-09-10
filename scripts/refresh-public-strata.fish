#!/usr/bin/env fish
# Rebuild the public differential strata (01 c2pa-rs fixtures, 02 ExifTool images,
# 03 synthetic), re-collect their ground truth, and run the differential test.
#
#   scripts/refresh-public-strata.fish              # pin to upstream HEAD of today
#   scripts/refresh-public-strata.fish --keep-pins  # reuse the shas in SOURCE.txt
#   scripts/refresh-public-strata.fish --no-test    # stop after collecting
#
# Ground truth (expectations.json, cases.json) is what gets committed; the files are
# not. After a run, `git diff corpus/differential` shows exactly what changed in
# the reference tools' view of the corpus, and the two shas printed at the end go
# into .github/workflows/differential.yml.

set -l root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
set -l diff $root/corpus/differential
set -l anchors $root/crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem
set -l keep_pins 0
set -l run_test 1
for a in $argv
    switch $a
        case --keep-pins
            set keep_pins 1
        case --no-test
            set run_test 0
        case '*'
            echo "unknown argument: $a" >&2
            exit 2
    end
end

for tool in git exiftool uv fish
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
test -f $anchors; or begin
    echo "trust anchors not found: $anchors" >&2
    exit 2
end

function pin --argument-names var repo source_txt keep
    if set -q $var; and test -n "$$var"
        echo "$var: from environment"
        return
    end
    if test "$keep" = 1; and test -f $source_txt
        set -gx $var (string replace 'commit: ' '' (grep '^commit: ' $source_txt))
        echo "$var: from $source_txt"
        return
    end
    set -gx $var (git ls-remote $repo HEAD | cut -f1)
    echo "$var: upstream HEAD"
end

pin C2PA_FIXTURES_COMMIT https://github.com/contentauth/c2pa-rs $diff/01-c2pa-rs/SOURCE.txt $keep_pins
pin EXIFTOOL_FIXTURES_COMMIT https://github.com/exiftool/exiftool $diff/02-exiftool/SOURCE.txt $keep_pins

echo
echo "== 01 c2pa-rs fixtures @ $C2PA_FIXTURES_COMMIT"
fish $root/scripts/fetch-c2pa-fixtures.fish --refresh; or exit 1
echo
echo "== 02 ExifTool images @ $EXIFTOOL_FIXTURES_COMMIT"
fish $root/scripts/fetch-exiftool-fixtures.fish --refresh; or exit 1
echo
echo "== 03 synthetic"
fish $root/scripts/gen-synthetic-dst.fish --refresh; or exit 1

echo
echo "== ground truth"
uv run $root/scripts/differential.py $diff/01-c2pa-rs --trust-anchors $anchors | tail -1; or exit 1
uv run $root/scripts/differential.py $diff/02-exiftool | tail -1; or exit 1
uv run $root/scripts/differential.py $diff/03-synthetic | tail -1; or exit 1

echo
echo "== pins for .github/workflows/differential.yml"
echo "  C2PA_FIXTURES_COMMIT: $C2PA_FIXTURES_COMMIT"
echo "  EXIFTOOL_FIXTURES_COMMIT: $EXIFTOOL_FIXTURES_COMMIT"
echo
echo "== changed ground truth"
git -C $root status --short -- corpus/differential

if test $run_test = 1
    echo
    echo "== differential test"
    cd $root
    cargo test -p halftone-cli --features c2pa --test differential -- --ignored --nocapture
end
