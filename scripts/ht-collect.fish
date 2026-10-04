# ht-collect.fish — corpus collection helpers for corpus/differential/04-generators.
#
# Phone (adb; set ANDROID_SERIAL when more than one device is connected):
#   ht-mark                     start a capture: marker file on the phone
#   ht-type [-s] "prompt"       type into the focused field (-s: press Enter after)
#   ht-send                     tap the app's Send button (when Enter adds a newline)
#   ht-new                      files saved on the phone since ht-mark
#   ht-latest                   newest images by MediaStore date (ignores the marker)
#   ht-pull <remote> <name>     pull byte-exact into the stratum, sha256-checked, re-mark
#   ht-grab <stem>              the one new image: ht-pull as <stem>.<ext> + ht inspect
#
# Desktop (browser downloads into ~/Downloads):
#   ht-dmark                    start a capture: marker for ~/Downloads
#   ht-take <stem>              the one new download: move into the stratum + ht inspect
#   ht-tmp <path>               the one new download: move to <path>, outside the corpus
#
# Files keep the extension they were delivered with (SOURCE.md naming rule). Every
# "the one new …" command refuses when there are zero or several candidates and lists
# them instead of guessing.
#
# Install (once, from the repo root, in an interactive fish):
#   source scripts/ht-collect.fish
#   funcsave ht-mark ht-type ht-send ht-new ht-latest ht-pull ht-grab ht-dmark ht-take ht-tmp \
#       __ht_stratum __ht_phone_new __ht_desk_new
# funcsave overwrites earlier versions in ~/.config/fish/functions.

# ---- shared ---------------------------------------------------------------------

function __ht_stratum --description 'Absolute path of corpus/differential/04-generators'
    set -l root (git rev-parse --show-toplevel 2>/dev/null)
    or begin
        echo "run inside the halftone repo" >&2
        return 1
    end
    echo $root/corpus/differential/04-generators
end

# ---- phone ----------------------------------------------------------------------

function ht-mark --description 'Phone: mark the start of a capture'
    adb shell touch /storage/emulated/0/Download/ht-marker
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

function ht-latest --description 'Phone: newest images by MediaStore date_added'
    adb shell content query --uri content://media/external/images/media \
        --projection _data:date_added:mime_type --sort '"date_added DESC"' | head -5
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

function ht-grab --argument-names stem --description 'Phone: pull the one new image as <stem>.<delivered ext> and inspect it'
    if test -z "$stem"
        echo "usage: ht-grab <name-without-extension>"
        return 1
    end
    # (?:…): a capturing group would make fish print the extension as an extra line.
    set -l files (__ht_phone_new | string match -r -i '.*\.(?:jpe?g|png|webp|gif|heic|heif|avif)$')
    switch (count $files)
        case 0
            echo "no new image since ht-mark (check the save, or run ht-latest)"
            return 1
        case 1
            set -l ext (string match -r '[^.]+$' -- $files[1])
            echo "found $files[1]"
            ht-pull $files[1] $stem.$ext; or return 1
            ht inspect (__ht_stratum)/$stem.$ext
        case '*'
            echo "more than one new image since ht-mark; pull the right one with ht-pull:"
            printf '  %s\n' $files
            return 1
    end
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
