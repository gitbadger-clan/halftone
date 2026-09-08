#!/usr/bin/env fish
# Fetch ExifTool's test images (exiftool/exiftool, t/images) into
# corpus/differential/02-exiftool. Only files Halftone can load are kept, decided by
# sniffed MIME type, never by extension: this set deliberately contains mislabeled
# files. Writes SOURCE.txt with the commit and licence.
#
#   scripts/fetch-exiftool-fixtures.fish            # into corpus/differential/02-exiftool
#   scripts/fetch-exiftool-fixtures.fish --refresh  # wipe and re-fetch
#
# Requires: git ≥ 2.25 (sparse checkout), exiftool. Network: github.com only.
# The corpus directory is not for redistribution: ExifTool is GPL-1+/Artistic and
# the images are test material. Commit expectations.json, not the files.

set -l repo https://github.com/exiftool/exiftool
set -l subdir t/images
set -l root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
set -l dest $root/corpus/differential/02-exiftool
set -l keep image/jpeg image/png image/webp image/heic image/heif image/avif

if not type -q exiftool
    echo "exiftool not found on PATH" >&2
    exit 2
end

if test "$argv[1]" = --refresh
    rm -rf $dest
end
if test -e $dest/SOURCE.txt
    echo "$dest already populated ("(count $dest/*)" files); use --refresh to re-fetch"
    exit 0
end
mkdir -p $dest

set -l tmp (mktemp -d)
echo "sparse clone of $repo ($subdir) …"
git clone --quiet --depth 1 --filter=blob:none --sparse $repo $tmp/repo; or exit 1
git -C $tmp/repo sparse-checkout set $subdir; or exit 1
set -l commit (git -C $tmp/repo rev-parse --short HEAD)

set -l kept 0
set -l skipped 0
set -l relabeled 0
for f in (find $tmp/repo/$subdir -type f | sort)
    set -l mime (exiftool -s3 -MIMEType $f 2>/dev/null)
    if contains -- $mime $keep
        set -l rel (string replace "$tmp/repo/$subdir/" '' $f)
        set -l name (string replace -a / __ $rel)
        # Give the copy the extension its bytes deserve, keeping the original name in
        # front so a disagreement still points at the upstream file.
        set -l ext (string lower (path extension $name))
        set -l want (switch $mime
            case image/jpeg; echo .jpg
            case image/png; echo .png
            case image/webp; echo .webp
            case image/heic image/heif; echo .heic
            case image/avif; echo .avif
        end)
        if test "$ext" != "$want"; and not test "$ext" = .jpeg -a "$want" = .jpg
            set name "$name$want"
            set relabeled (math $relabeled + 1)
        end
        cp $f $dest/$name
        set kept (math $kept + 1)
    else
        set skipped (math $skipped + 1)
    end
end

printf "source: %s\nsubdir: %s\ncommit: %s\nfetched: %s\nkept: %d image files (sniffed jpeg/png/webp/heic/avif), %d of them with an extension that did not match their bytes (suffix added)\nskipped: %d non-image or unsupported files\nlicence: ExifTool is GPL-1+ OR Artistic; test images are test material. Do not redistribute this directory.\n" \
    $repo $subdir $commit (date -u +%Y-%m-%dT%H:%M:%SZ) $kept $relabeled $skipped > $dest/SOURCE.txt

rm -rf $tmp
echo "kept $kept ($relabeled relabeled), skipped $skipped -> $dest (commit $commit)"
echo "next: scripts/differential.py corpus/differential/02-exiftool"
