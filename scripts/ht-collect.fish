# ht-collect.fish — corpus collection helpers for corpus/differential/<stratum>.
#
# Every command that writes into the corpus needs the target stratum:
#   set -gx HT_STRATUM 06-phones      # or 04-generators, 00-base, 05-fold, 07-editors,
#                                     # 10-messaging, 11-screenshots (08-multipart is
#                                     # built by scripts/make-multipart-stratum.fish)
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
#   ht-range [-o dir] <day> [...] every image taken in a time window, numbered
#                                image1, image2, … (outside the corpus; see the function)
#
# Desktop (browser downloads into ~/Downloads):
#   ht-dmark                     start a capture: marker for ~/Downloads
#   ht-take [-f] <stem>          the one new download into the stratum, inspected;
#                                -f replaces an existing file, keeping the old copy
#   ht-tmp <path>                the one new download to <path>, outside the corpus
#
# Check:
#   ht-layout <files…>           per image: size, container parts, c2pa status
#
# Files replaced by ht-grab -f and ht-take -f are kept in /tmp/ht-replaced/
# (cleared on restart).
# Files keep the extension they were delivered with (SOURCE.md naming rule).
#
# Install or update (repo root, after every edit; overwrites saved copies):
#   source scripts/ht-collect.fish
#   funcsave ht-use ht-mark ht-type ht-send ht-new ht-latest ht-pull ht-grab \
#       ht-range ht-dmark ht-take ht-tmp ht-layout __ht_stratum __ht_phone_new __ht_desk_new
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
    if test -z "$stem"; or string match -q -- '-*' $stem
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

function ht-take --description 'Desktop: move the one new download into the stratum as <stem>.<delivered ext> and inspect it; -f replaces an existing file'
    set -l force 0
    if contains -- $argv[1] -f --force
        set force 1
        set -e argv[1]
    end
    set -l stem $argv[1]
    if test -z "$stem"; or string match -q -- '-*' $stem
        echo "usage: ht-take [-f] <name-without-extension>"
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
            # Only now, with the new download found, deal with any old file of this
            # name, under any extension, so one name never holds two versions.
            set -l old $dir/$stem.*
            if test (count $old) -gt 0
                if test $force -eq 0
                    echo "exists: $old[1] (use ht-take -f $stem to replace it)"
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

function ht-range --description 'Phone: pull every image taken in a time window as image1.<ext>, image2.<ext>, … (outside the corpus)'
    # ht-range [-o dir] YYYY-MM-DD                     the whole day
    # ht-range [-o dir] YYYY-MM-DD HH:MM               from HH:MM to the end of that day
    # ht-range [-o dir] YYYY-MM-DD HH:MM HH:MM         between two times that day
    # ht-range [-o dir] YYYY-MM-DD HH:MM +45m | +2h    from HH:MM for a duration
    #
    # Times are local, matched against the capture time Android records
    # (MediaStore datetaken); trashed and unfinished files are excluded.
    # Default destination: ~/halftone-candidates/<date>/, deliberately outside the
    # corpus so candidates are never collected. Numbering continues after any
    # imageN already there; index.tsv records imageN → phone path → capture time.
    # Files are pulled byte-exact and sha256-checked; ht-mark is not touched.
    # A failed pull or hash mismatch still uses up its imageN, so numbering can
    # have gaps; a mismatched file stays on disk without an index.tsv row.
    argparse 'o/out=' -- $argv; or return 1
    set -l day $argv[1]
    if not string match -qr '^\d{4}-\d{2}-\d{2}$' -- "$day"
        echo 'usage: ht-range [-o dir] YYYY-MM-DD [HH:MM [HH:MM | +N(m|h)]]'
        return 1
    end

    # Window [start, end) in epoch seconds, local time.
    set -l t0 00:00
    set -q argv[2]; and set t0 $argv[2]
    set -l start (date -j -f '%Y-%m-%d %H:%M' "$day $t0" +%s 2>/dev/null)
    or begin
        echo "not a time: $t0 (use HH:MM)"
        return 1
    end
    set -l end
    if set -q argv[3]
        set -l spec $argv[3]
        if string match -qr '^\+\d+[mh]$' -- $spec
            set -l num (string match -r '\d+' -- $spec)
            if test (string sub -s -1 -- $spec) = h
                set end (math $start + $num '*' 3600)
            else
                set end (math $start + $num '*' 60)
            end
        else
            set end (date -j -f '%Y-%m-%d %H:%M' "$day $spec" +%s 2>/dev/null)
            or begin
                echo "not a time or duration: $spec (use HH:MM, +45m or +2h)"
                return 1
            end
        end
    else
        # To midnight; via the next calendar day so DST days stay correct.
        set -l next (date -j -v+1d -f '%Y-%m-%d' "$day" +%Y-%m-%d)
        set end (date -j -f '%Y-%m-%d %H:%M' "$next 00:00" +%s)
    end
    if test $end -le $start
        echo "the window ends before it starts"
        return 1
    end

    # datetaken is in milliseconds.
    set -l where "datetaken >= "$start"000 AND datetaken < "$end"000 AND is_trashed=0 AND is_pending=0"
    set -l rows (adb shell content query --uri content://media/external/images/media \
        --projection _data:datetaken --where "\"$where\"" --sort '"datetaken ASC"')
    set -l hits (string replace -rf '^Row: \d+ _data=(.*), datetaken=(\d+)$' '$1|$2' -- $rows)
    if test (count $hits) -eq 0
        echo "no images taken between "(date -r $start '+%Y-%m-%d %H:%M')" and "(date -r $end '+%Y-%m-%d %H:%M')
        return 1
    end

    set -l dest ~/halftone-candidates/$day
    set -q _flag_out; and set dest $_flag_out
    mkdir -p $dest; or return 1

    # Continue after the highest imageN already in the folder.
    set -l n 0
    for e in $dest/image*.*
        set -l k (string match -r '^image(\d+)\.' -- (basename $e))[2]
        if test -n "$k"; and test $k -gt $n
            set n $k
        end
    end

    set -l pulled 0
    for h in $hits
        set -l parts (string split '|' -- $h)
        set -l p $parts[1]
        set -l taken (date -r (math -s0 $parts[2] / 1000) '+%Y-%m-%d %H:%M:%S')
        set n (math $n + 1)
        set -l ext (string match -r '[^.]+$' -- $p)
        set -l dst $dest/image$n.$ext
        if not adb pull $p $dst >/dev/null
            echo "FAILED   $p"
            continue
        end
        set -l a (adb shell sha256sum "'$p'" | string split -f1 ' ')
        set -l b (shasum -a 256 $dst | string split -f1 ' ')
        if test "$a" != "$b"
            echo "HASH MISMATCH  $dst" >&2
            continue
        end
        printf '%s\t%s\t%s\n' image$n.$ext $p $taken >>$dest/index.tsv
        printf 'image%-4s %s  %s\n' $n.$ext $taken (basename $p)
        set pulled (math $pulled + 1)
    end
    echo "$pulled image(s) → $dest"
end
