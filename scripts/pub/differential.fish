#!/usr/bin/env fish
# Differential evidence for posts: commit, logged disagreement classes, and the
# test's per-stratum lines, all from one run.
#
#   fish scripts/pub/differential.fish            # run, print the block, save the full record
#   fish scripts/pub/differential.fish --png      # also render the block with freeze
#   fish scripts/pub/differential.fish --png --out x.png
#
# One test run feeds everything, so the numbers in the text and the image cannot drift:
#   scripts/pub/out/differential-<date>-<commit>.txt   header, class headings, full cargo output
#   scripts/pub/out/differential-<date>-<commit>.png   the block you quote (with --png)
# A dirty tree is shown as such in both; the PNG is refused if the test failed.

set -l root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
cd $root
set -l png 0
set -l out
while set -q argv[1]
    switch $argv[1]
        case --png
            set png 1
        case --out
            set out $argv[2]
            set -e argv[1]
        case '*'
            echo "unknown argument: $argv[1]" >&2
            exit 2
    end
    set -e argv[1]
end
if test $png = 1; and not type -q freeze
    echo "freeze not found on PATH (brew install charmbracelet/tap/freeze)" >&2
    exit 2
end

set -l day (date -u +%F)
set -l commit (git rev-parse --short HEAD)
set -l dirty ""
test -n "$(git status --porcelain -- crates Cargo.toml Cargo.lock DIFFERENTIAL.md corpus/differential)"
and set dirty " (dirty: uncommitted changes in code, log or corpus)"
set -l base scripts/pub/out/differential-$day-$commit
test -n "$out"; or set out $base.png
mkdir -p scripts/pub/out

# 1. the run
set -l testcmd cargo test -p halftone-cli --features c2pa --test differential -- --nocapture --ignored
set -l raw (mktemp -t differential.XXXXXX)
echo "running: $testcmd" >&2
$testcmd >$raw 2>&1
set -l rc $status
set -l classes (grep -c '^### D-' DIFFERENTIAL.md)

# 2. the full record
begin
    echo "# Halftone differential"
    echo "date:    "(date -u +%Y-%m-%dT%H:%M:%SZ)
    echo "commit:  $commit$dirty"
    echo "c2patool on PATH: "(c2patool --version 2>/dev/null; or echo none)
    echo "logged classes (### D- headings in DIFFERENTIAL.md): $classes"
    string match -r '^### D-\d+.*' <DIFFERENTIAL.md | string replace -r '^' '  '
    echo
    echo "\$ $testcmd"
    cat $raw
    echo "exit: $rc"
end >$base.txt

# 3. the block you quote: prompt lines are the real commands, output is from the run above
set -l dim (printf '\e[90m')
set -l acc (printf '\e[36m')
set -l bad (printf '\e[31m')
set -l rst (printf '\e[0m')
set -l dirty_mark ""
test -n "$dirty"; and set dirty_mark "$bad$dirty$rst" # a variable, not (…): an empty substitution would drop the whole line
set -l block (mktemp -t differential-block.XXXXXX)
begin
    echo "$dim# halftone differential · $day UTC$rst"
    echo "$acc❯$rst git rev-parse --short HEAD"
    echo "$commit$dirty_mark"
    echo "$acc❯$rst grep -c '^### D-' DIFFERENTIAL.md"
    echo $classes
    echo "$acc❯$rst cargo test -p halftone-cli --features c2pa --test differential -- --ignored"
    string match -r '^(?:differential|test result|\|).*' <$raw # (?:…): a capture group would print twice
end >$block

cat $block
echo
echo "record -> $base.txt"

if test $rc -ne 0
    echo "test failed (exit $rc); no image rendered" >&2
    rm -f $raw $block
    exit $rc
end
if test $png = 1
    freeze --execute "cat $block" --output $out --theme dracula --window \
        --padding 20,28 --font.family "JetBrains Mono" --font.size 14
    and echo "image  -> $out"
end
rm -f $raw $block
