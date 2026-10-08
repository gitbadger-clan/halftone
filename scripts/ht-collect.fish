# ht-collect.fish — corpus collection helpers for corpus/differential/<stratum>.
#
# Every command that writes into the corpus needs the target stratum:
#   set -gx HT_STRATUM 06-phones      # or 04-generators, 00-base, 05-fold, 07-editors
# Without it they refuse: a missing value once filed phone shots under 04.
#
# Phone (adb; one phone per terminal):
#   ht-use <model part>          select the phone for this terminal (sets ANDROID_SERIAL)
#   ht-mark                      start a capture: marker file on the phone
#   ht-type [-s] "prompt"        type into the focused field (-s: press Enter after)
#   ht-send                      tap the app's Send button
#   ht-new                       files saved since ht-mark (unfiltered, for diagnosis)
#   ht-latest                    newest finished images (trashed and pending excluded)
#   ht-pull <remote> <name.ext>  pull byte-exact, sha256-checked, re-mark
#   ht-grab [-f] <stem>          the one new image as <stem>.<delivered ext>, inspected;
#                                -f replaces an existing file, keeping the old copy
#
# Desktop (browser downloads into ~/Downloads):
#   ht-dmark                     start a capture: marker for ~/Downloads
#   ht-take <stem>               the one new download into the stratum, inspected
#   ht-tmp <path>                the one new download to <path>, outside the corpus
#
# Check:
#   ht-layout <files…>           per image: size, container parts, c2pa status
#
# Files replaced by ht-grab -f are kept in /tmp/ht-replaced/.
# Files keep the extension they were delivered with (SOURCE.md naming rule).
#
# Install or update (repo root, after every edit; overwrites saved copies):
#   source scripts/ht-collect.fish
#   funcsave ht-use ht-mark ht-type ht-send ht-new ht-latest ht-pull ht-grab \
#       ht-dmark ht-take ht-tmp ht-layout __ht_stratum __ht_phone_new __ht_desk_new

# ---- shared ---------------------------------------------------------------------

function __ht_stratum --description 'Absolute path of the stratum named by $HT_STRATUM (required)'
    set -l root (git rev-parse --show-toplevel 2>/dev/null)
    or begin
        echo "run inside the halftone repo" >&2
        return 1
    end
    if not set -q HT_STRATUM; or test -z "$HT_STRATUM"
        echo "HT_STRATUM is not set: e.g. set -gx HT_STRATUM 06-phones" >&2
        return 1
    end
    set -l dir $root/corpus/differential/$HT_STRATUM
    test -d $dir; or begin
        echo "no such stratum folder: $dir" >&2
        return 1
    end
    echo $dir
end

function ht-layout --description 'Per image: size, container parts, c2pa status'
    for f in $argv
        string match -q -r -i '\.(?:jpe?g|png|webp|heic|heif|avif)$' -- $f; or continue
        set -l parts (exiftool -s3 -XMP-GContainer:DirectoryItemSemantic $f)
        if test -z "$parts"
            string match -q -r -i '\.jpe?g$' -- $f; and set parts "single JPEG"; or set parts "no container"
        end
        set -l c2pa (ht inspect --json --only manifest $f 2>/dev/null | jq -r '.evidence[] | select(.source.name == "c2pa") | .status')
        test -n "$c2pa"; or set c2pa "?"
        printf '%-50s %9s B  %-30s c2pa=%s\n' (basename $f) (wc -c < $f | string trim) "$parts" $c2pa
    end
end

# ---- phone ----------------------------------------------------------------------

function ht-mark --description 'Phone: mark the start of a capture'
    adb shell touch /storage/emulated/0/Download/ht-marker
end

function ht-use --argument-names want --description 'Select the adb device whose model contains <want>, e.g. fold or pixel_5'
    if test -z "$want"
        adb devices -l
        echo "usage: ht-use <part of the model name>"
        return 1
    end
    set -l hits (adb devices -l | string match -i "* device *model:*$want*")
    switch (count $hits)
        case 0
            echo "no ready device whose model matches '$want':"
            adb devices -l
            return 1
        case 1
            set -gx ANDROID_SERIAL (string split -f1 ' ' -- $hits[1])
            echo "ANDROID_SERIAL=$ANDROID_SERIAL"
        case '*'
            echo "several connections match '$want' (same phone over USB and wireless?);"
            echo "drop one with: adb disconnect <serial>"
            printf '  %s\n' $hits
            return 1
    end
