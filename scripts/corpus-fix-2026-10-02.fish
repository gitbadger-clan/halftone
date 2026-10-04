#!/usr/bin/env fish
# Fix the 04-generators rows the corpus test flagged (inspection 2026-10-02).
# Every rename refuses to overwrite; every deletion first checks the file is
# byte-identical to the one kept, and skips it otherwise.
#
#   fish scripts/corpus-fix-2026-10-02.fish              # apply
#   fish scripts/corpus-fix-2026-10-02.fish --dry-run    # show what it would do
#   fish scripts/corpus-fix-2026-10-02.fish --no-meta    # leave Meta's identical web routes alone

set -g dry 0
contains -- --dry-run $argv; and set -g dry 1
set -g meta 1
contains -- --no-meta $argv; and set -g meta 0

set -l root (git rev-parse --show-toplevel); or exit 1
cd $root/corpus/differential/04-generators; or exit 1

set -g changed 0
set -g problems 0

function say --argument-names color
    set_color $color
    printf '%s\n' $argv[2..]
    set_color normal
end

function sha
    shasum -a 256 $argv[1] | string split -f1 ' '
end

function rename --argument-names from to why
    if not test -e $from
        if test -e $to
            say green "ok    $to (already renamed)"
        else
            say red "MISS  $from"
            set -g problems (math $problems + 1)
        end
        return
    end
    if test -e $to
        say red "SKIP  $to already exists; not overwriting"
        set -g problems (math $problems + 1)
        return
    end
    say normal "mv    $from -> $to   # $why"
    test $dry -eq 1; or mv $from $to
    set -g changed (math $changed + 1)
end

function drop_same --argument-names drop keep why
    if not test -e $drop
        say green "ok    $drop (already gone)"
        return
    end
    if not test -e $keep
        say red "SKIP  $drop: $keep is missing, so nothing would be kept"
        set -g problems (math $problems + 1)
        return
    end
    if test (sha $drop) != (sha $keep)
        say red "SKIP  $drop: not byte-identical to $keep"
        set -g problems (math $problems + 1)
        return
    end
    say normal "rm    $drop   # same bytes as $keep; $why"
    test $dry -eq 1; or rm $drop
    set -g changed (math $changed + 1)
end

say yellow "== 1. other generations filed under an existing n: next free n"
rename meta-ai__instant__app-save-open__p1__1.webp meta-ai__instant__app-save-open__p1__2.webp "app generation, not the web one"
rename meta-ai__instant__app-save-open__p2__2.webp meta-ai__instant__app-save-open__p2__3.webp "app generation"
rename meta-ai__instant__app-save-open__p3__3.webp meta-ai__instant__app-save-open__p3__4.webp "app generation"
rename meta-ai__instant__app-save-open__p4__4.webp meta-ai__instant__app-save-open__p4__5.webp "app generation"
rename meta-ai__instant__app-save-open__p5__5.jpg meta-ai__instant__app-save-open__p5__6.jpg "app generation"
rename meta-ai__thinking__app-save-open__p1__1.jpg meta-ai__thinking__app-save-open__p1__2.jpg "app generation"
rename bing-image__mai-image-2.5-flash__copy-image__p1__1.png bing-image__mai-image-2.5-flash__copy-image__p1__2.png "copied from a regenerated p1"
rename canva__canva-ai__web-download-jpg__p1__1.jpg canva__canva-ai__web-download-jpg__p1__2.jpg "JPEG export of generation 2"

say yellow "== 2. Z.ai 'wm-off' p1 browser-save/copy are copies of the wm-on image"
drop_same zai__glm-wm-off__browser-save__p1__1.jpeg zai__glm-wm-on__web-download__p1__1.png "saved from the wm-on generation by mistake"
drop_same zai__glm-wm-off__copy-image__p1__1.png zai__glm-wm-on__copy-image__p1__1.png "copied from the wm-on generation by mistake"

say yellow "== 3. routes that delivered identical bytes: keep the download"
drop_same bing-image__mai-image-2.5-flash__browser-save__p1__1.jpg bing-image__mai-image-2.5-flash__web-download__p1__1.jpg "browser-save = download"
drop_same zai__glm-wm-on__browser-save__p1__1.jpeg zai__glm-wm-on__web-download__p1__1.png "browser-save = download"
drop_same zai__glm-wm-on__browser-save__p2__2.jpeg zai__glm-wm-on__web-download__p2__2.png "browser-save = download"

if test $meta -eq 1
    say yellow "== 4. Meta web routes the test will flag next (identical bytes only)"
    drop_same meta-ai__instant__web-download-open__p1__1.jpg meta-ai__instant__web-download-thumb__p1__1.jpg "card and opened view give one file"
    drop_same meta-ai__instant__share-download-open__p1__1.jpg meta-ai__instant__web-download-thumb__p1__1.jpg "share download = download"
    drop_same meta-ai__instant__share-download-thumb__p1__1.jpg meta-ai__instant__web-download-thumb__p1__1.jpg "share download = download"
    drop_same meta-ai__instant__share-browser-save__p1__1.webp meta-ai__instant__browser-save-open__p1__1.webp "share browser-save = browser-save"
    drop_same meta-ai__thinking__share-download-thumb__p1__1.jpg meta-ai__thinking__web-download-thumb__p1__1.jpg "share download = download"
end

echo
if test $dry -eq 1
    say yellow "dry run: $changed change(s) planned, $problems problem(s)"
else
    say green "$changed change(s), $problems problem(s)"
end
test $problems -eq 0; or say red "check the SKIP/MISS lines before re-collecting"
say normal "next:" \
    "  1. decide Canva editor vs free (see the instructions), then re-collect 04 with all three trust inputs" \
    "  2. HALFTONE_CORPUS_REPORT=1 cargo test --release -p halftone-cli --test corpus -- --ignored --nocapture" \
    "  3. update SOURCE.md for every line above"
