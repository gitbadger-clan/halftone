#!/usr/bin/env fish
# Offline checks in a Linux container with no network interface (docker --network none),
# with every run logged to target/versiondiff/offline-<utc>-<commit>.log.
#
#   fish scripts/offline-docker.fish [--rebuild]
#
# 1. image is built from this checkout (rebuilt when HEAD or the tree changed)
# 2. the container has no network
# 3. snapshots ht + c2patool 0.28 with and without network; compares (want: 0 changed)
# 4. offline-trace.sh: strace controls, connect() counts, OCSP detail (want: ht 0)
#
# Exit: 0 ok · 1 setup or control failure · 3 ht tried the network · 4 output changed with network
# Caveat: tests Linux builds of this checkout, not the shipped macOS binary.

argparse rebuild -- $argv; or exit 2
set -q _flag_rebuild; and set -g rebuild 1 # argparse flags are script-local; main needs it
set -g repo (git rev-parse --show-toplevel); or exit 2
cd $repo
set -g image halftone-offline
set -g commit (git rev-parse --short HEAD)
test -n "$(git status --porcelain -- Cargo.toml Cargo.lock crates)"; and set commit $commit-dirty
mkdir -p target/versiondiff
set -g log target/versiondiff/offline-(date -u +%Y%m%dT%H%M%SZ)-$commit.log

function drun -a net
    docker run --rm --network $net --cap-add SYS_PTRACE -v $repo:/work -w /work $image $argv[2..]
end

function main
    echo "# Halftone offline check"
    echo "date:    "(date -u +%Y-%m-%dT%H:%M:%SZ)
    echo "commit:  $commit"

    # 1. image matches this checkout
    set -l built (docker image inspect -f '{{index .Config.Labels "org.halftone.commit"}}' $image 2>/dev/null)
    if set -q rebuild; or test "$built" != "$commit"; or string match -q '*-dirty' $commit
        echo "image:   building for $commit (was: "(test -n "$built"; and echo $built; or echo none)")"
        DOCKER_BUILDKIT=1 docker build -q -f docker/offline.dockerfile \
            --build-arg HALFTONE_COMMIT=$commit -t $image . >/dev/null; or return 1
    end
    echo "image:   "(docker image inspect -f '{{.Id}}' $image | string sub -l 19)" built from "(docker image inspect -f '{{index .Config.Labels "org.halftone.commit"}}' $image)
    echo "docker:  "(docker version -f '{{.Server.Version}}' 2>/dev/null)
    test -f target/conformance-v22/samples.json
    or begin
        echo "FAIL conformance samples not cached; run fish scripts/upgrade-check.fish once"
        return 1
    end

    # 2. no network
    echo
    echo "## network"
    if drun none python3 -c "import socket; socket.create_connection(('1.1.1.1', 443), 3)" 2>/dev/null
        echo "FAIL container reached the network with --network none"
        return 1
    end
    echo "ok   container has no network"

    # 3. does any output depend on the network?
    echo
    echo "## output with vs without network (ht + c2patool 0.28)"
    for net in none bridge
        drun $net sh -c "python3 scripts/versiondiff.py snapshot corpus/differential/0*/ --conformance \
            --label docker-$net --ht ht --c2patool /opt/c2patool-0.28/bin/c2patool --settings-style typed \
            --out target/versiondiff/docker-$net.json" | string replace -r '^' '  '; or return 1
    end
    set -l cmp (uv run scripts/versiondiff.py compare target/versiondiff/docker-bridge.json target/versiondiff/docker-none.json)
    printf '%s\n' $cmp | string match -r '^## (?:Halftone|c2patool):.*'
    set -l changed (printf '%s\n' $cmp | string match -r --groups-only '^## Halftone: (\d+) of')
    set -l c2changed (printf '%s\n' $cmp | string match -q '## c2patool:*'; and echo yes; or echo no)

    # 4. trace
    echo
    drun none sh scripts/offline-trace.sh
    set -l rc $status
    test $rc -ne 0; and return $rc
    if test "$changed" != 0; or test $c2changed = yes
        echo "FAIL output changed when the network was available; see compare above"
        return 4
    end
    echo
    echo "RESULT ok: no network dependence, no connection attempts by ht"
end

main 2>&1 | tee $log
set -l rc $pipestatus[1]
echo "log -> $log (exit $rc)"
exit $rc