end

function __ht_phone_new --description 'Phone: files newer than the marker'
    adb shell test -e /storage/emulated/0/Download/ht-marker
    or begin
        echo "no marker on the phone: run ht-mark before saving" >&2
        return 1
    end
    # /sdcard is a symlink; find must start at the real directory. Permission noise
    # from app-private folders is dropped.
    adb shell find /storage/emulated/0/ -type f -newer /storage/emulated/0/Download/ht-marker 2>/dev/null \
        | string trim
end

function ht-new --description 'Phone: files saved since ht-mark'
    __ht_phone_new
end

function ht-pull --argument-names remote name --description 'Phone: pull into the stratum, sha256-checked'
    if test -z "$remote" -o -z "$name"
        echo "usage: ht-pull <remote path> <name>"
        return 1
    end
    set -l dir (__ht_stratum); or return 1
    set -l dst $dir/(basename $name)
    if test -e $dst
        echo "exists: $dst"
        return 1
    end
    adb pull $remote $dst >/dev/null; or return 1
    set -l a (adb shell sha256sum "'$remote'" | string split -f1 ' ')
    set -l b (shasum -a 256 $dst | string split -f1 ' ')
    if test "$a" != "$b"
        echo "HASH MISMATCH  $dst (left in place for inspection)" >&2
        return 1
    end
    echo "ok  $dst  "(file -b --mime-type $dst)
    ht-mark
end

function ht-grab --description 'Phone: pull the one new image as <stem>.<delivered ext> and inspect it; -f replaces an existing file'
    set -l force 0
    if contains -- $argv[1] -f --force
        set force 1
        set -e argv[1]
    end
    set -l stem $argv[1]
    if test -z "$stem"
        echo "usage: ht-grab [-f] <name-without-extension>"
        return 1
    end
    # (?:…): a capturing group would make fish print the extension as an extra line.
    set -l all (__ht_phone_new | string match -r -i '.*\.(?:jpe?g|png|webp|gif|heic|heif|avif)$')
    # Hidden names are not finished photos: .trashed-… (in the bin),
    # .pending-… (still being written), .thumbnails/… (the gallery's cache).
    set -l files (string match -v -r '/\.' -- $all)
    set -l pending (string match -r '.*/\.pending-.*' -- $all)
    switch (count $files)
        case 0
            if test (count $pending) -gt 0
                echo "a photo is still being written ($pending[1]); wait a moment and run ht-grab again"
            else
                echo "no new image since ht-mark (check the save, or run ht-latest)"
            end
            return 1
        case 1
            set -l dir (__ht_stratum); or return 1
            set -l ext (string match -r '[^.]+$' -- $files[1])
            # Only now, with the new photo found, move any old file of this name aside.
            set -l old $dir/$stem.*
            if test (count $old) -gt 0
                if test $force -eq 0
                    echo "exists: $old[1] (use ht-grab -f $stem to replace it)"
                    return 1
                end
                set -l bin /tmp/ht-replaced
                mkdir -p $bin
                for o in $old
                    set -l kept $bin/(date +%Y%m%d-%H%M%S)-(basename $o)
                    mv $o $kept
                    echo "replaced: $o (old copy: $kept)"
                end
            end
            echo "found $files[1]"
            ht-pull $files[1] $stem.$ext; or return 1
            ht inspect $dir/$stem.$ext
        case '*'
            echo "more than one new image since ht-mark; pull the right one with ht-pull:"
            printf '  %s\n' $files
            return 1
    end
end

function ht-latest --description 'Phone: newest images by MediaStore date_added, excluding trashed and unfinished'
    adb shell content query --uri content://media/external/images/media \
        --projection _data:date_added:mime_type \
        --where '"is_trashed=0 AND is_pending=0"' \
        --sort '"date_added DESC"' | head -5
