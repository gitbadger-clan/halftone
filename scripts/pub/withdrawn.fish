#!/usr/bin/env fish
# Frame for the withdrawn-findings post: the two dated Withdrawn entries from the
# committed SOURCE.md, and the byte-identical groups that exposed the Z.ai files.
#
#   fish scripts/pub/withdrawn.fish          # print the frame
#   fish scripts/pub/withdrawn.fish --png    # render scripts/pub/out/withdrawn.png
#
# Requires: git, jq, awk; freeze for --png

set -l self (realpath (status filename)) # before `cd $repo`, so relative invocations still resolve
set -l repo (git rev-parse --show-toplevel 2>/dev/null); or begin
    echo "run inside the halftone repo" >&2
    exit 2
end
cd $repo

set -l src corpus/differential/04-generators/SOURCE.md
set -l exp corpus/differential/04-generators/expectations.json
set -l pre 18dbbe5 # last commit before the 2026-10-02 corpus fix
set -l out scripts/pub/out/withdrawn.png

# --- render mode ------------------------------------------------------------
if contains -- --png $argv
    type -q freeze; or begin
        echo "freeze not found: brew install charmbracelet/tap/freeze" >&2
        exit 2
    end
    mkdir -p (dirname $out)
    freeze --execute "fish $self" \
        --output $out \
        --background '#1c1e26' \
        --font.size 15 \
        --line-height 1.35 \
        --wrap 92 \
        --padding 24,32 \
        --margin 0 \
        --border.radius 8 \
        --window
    echo "wrote $out"
    exit 0
end

# --- frame ------------------------------------------------------------------
git cat-file -e HEAD:$src 2>/dev/null; or begin
    echo "$src is not committed: commit it first, the post points at the repo" >&2
    exit 1
end
# Committed text only, so the image shows what the repo says.
set -l body (git show HEAD:$src | awk '/^- Withdrawn/{p=1; print; next} p && /^(- |#)/{p=0} p')
test (count (string match -- '- Withdrawn*' $body)) -eq 2; or begin
    echo "expected exactly two Withdrawn entries in HEAD:$src" >&2
    exit 1
end

set -l D (set_color brblack)
set -l H (set_color --bold brcyan)
set -l R (set_color normal)

echo "$D# 04-generators/SOURCE.md, Withdrawn entries (HEAD "(git rev-parse --short HEAD)")$R"
echo
for line in $body
    if string match -q -- '- Withdrawn*' $line
        echo $H$line$R
    else
        echo $line
    end
end
echo
echo "$D# Z.ai files byte-identical before the fix ($pre:expectations.json)$R"
git show $pre:$exp | jq -r '
    .files | to_entries | group_by(.value.sha256)[]
    | select(length > 1) | map(.key) | select(any(.[]; startswith("zai__")))
    | map(sub("^zai__glm-"; "") | sub("__"; " ") | sub("__"; " ") | sub("\\\\.[a-z]+$"; "")) | join("  ==  ")'
echo
echo "$D# Firefly: the re-download was deleted on 2026-10-02 only after its sha256$R"
echo "$D# matched the upscaled file's.$R"
