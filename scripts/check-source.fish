#!/usr/bin/env fish
# Check 04-generators/SOURCE.md against the files on disk and the ground truth.
#
#   fish scripts/check-source.fish            # checks + facts sheet
#   fish scripts/check-source.fish <commit>   # also: byte-identical groups in that
#                                             # commit's expectations.json (pre-fix drops)
#
# Fails on: placeholders left; lowercased timestamps or D-numbers; text mangled by
# a lost backtick span; Files (N) total != disk; file variants or paths that
# SOURCE.md never mentions; short names (`p1__5`, `web-download__p1__4`) that match
# no file of their section's generator; Pending items already on disk; `## `
# sections with no generator, and generators on disk with no section.
# Names in SOURCE.md that aren't on disk are reported only (Pending items name
# files that don't exist yet). Open items are listed for reading: whether a line
# is stale or contradicts another is not something a script can tell.

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
if set -q argv[1]
    git rev-parse -q --verify "$argv[1]^{commit}" >/dev/null; or begin
        echo "argument must be a commit (e.g. HEAD), not a path: $argv[1]" >&2
        exit 2
    end
end

set -l g corpus/differential/04-generators
set -l src $g/SOURCE.md
set -l exp $g/expectations.json
set -l files (ls $g | rg '\.(png|jpe?g|webp)$')
set -l fail 0

# `## ` heading prefix = generator token. Subsections (`###`) inherit their
# parent's. A new generator section fails the run until it is added here.
set -l secmap 'Z.ai=zai' 'Google Flow=google-flow' 'Gemini app=gemini-app' \
    'Gemini API=gemini-api' 'aggregator-zimage=aggregator-zimage' 'Bing=bing-image' \
    'Adobe Firefly=adobe-firefly' 'ChatGPT=chatgpt' 'OpenAI API=openai-api' \
    'Meta AI=meta-ai' 'Canva=canva' 'Ideogram=ideogram'
set -l nogen Naming Midjourney 'Not yet collected' Template

# SOURCE.md body as "NR: line", without Naming (uses <…> on purpose) and from
# Template on. strip=1 removes backticked spans, which are literal.
function __body --argument-names strip src
    awk -v strip=$strip '/^## Template/{exit} /^## Naming/{skip=1; next} /^## /{skip=0}
        !skip {line=$0; if (strip) gsub(/`[^`]*`/, "", line); print NR": "line}' $src
end

function __report --argument-names title
    echo "== $title"
    if set -q argv[2]
        printf '%s\n' $argv[2..]
        return 1
    end
    echo ok
end

# <lowercase…>, <Uppercase…>, <, …>, <…>, <a | b>, [[…]]
__report placeholders (__body 1 $src | rg '\[\[|<[A-Za-z,…|]'); or set fail 1

# A case-folding edit: 2026-10-15t23:52z, 23:53z, (d-010)
__report "lowercased timestamps or D-numbers" \
    (__body 1 $src | rg '[0-9]{4}-[0-9]{2}-[0-9]{2}t[0-9]|[0-9]{2}:[0-9]{2}(:[0-9]{2})?z\b|\bd-[0-9]{3}\b'); or set fail 1

# A backtick span lost or glued: `p1__2download`, "model ()", "+ .", "``"
__report "text mangled by a lost backtick span" \
    (__body 0 $src | rg '__[0-9]+[A-Za-z]|\(\s*\)|\+\s+[.,;]|(^|[^`])``([^`]|$)'); or set fail 1

echo "== Files (N) total vs disk"
set -l doc (math (rg -o 'Files \(([0-9]+)\)' -r '$1' $src | string join +))
echo "SOURCE.md $doc, disk "(count $files)
test $doc -eq (count $files); or set fail 1

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
__report "variants and paths SOURCE.md never mentions" $und; or set fail 1

# Per section: short names in the text, backticked tokens in Pending bullets,
# headings without a generator. Withdrawn bullets name removed files: skipped.
set -l scan (awk -v map=(string join '|' $secmap) -v skip=(string join '|' $nogen) '
    BEGIN { n = split(map, m, "|"); k = split(skip, s, "|") }
    function gen_of(h,   i, kv) {
        for (i = 1; i <= k; i++) if (index(h, "## " s[i]) == 1) return ""
        for (i = 1; i <= n; i++) {
            split(m[i], kv, "=")
            if (index(h, "## " kv[1]) == 1) return kv[2]
        }
        return "?"
    }
    /^```/ { fence = !fence; next }
    fence { next }
    /^## Template/ { exit }
    /^## / {
        gen = gen_of($0); bul = ""
        if (gen == "?") { print "unmapped\t-\t" NR "\t" $0; gen = "" }
        next
    }
    /^- / { bul = $0 }
    /^[^ -]/ { bul = "" }
    gen == "" { next }
    bul ~ /^- Withdrawn/ { next }
    {
        line = $0
        if (bul ~ /^- Pending/) {
            while (match(line, /`[^`]+`/)) {
                print "pend\t" gen "\t" NR "\t" substr(line, RSTART + 1, RLENGTH - 2)
                line = substr(line, RSTART + RLENGTH)
            }
            next
        }
        while (match(line, /([a-z0-9.-]+__)*[pe][0-9]+__[0-9]+/)) {
            print "name\t" gen "\t" NR "\t" substr(line, RSTART, RLENGTH)
            line = substr(line, RSTART + RLENGTH)
        }
    }' $src)

set -l bad
set -l stale
set -l unmapped
for row in $scan
    set -l r (string split \t -- $row)
    switch $r[1]
        case unmapped
            set -a unmapped "$r[3]: $r[4]"
        case name
            set -l gf (string match -- "$r[2]__*" $files)
            string match -q -- "$r[4].*" $gf; and continue
            string match -q -- "*__$r[4].*" $gf; and continue
            set -a bad "$r[3]: $r[4]  (no $r[2] file ends in it)"
        case pend
            set -l gf (string match -- "$r[2]__*" $files)
            if string match -q -- "*__$r[4]__*" $gf; or string match -q -- "*__$r[4].*" $gf
                set -l hit (string match -- "*__$r[4]__*" $gf; string match -- "*__$r[4].*" $gf)
                set -a stale "$r[3]: $r[4]  (Pending, but on disk: $hit[1])"
            end
    end
end
__report "short names matching no file of their section's generator" $bad; or set fail 1
__report "Pending items already on disk" $stale; or set fail 1

set -l mapped (string replace -r '.*=' '' -- $secmap)
set -l orphan
for gen in (string replace -r '__.*' '' -- $files | sort -u)
    contains -- $gen $mapped; or set -a orphan "$gen  (files on disk, no section in the map)"
end
__report "sections without a generator, generators without a section" $unmapped $orphan; or set fail 1

echo "== names in SOURCE.md not on disk (expect only Pending files)"
for tok in (rg -o '[a-z0-9.-]+(__[a-z0-9.-]+)*__p[0-9]+__[0-9]+' $src | sort -u)
    string match -q -- "*$tok*" $files; or echo "  $tok"
end

echo "== open items (read each: still open?)"
awk '/^## /{nyc = ($0 ~ /^## Not yet collected/)} /^- /{pend = ($0 ~ /^- Pending/)}
    /^[^ -]/{pend = 0} (nyc && /^- /) || (pend && /^- /) {print NR": "$0}' $src

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