end

function ht-type --description 'Phone: type text into the focused field; -s presses Enter after'
    set -l send 0
    if contains -- -s $argv[1]
        set send 1
        set -e argv[1]
    end
    if test (count $argv) -eq 0
        echo 'usage: ht-type [-s] "prompt"'
        return 1
    end
    # The text goes through a file on the phone so its shell never re-parses
    # punctuation. `input text` takes plain ASCII only, and %s means a space.
    set -l tmp (mktemp)
    string join ' ' -- $argv >$tmp
    adb push $tmp /data/local/tmp/ht-prompt.txt >/dev/null
    rm $tmp
    adb shell 'input text "$(sed "s/ /%s/g" /data/local/tmp/ht-prompt.txt)"'
    if test $send -eq 1
        sleep 0.5
        adb shell input keyevent KEYCODE_ENTER
    end
end

function ht-send --description "Phone: tap the app's Send button"
    adb shell uiautomator dump /data/local/tmp/ui.xml >/dev/null
    set -l m (adb shell cat /data/local/tmp/ui.xml \
        | string match -r -i 'content-desc="[^"]*send[^"]*"[^>]*?bounds="\[(\d+),(\d+)\]\[(\d+),(\d+)\]"')
    if test (count $m) -lt 5
        echo "no send button in the UI dump (list labels: adb shell cat /data/local/tmp/ui.xml)"
        return 1
    end
    adb shell input tap (math -s0 "($m[2] + $m[4]) / 2") (math -s0 "($m[3] + $m[5]) / 2")
end

# ---- desktop --------------------------------------------------------------------

function ht-dmark --description 'Desktop: mark the start of a capture in ~/Downloads'
    touch ~/.ht-desk-marker
end

function __ht_desk_new --description 'Desktop: finished image downloads newer than the marker'
    if not test -e ~/.ht-desk-marker
        echo "run ht-dmark before downloading" >&2
        return 1
    end
    find ~/Downloads -maxdepth 1 -type f -newer ~/.ht-desk-marker \
        ! -name '*.crdownload' ! -name '*.part' ! -name '*.download' ! -name '.DS_Store' \
        | string match -r -i '.*\.(?:jpe?g|png|webp|gif|heic|heif|avif)$'
end

function ht-take --argument-names stem --description 'Desktop: move the one new download into the stratum as <stem>.<delivered ext> and inspect it'
    if test -z "$stem"
        echo "usage: ht-take <name-without-extension>"
        return 1
    end
    set -l files (__ht_desk_new)
    switch (count $files)
        case 0
            echo "no new image in ~/Downloads since ht-dmark (download still running?)"
            return 1
        case 1
            set -l dir (__ht_stratum); or return 1
            set -l ext (string match -r '[^.]+$' -- $files[1])
            set -l dst $dir/$stem.$ext
            if test -e $dst
                echo "exists: $dst"
                return 1
            end
            echo "found $files[1]"
            mv $files[1] $dst; or return 1
            echo "ok  $dst  "(file -b --mime-type $dst)
            ht inspect $dst
            ht-dmark
        case '*'
            echo "more than one new image since ht-dmark; move the right one by hand:"
            printf '  %s\n' $files
            return 1
    end
end

function ht-tmp --argument-names dest --description 'Desktop: move the one new download to <dest>, keeping its delivered extension'
    if test -z "$dest"
        echo "usage: ht-tmp <path>"
        return 1
    end
    set -l files (__ht_desk_new)
    if test (count $files) -ne 1
        echo "found "(count $files)" new image(s) since ht-dmark:"
        printf '  %s\n' $files
        return 1
    end
    set -l ext (string match -r '[^.]+$' -- $files[1])
    # Any extension given in <dest> is replaced by the delivered one.
    set -l target (string replace -r '\.[A-Za-z0-9]+$' '' -- $dest).$ext
    if test -e $target
        echo "exists: $target"
        return 1
    end
    mv $files[1] $target; or return 1
    echo "ok  $target  "(file -b --mime-type $target)
    ht-dmark
end
