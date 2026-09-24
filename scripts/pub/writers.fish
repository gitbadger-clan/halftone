#!/usr/bin/env fish
# One row per writer (<generator>__<variant>): what its download path carries.
# The survival table's first column, for a directory that follows the corpus naming
# convention. Only web-download* files count (browser-save, copy-image and share
# routes are survival rows, not writer facts); byte-identical duplicates fold.
#
#   scripts/pub/writers.fish corpus/differential/04-generators
#   scripts/pub/writers.fish corpus/differential/04-generators --md > writers.md
#   scripts/pub/writers.fish corpus/differential/04-generators --png writers.png   # via freeze
#   scripts/pub/writers.fish corpus/differential/04-generators --png writers.png --dark
#   scripts/pub/writers.fish corpus/differential/04-generators --group generator   # one row per generator
#
# Columns: writer · files · manifest (state as evaluated today, or remote / none) ·
# signer · declared term · ingredients · xmp field · evaluated (date).

set -l dir ""
set -l md 0
set -l png ""
set -l group writer
set -l dark 0
set -l i 1
while test $i -le (count $argv)
    switch $argv[$i]
        case --md
            set md 1
        case --png
            set i (math $i + 1)
            set png $argv[$i]
        case --dark
            set dark 1
        case --group
            set i (math $i + 1)
            set group $argv[$i]
            contains -- $group writer generator; or begin
                echo "--group must be writer or generator" >&2
                exit 2
            end
        case '*'
            set dir $argv[$i]
    end
    set i (math $i + 1)
end
if test -z "$dir"; or not test -d "$dir"
    echo "usage: scripts/pub/writers.fish <directory> [--md] [--png out.png]" >&2
    exit 2
end
if test -n "$png"; and not type -q freeze
    echo "freeze not found on PATH (brew install charmbracelet/tap/freeze)" >&2
    exit 2
end
for tool in ht jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

set -l files (find $dir -maxdepth 1 -type f -name '*__web-download*' \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.webp' \) | sort)
if test (count $files) -eq 0
    echo "no *__web-download* files in $dir" >&2
    exit 1
end

set -l today (date -u +%Y-%m-%d)
set -l jq_prog '
  def col: if . == null or . == "" then "-" else . end;
  [ .inspections[] as $i
    | ([$i.evidence[] | select(.source.name=="c2pa")][0]) as $m
    | ([$i.evidence[] | select(.source.name=="marking_metadata")][0]) as $x
    | {
        writer: ($i.asset.path | split("/") | last | split("__") | .[0:$tokens] | join("__")),
        sha: $i.asset.sha256,
        manifest: ($m.details.validation_state
                   // (if ($m.details.remote_manifest_url // null) then "remote" else "none" end)),
        signer: ($m.details.issuer | col),
        declared: ($m.details.digital_source_type // [] | join(",") | col),
        ingredients: (($m.details.ingredients // null) | if . == null then "-" else tostring end),
        xmp: (if $x == null then "n/a"
              elif ($x.details.xmp_packets // 0) > 0
                then ($x.details.digital_source_type // [] | join(",") | if . == "" then "present, no term" else . end)
              else "none" end)
      } ]
  | unique_by(.sha)                       # byte-identical files fold
  | group_by(.writer)
  | map({
      writer: .[0].writer,
      files: length,
      manifest: (map(.manifest) | unique | join("/")),
      signer: (map(.signer) | unique | join("/")),
      declared: (map(.declared) | unique | join("/")),
      ingredients: (map(.ingredients) | unique | join("/")),
      xmp: (map(.xmp) | unique | join("/"))
    })
  | .[] | [.writer, (.files|tostring), .manifest, .signer, .declared, .ingredients, .xmp] | @tsv'

set -l tokens 2
test $group = generator; and set tokens 1
set -l rows (ht inspect --batch --json --only manifest,container $files | jq -r --argjson tokens $tokens $jq_prog)
set -l header (printf 'writer\tfiles\tmanifest\tsigner\tdeclared\tingredients\txmp field')

if test $md = 1
    echo "| writer | files | manifest ($today) | signer | declared | ingredients | XMP field |"
    echo "|---|---|---|---|---|---|---|"
    for r in $rows
        echo "| "(string replace -a \t ' | ' $r)" |"
    end
    echo
    echo "Download paths only; byte-identical files counted once. Manifest state as evaluated on $today against the vendored C2PA trust list; Valid = signature and hash verify but the signer is not on that list; Invalid = manifest present but the signature does not verify today (expired certificate or broken binding); remote = the file carries only a URL to a manifest, not fetched."
else
    set -l text (printf '%s\n' $header $rows | column -t -s \t)
    set -a text ""
    set -a text "manifest state evaluated $today · download paths only · byte-identical files counted once"
    set -a text "Valid = signature and hash verify, signer not on the vendored C2PA list · Invalid = manifest present, signature does not verify today (expired certificate or broken binding; see the rationale)"
    set -a text "remote = only a URL to a manifest, not fetched · none = no manifest · a/b = the writer's files differ"
    printf '%s\n' $text
    if test -n "$png"
        # Plain text, no window chrome: reads in a feed and prints. Light by
        # default; --dark uses the same palette's dark variant on its own base
        # colour so the table does not float on a white slab.
        set -l theme catppuccin-latte
        set -l bg '#ffffff'
        if test $dark = 1
            set theme catppuccin-mocha
            set bg '#1e1e2e'
        end
        set -l tmp (mktemp -t writers.XXXXXX.txt)
        printf '%s\n' $text >$tmp
        freeze $tmp --language text --theme $theme \
            --font.family "JetBrains Mono, Menlo, monospace" --font.size 14 \
            --padding 24,32 --margin 0 --border.radius 8 --background $bg \
            --shadow.blur 0 --output $png; or begin
            rm -f $tmp
            exit 1
        end
        rm -f $tmp
        echo "wrote $png" >&2
    end
end
