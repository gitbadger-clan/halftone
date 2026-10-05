#!/usr/bin/env fish
# Check 04-generators/SOURCE.md against the files on disk and the ground truth.
#
#   fish scripts/check-source.fish            # checks + facts sheet
#   fish scripts/check-source.fish <commit>   # also: byte-identical groups in that
#                                             # commit's expectations.json (pre-fix drops)
#
# Fails on: placeholders left, Files (N) total != disk, file variants or paths that
# SOURCE.md never mentions. Names in SOURCE.md that aren't on disk are reported only
# (Pending items name files that don't exist yet). The facts sheet and the hash
# groups are for reading each Finding against.

set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo
for tool in rg jq exiftool shasum
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

set -l g corpus/differential/04-generators
set -l src $g/SOURCE.md
set -l exp $g/expectations.json
set -l files (ls $g | rg '\.(png|jpe?g|webp)$')
set -l fail 0

echo "== placeholders"
# Naming uses <…> on purpose and Template is last; backticked spans are literal.
set -l ph (awk '/^## Template/{exit} /^## Naming/{skip=1; next} /^## /{skip=0}
    !skip {line=$0; gsub(/`[^`]*`/, "", line); print NR": "line}' $src | rg '\[\[|<[a-z]')
if set -q ph[1]
    printf '%s\n' $ph
    set fail 1
else
    echo ok
end

echo "== Files (N) total vs disk"
set -l doc (math (rg -o 'Files \(([0-9]+)\)' -r '$1' $src | string join +))
echo "SOURCE.md $doc, disk "(count $files)
test $doc -eq (count $files); or set fail 1

echo "== variants and paths SOURCE.md never mentions"
set -l und
for f in $files
    set -l p (string split -- '__' $f)
    for tok in $p[2] $p[3]
        rg -qF -- $tok $src; and continue
        set -l tail (string split -r -m1 -- - $tok)[-1] # -1x1, -upscaled, -no-bg …
        rg -qF -- "-$tail" $src; and continue
        set -a und "$tok  ($f)"
    end
end
if set -q und[1]
    printf '%s\n' $und
    set fail 1
else
    echo ok
end

echo "== names in SOURCE.md not on disk (expect only Pending files)"
for tok in (rg -o '[a-z0-9.-]+(__[a-z0-9.-]+)*__p[0-9]+__[0-9]+' $src | sort -u)
    string match -q -- "*$tok*" $files; or echo "  $tok"
end

echo "== facts sheet: bytes, real type (≠ = extension disagrees), size, XMP DST, c2patool state as collected"
exiftool -q -T -FileName -FileSize# -MIMEType -ImageSize -XMP-iptcExt:DigitalSourceType $g/$files | sort \
    | while read -l -d \t name bytes mime dims dst
    set -l ext (string lower (string split -r -m1 . $name)[-1])
    test $ext = jpg; and set ext jpeg
    set -l m (string replace image/ '' $mime)
    set -l mark ' '
    test "$ext" = "$m"; or set mark '≠'
    set -l state (jq -r --arg k $name '.files[$k].c2patool.validation_state // "-"' $exp)
    printf '%-64s %9s %s%-5s %-10s %-26s %s\n' $name $bytes $mark $m $dims (string replace -r '.*/' '' -- $dst) $state
end

echo "== byte-identical files on disk now"
shasum -a 256 $g/$files | awk '{n[$1]++; f[$1]=f[$1]"  "$2} END {for (h in n) if (n[h]>1) print substr(h,1,12) f[h]}'

if set -q argv[1]
    echo "== byte-identical groups in $argv[1]:$exp"
    git show $argv[1]:$exp | jq -r '.files | to_entries | group_by(.value.sha256)[]
        | select(length > 1) | map(.key) | join("  ==  ")'
end

exit $fail
