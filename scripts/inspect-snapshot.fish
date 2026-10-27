# Snapshot `ht inspect --json` for every file in the differential corpus,
# normalized so two snapshots differ only where behaviour differs: keys sorted,
# `created_at` and every `duration_ms` removed. Take one before a refactor that
# must not change behaviour, one after, and diff them.
#
#   fish scripts/inspect-snapshot.fish ../snapshots/before
#   # ...refactor...
#   fish scripts/inspect-snapshot.fish ../snapshots/after
#   diff -r ../snapshots/before ../snapshots/after; and echo "no behaviour change"
#
# Uses a c2pa-enabled debug build of ht. Shelf-life rationales count days to
# certificate expiry, so take both snapshots on the same day.

function die
    echo $argv >&2
    exit 1
end

test (count $argv) -eq 1; or die "usage: fish scripts/inspect-snapshot.fish OUTDIR"
set out $argv[1]
test -e $out; and die "$out already exists; pick a new directory"
command -q jq; or die "jq not found"

cargo build -q -p halftone-cli --features halftone-cli/c2pa; or die "build failed"
set ht target/debug/ht
mkdir -p $out

set n 0
set failed 0
for f in corpus/differential/*/*
    test -f $f; or continue
    switch (basename $f)
        case '*.json' '*.md' '*.txt' '*.csv' '*.fish' '*.py'
            continue
    end
    set rel (string replace 'corpus/differential/' '' $f)
    set dest $out/(string replace -a / __ $rel).json
    # jq succeeds on empty input and ht's exit status may carry findings, so an
    # empty result is what marks a failed inspection.
    $ht inspect $f --json 2>/dev/null \
        | jq -S 'del(.created_at) | walk(if type == "object" then del(.duration_ms) else . end)' >$dest
    if not test -s $dest
        echo "  failed: $rel" >&2
        set failed (math $failed + 1)
    end
    set n (math $n + 1)
end

echo "$n files -> $out"
test $failed -eq 0; or die "$failed files failed; the snapshot is incomplete"
