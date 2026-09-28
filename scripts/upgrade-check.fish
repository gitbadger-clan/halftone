#!/usr/bin/env fish
# What does Halftone say differently after a dependency upgrade? (D-011: the tools
# both "worked" on c2pa 0.91; only a comparison showed trust had gone.)
#
#   fish scripts/upgrade-check.fish [base-ref]            # default base: main
#   fish scripts/upgrade-check.fish main \
#       --old-c2patool /opt/homebrew/bin/c2patool --old-style legacy \
#       --new-c2patool ~/.local/c2patool-0.28/bin/c2patool --new-style typed
#
# Builds <base-ref> in a worktree and HEAD in place, snapshots both over every stratum
# in corpus/differential/ plus the C2PA v2.2 conformance samples, and writes a report to
# target/versiondiff/. A base snapshot is reused on later runs (--rebuild to redo it),
# so each upgrade only builds the old tree once. Exit 1 = Halftone disagrees with the
# new c2patool somewhere (bug candidate); changes between versions are for review.

argparse 'old-c2patool=' 'old-style=' 'new-c2patool=' 'new-style=' rebuild keep-worktree -- $argv
or exit 2

set -l repo (git rev-parse --show-toplevel); or exit 2
cd $repo
set -l base_ref main
test (count $argv) -ge 1; and set base_ref $argv[1]
set -l base (git rev-parse --short $base_ref); or exit 2
set -l head (git rev-parse --short HEAD)
test -n "$(git status --porcelain -- Cargo.toml Cargo.lock crates)"; and set head "$head-dirty"
set -q _flag_old_style; or set _flag_old_style typed
set -q _flag_new_style; or set _flag_new_style typed

function c2pa_version -a lock
    grep -A1 '^name = "c2pa"$' $lock | string match -r --groups-only 'version = "(.*)"'
end
set -l base_c2pa (git show $base:Cargo.lock | grep -A1 '^name = "c2pa"$' | string match -r --groups-only 'version = "(.*)"')
set -l head_c2pa (c2pa_version Cargo.lock)
echo "base $base (c2pa $base_c2pa) → head $head (c2pa $head_c2pa)"

set -l strata corpus/differential/0*/
set -l out target/versiondiff
set -l base_snap $out/base-$base.json
set -l head_snap $out/head-$head.json
mkdir -p $out

if not test -f $base_snap; or set -q _flag_rebuild
    set -l wt /tmp/ht-base-$base
    test -d $wt; or git worktree add --detach --quiet $wt $base; or exit 1
    cargo build --release --locked --quiet -p halftone-cli --features c2pa \
        --manifest-path $wt/Cargo.toml; or exit 1
    set -l c2 (set -q _flag_old_c2patool; and printf '%s\n' --c2patool $_flag_old_c2patool --settings-style $_flag_old_style)
    uv run scripts/versiondiff.py snapshot $strata --conformance --label "c2pa-$base_c2pa@$base" \
        --note "base $base_ref" --ht $wt/target/release/ht $c2 --out $base_snap; or exit 1
    set -q _flag_keep_worktree; or git worktree remove --force $wt
else
    echo "reusing $base_snap"
end

cargo build --release --locked --quiet -p halftone-cli --features c2pa; or exit 1
set -l c2 (set -q _flag_new_c2patool; and printf '%s\n' --c2patool $_flag_new_c2patool --settings-style $_flag_new_style)
uv run scripts/versiondiff.py snapshot $strata --conformance --label "c2pa-$head_c2pa@$head" \
    --note HEAD --ht target/release/ht $c2 --out $head_snap; or exit 1

set -l report $out/report-$base-$head.md
uv run scripts/versiondiff.py compare $base_snap $head_snap | tee $report
set -l rc $pipestatus[1]
echo "report -> $report"
exit $rc
