#!/usr/bin/env fish
# One line per file: what the manifest layer and the XMP marking say. This is the
# survival table's first column set, produced from `ht inspect --batch --json` so
# the numbers come from the same document the report will use.
#
#   scripts/table.fish corpus/differential/04-generators
#   scripts/table.fish corpus/differential/04-generators --tsv > table.tsv
#   scripts/table.fish some/dir --only manifest        # manifest columns only
#
# Columns: file · manifest state (Trusted / Valid / Invalid / remote / -) · signer ·
# declared digitalSourceType · ingredient count · actions · xmp (none / present /
# the DigitalSourceType term).
#
# Requires: ht on PATH (built with --features c2pa), jq. column(1) for the aligned
# view; --tsv skips it.

set -l dir ""
set -l tsv 0
set -l only manifest,container
set -l i 1
while test $i -le (count $argv)
    switch $argv[$i]
        case --tsv
            set tsv 1
        case --only
            set i (math $i + 1)
            set only $argv[$i]
        case '*'
            set dir $argv[$i]
    end
    set i (math $i + 1)
end
if test -z "$dir"; or not test -d "$dir"
    echo "usage: scripts/table.fish <directory> [--tsv] [--only manifest,container]" >&2
    exit 2
end
for tool in ht jq
    type -q $tool; or begin
        echo "$tool not found on PATH" >&2
        exit 2
    end
end

set -l files (find $dir -maxdepth 1 -type f \( -iname '*.png' -o -iname '*.jpg' -o -iname '*.jpeg' -o -iname '*.webp' -o -iname '*.heic' -o -iname '*.heif' -o -iname '*.avif' \) | sort)
if test (count $files) -eq 0
    echo "no image files in $dir" >&2
    exit 1
end

set -l jq_prog '
  def col: if . == null or . == "" then "-" else . end;
  ["file","manifest","signer","declared","ingredients","actions","xmp"], (
  .inspections[] as $i
  | ([$i.evidence[] | select(.source.name=="c2pa")][0]) as $m
  | ([$i.evidence[] | select(.source.name=="marking_metadata")][0]) as $x
  | [
      ($i.asset.path | split("/") | last),
      ($m.details.validation_state
        // (if ($m.details.remote_manifest_url // null) then "remote" else null end)
        | col),
      ($m.details.issuer | col),
      ($m.details.digital_source_type // [] | join(",") | col),
      (($m.details.ingredients // null) | if . == null then "-" else tostring end),
      ([$m.details.digital_source_type_hits[]?.action] | join(",") | col),
      (if $x == null then "n/a"
       elif ($x.details.xmp_packets // 0) > 0
         then ($x.details.digital_source_type // [] | join(",") | if . == "" then "present" else . end)
       else "none" end)
    ]) | @tsv'

# ht exits 2 when anything is Present; that is a verdict, not an error.
set -l out (ht inspect --batch --json --only $only $files | jq -r $jq_prog)
if test $tsv = 1
    printf '%s\n' $out
else if type -q column
    printf '%s\n' $out | column -t -s \t
else
    printf '%s\n' $out
end
