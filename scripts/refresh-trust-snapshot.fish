#!/usr/bin/env fish
# Refresh the vendored C2PA trust-list snapshot in crates/halftone-c2pa/trust/.
# Run from the workspace root; commit the result. CI does not run this.

set dir crates/halftone-c2pa/trust
set base https://raw.githubusercontent.com/c2pa-org/conformance-public/main/trust-list
test -d $dir; or begin
    echo "run from the workspace root" >&2
    exit 1
end

for f in C2PA-TRUST-LIST.pem C2PA-TSA-TRUST-LIST.pem
    curl -fsSL -o $dir/$f.new $base/$f; or exit 1
    if test (grep -c 'BEGIN CERTIFICATE' $dir/$f.new) -eq 0
        echo "$f: no certificates in response" >&2
        rm -f $dir/$f.new
        exit 1
    end
    mv $dir/$f.new $dir/$f
end

set commit (curl -fsSL "https://api.github.com/repos/c2pa-org/conformance-public/commits?path=trust-list&per_page=1" \
    | python3 -c "import sys,json; c=json.load(sys.stdin)[0]; print(c['sha'][:12], c['commit']['committer']['date'])" 2>/dev/null)
test -n "$commit"; or set commit "unknown (API unavailable)"

begin
    echo "c2pa-org/conformance-public@$commit"
    echo "source: $base"
    echo "fetched: "(date -u +%Y-%m-%dT%H:%M:%SZ)
    for f in C2PA-TRUST-LIST.pem C2PA-TSA-TRUST-LIST.pem
        echo "$f sha256 "(shasum -a 256 $dir/$f | cut -d' ' -f1)" ("(grep -c 'BEGIN CERTIFICATE' $dir/$f)" certs)"
    end
end > $dir/SNAPSHOT

cat $dir/SNAPSHOT
git -C . status --short $dir
