#!/usr/bin/env fish
# The two assets for the D-008 reply post:
#   d008-help.png  c2patool --help, the lines that name the default settings file
#   d008-diff.png  the differential test summary (files compared, disagreements)
#
#   scripts/pub/d008-reply.fish               # print both blocks to the terminal
#   scripts/pub/d008-reply.fish --png         # also render both PNGs with freeze
#   scripts/pub/d008-reply.fish --skip-test   # help excerpt only (the test takes ~40 s)
#
# Both images are real output of the commands shown; nothing is typed in by hand.
# The test run needs corpus/differential on disk and ht built with --features c2pa.

set -l root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
set -l outdir $root/scripts/pub/out
set -l png 0
set -l run_test 1
while set -q argv[1]
    switch $argv[1]
        case --png
            set png 1
        case --skip-test
            set run_test 0
        case '*'
            echo "unknown argument: $argv[1]" >&2
            exit 2
    end
    set -e argv[1]
end

cd $root
for tool in c2patool grep
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
if test $png = 1; and not type -q freeze
    echo "freeze not found on PATH (brew install charmbracelet/tap/freeze)" >&2
    exit 2
end
if test $run_test = 1; and not test -d corpus/differential
    echo "corpus/differential not on disk; pass --skip-test" >&2
    exit 2
end
mkdir -p $outdir

# Shared look, same as d008.fish so the two posts match.
set -l style --theme dracula --window --padding 20,28 --font.family "JetBrains Mono" --font.size 14

# 1. The help excerpt: the flag and the default path c2patool reads without saying so.
set -l help_cmd "c2patool --help | grep -A3 -- '--settings <'"
# 2. The differential summary: one line per stratum, the total, the verdict. freeze
#    kills executed commands after 10 s by default; the test needs about 40.
set -l test_cmd "cargo test -p halftone-cli --features c2pa --test differential -- --ignored --nocapture 2>&1 | grep -E '^(differential|test result)'"

echo "# d008-help.png"
fish -c $help_cmd
if test $run_test = 1
    echo
    echo "# d008-diff.png"
    fish -c $test_cmd
end

test $png = 1; or exit 0

freeze --execute "fish -c \"$help_cmd\"" --output $outdir/d008-help.png $style
or exit $status
echo "wrote $outdir/d008-help.png"

if test $run_test = 1
    freeze --execute "fish -c \"$test_cmd\"" --execute.timeout 5m --output $outdir/d008-diff.png $style
    or exit $status
    echo "wrote $outdir/d008-diff.png"
end
