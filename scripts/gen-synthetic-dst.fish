#!/usr/bin/env fish
# Stratum 3: synthetic DigitalSourceType edge cases, written with ExifTool onto plain
# base images, into corpus/differential/03-synthetic. Every file's intended Halftone
# verdict is known by construction and recorded in cases.json next to the files;
# scripts/differential.py merges it into expectations.json as `halftone`, and the
# differential test asserts it alongside the ExifTool/c2patool comparison.
#
#   scripts/gen-synthetic-dst.fish              # writes corpus/differential/03-synthetic
#   scripts/gen-synthetic-dst.fish --refresh    # regenerate; keeps expectations.json
#
# Base images: corpus/differential/00-base/base.{jpg,png,webp}; created via
# `uv run --with pillow --with numpy` if absent. Requires exiftool ≥ 12 and uv.

set -g root (git rev-parse --show-toplevel 2>/dev/null; or pwd)
set -g base $root/corpus/differential/00-base
set -g dest $root/corpus/differential/03-synthetic
set -g scheme http://cv.iptc.org/newscodes/digitalsourcetype

if not type -q exiftool
    echo "exiftool not found on PATH" >&2
    exit 2
end
if test "$argv[1]" = --refresh
    # Regenerate the files and cases.json; keep the committed expectations.json.
    if test -d $dest
        for f in $dest/*
            test (basename $f) = expectations.json; or rm -rf $f
        end
    end
end
# "Populated" means generated files are present, not just the committed ground truth.
set -l generated (find $dest -maxdepth 1 -type f ! -name expectations.json ! -name cases.json 2>/dev/null)
if test (count $generated) -gt 0
    echo "$dest already populated ("(count $generated)" files); use --refresh to regenerate"
    exit 0
end
mkdir -p $dest $base

if not test -e $base/base.jpg -a -e $base/base.png -a -e $base/base.webp
    uv run --quiet --with pillow --with numpy python -c "
from PIL import Image
import numpy as np
rng = np.random.default_rng(0)
img = Image.fromarray(rng.integers(0, 255, (96, 96, 3), dtype=np.uint8))
img.save('$base/base.jpg', quality=85)
img.save('$base/base.png')
img.save('$base/base.webp', quality=80)
"; or begin
        echo "could not create base images (needs uv; Pillow and numpy are fetched on demand)" >&2
        exit 2
    end
end

# cases.json lines are accumulated here: name -> {status, digital_source_type, note}
set -g cases

# add <name> <container> <expected-status> <expected-code|-> <note> [exiftool args…]
function add
    set -l name $argv[1]
    set -l ext $argv[2]
    set -l status_ $argv[3]
    set -l code $argv[4]
    set -l note $argv[5]
    set -l args $argv[6..]
    set -l out $dest/$name.$ext
    exiftool -q -q -o $out $args $base/base.$ext; or begin
        echo "exiftool failed for $name" >&2
        exit 1
    end
    set -l codes '[]'
    if test "$code" != -
        set codes "[\"$code\"]"
    end
    set -a cases "\"$name.$ext\": {\"status\": \"$status_\", \"digital_source_type\": $codes, \"note\": \"$note\"}"
end

set -l dst -XMP-iptcExt:DigitalSourceType

# 1. Every vocabulary term as a URI, on JPEG. Generative -> present; the rest -> absent.
for t in trainedAlgorithmicMedia compositeWithTrainedAlgorithmicMedia
    add "term_$t" jpg present $t "generative term, URI form" $dst=$scheme/$t
end
for t in digitalCapture computationalCapture negativeFilm positiveFilm print humanEdits \
    compositeCapture algorithmicallyEnhanced dataDrivenMedia digitalCreation \
    virtualRecording compositeSynthetic algorithmicMedia screenCapture
    add "term_$t" jpg absent $t "non-generative term, URI form" $dst=$scheme/$t
end
for t in digitalArt minorHumanEdits
    add "term_$t" jpg absent $t "retired term; interpreted, flagged retired" $dst=$scheme/$t
end

# 2. Generative term on the other two containers.
add gen_png png present trainedAlgorithmicMedia "PNG iTXt XML:com.adobe.xmp" $dst=$scheme/trainedAlgorithmicMedia
add gen_webp webp present compositeWithTrainedAlgorithmicMedia "WebP XMP chunk" $dst=$scheme/compositeWithTrainedAlgorithmicMedia

# 3. Value spellings.
add bare_term jpg present trainedAlgorithmicMedia "bare term, no scheme URI" $dst=trainedAlgorithmicMedia
add https_scheme jpg present trainedAlgorithmicMedia "https scheme instead of http" $dst=https://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia
add trailing_slash jpg present trainedAlgorithmicMedia "trailing slash on the URI" $dst=$scheme/trainedAlgorithmicMedia/
add case_variant jpg inconclusive TrainedAlgorithmicMedia "capitalised term: not in vocabulary, verbatim" $dst=$scheme/TrainedAlgorithmicMedia
add unknown_term jpg inconclusive syntheticFoo "term outside the vocabulary" $dst=$scheme/syntheticFoo
add unknown_scheme jpg present trainedAlgorithmicMedia "foreign scheme, known term: last segment wins" $dst=http://example.org/vocab/trainedAlgorithmicMedia

# 4. Syntactic form: ExifTool default is element form; XMPShorthand writes attributes.
add form_attribute jpg present trainedAlgorithmicMedia "attribute (shorthand) form" -api XMPShorthand=1 $dst=$scheme/trainedAlgorithmicMedia

# 5. Packet size: >64 KiB forces ExtendedXMP on JPEG.
string repeat -n 70000 X >$dest/.pad.txt
add extended_xmp jpg present trainedAlgorithmicMedia "ExtendedXMP: field in the extension packet" "-XMP-dc:Description<=$dest/.pad.txt" $dst=$scheme/trainedAlgorithmicMedia
add extended_xmp_no_field jpg absent - "ExtendedXMP present, no field anywhere" "-XMP-dc:Description<=$dest/.pad.txt"
rm -f $dest/.pad.txt

# 6. XMP present without the field.
for ext in jpg png webp
    add xmp_no_field $ext absent - "XMP with CreatorTool only" -XMP-xmp:CreatorTool="Synthetic Writer 1.0"
end

# 7. IIM.
add iim_only jpg absent - "IIM OriginatingProgram, no XMP" -IPTC:OriginatingProgram=SynthCam -IPTC:ProgramVersion=2.1
add iim_and_dst jpg present trainedAlgorithmicMedia "IIM plus generative XMP" -IPTC:OriginatingProgram=SynthGen $dst=$scheme/trainedAlgorithmicMedia
add iim_and_capture jpg absent digitalCapture "IIM plus capture XMP" -IPTC:OriginatingProgram=SynthCam $dst=$scheme/digitalCapture

# 8. Plain copies as negatives.
for ext in jpg png webp
    add plain $ext absent - "no metadata at all"
end

echo "{" >$dest/cases.json
set -l n (count $cases)
for i in (seq $n)
    if test $i -lt $n
        echo "  $cases[$i]," >>$dest/cases.json
    else
        echo "  $cases[$i]" >>$dest/cases.json
    end
end
echo "}" >>$dest/cases.json
echo (count $cases)" files -> $dest (cases.json holds the intended verdicts)"
echo "next: uv run scripts/differential.py corpus/differential/03-synthetic"
