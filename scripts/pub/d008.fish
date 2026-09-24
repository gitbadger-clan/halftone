#!/usr/bin/env fish
# The D-008 capture: c2patool with the trust list in the env var (ignored), Halftone
# with the same list, c2patool with the list in a settings file.
#
#   scripts/pub/d008.fish            # print the block to the terminal
#   scripts/pub/d008.fish --png      # also render scripts/pub/out/d008.png with freeze
#   scripts/pub/d008.fish --png --out x.png
#
# Every line is a real command run against the real file; nothing is typed in by
# hand. The trust TOML is generated here the way the collector generates it
# (scripts/differential.py::c2patool_settings), from the vendored PEM.

set -l root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
set -l out $root/scripts/pub/out/d008.png
set -l png 0
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

set -l f corpus/differential/04-generators/aggregator-zimage__base__web-download__p1__1.png
set -l pem crates/halftone-c2pa/trust/C2PA-TRUST-LIST.pem
set -l toml scripts/pub/out/trust.toml

cd $root
for tool in ht c2patool jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end
if test $png = 1; and not type -q freeze
    echo "freeze not found on PATH (brew install charmbracelet/tap/freeze)" >&2
    exit 2
end
test -f $f; or begin
    echo "corpus file missing: $f" >&2
    exit 2
end
test -f $pem; or begin
    echo "trust list missing: $pem" >&2
    exit 2
end
mkdir -p (dirname $out) (dirname $toml)

# Same shape as the collector's settings file, PEM inlined.
printf '[verify]\nremote_manifest_fetch = false\nocsp_fetch = false\nverify_trust = true\n\n[trust]\ntrust_anchors = """\n%s"""\n' \
    (cat $pem | string collect) >$toml

# The script freeze executes. It prints each command as a prompt line, then runs it.
# Isolated from the operator's own c2patool config so the first line cannot come
# back Trusted by accident (D-008, update 2026-09-24).
set -l inner (mktemp -t d008.XXXXXX)
printf '%s\n' \
    "set -e C2PATOOL_SETTINGS" \
    "set -gx XDG_CONFIG_HOME (mktemp -d)" \
    "function show" \
    "    set_color brblack; printf '❯ '; set_color normal; echo \$argv" \
    "    fish -c \"\$argv\"" \
    end \
    "set_color brblack; echo '# same file, same trust list'; set_color normal" \
    "show 'set -l f $f'" \
    "show 'set -l pem $pem'" \
    echo \
    "show \"C2PATOOL_TRUST_ANCHORS=$pem c2patool $f | jq -r .validation_state\"" \
    "show \"ht inspect --json $f | jq -r '.evidence[] | select(.source.name==\\\"c2pa\\\") | .details.validation_state'\"" \
    "show \"c2patool --settings $toml $f | jq -r .validation_state\"" >$inner

fish $inner
set -l rc $status
if test $rc -ne 0
    rm -f $inner
    exit $rc
end

if test $png = 1
    freeze --execute "fish $inner" --execute.timeout 60s \
        --output $out --theme dracula --window --padding 20,28 \
        --font.family "JetBrains Mono" --font.size 14
    set rc $status
    test $rc -eq 0; and echo "wrote $out"
end
rm -f $inner
exit $rc
