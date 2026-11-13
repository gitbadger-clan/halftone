# 05-fold — provenance

The Fold's own editing tools, applied to photos already in the corpus. One file
per tool. Files are not committed; this file and `expectations.json` are. Names
follow 07, with the app in place of the editor and the tool in place of the
operation: `<app>__<tool>__<route>__<subject>__<n>.<ext>`.

Collected 2026-11-13 with `scripts/htc.py` (`HT_STRATUM=05-fold`).

## Device

- Pixel 9 Pro Fold, Android 17.
- Google Photos 8.0.0.1011730683.
- Pixel Studio 7.67.0.882706237 on this device is an annotation and text tool, not
a prompt-driven generator (checked 2026-11-13). The p1–p3 generation rows planned
for it do not apply; the Gemini app on the Fold is collected in 04-generators as
gemini-app__nano-banana__*. One annotation row is collected here instead.

## Inputs

- `e1`: `00-base/photo_bowl.jpg`, the glass bowl of green apples, 3000×4000,
  Ultra HDR off, so **a single JPEG with no gain map**. On the phone it is
  `DCIM/Camera/PXL_20261008_132208267.RAW-01.jpg`, sha256 `69a6cd51…3404a4`,
  the same bytes as the corpus file (checked 2026-11-13). The copy on the
  device was later changed by an in-place Save and a revert
  (see Lineage); the corpus file in 00-base is unaffected. Identified by eye,
  confirmed by hash. Camera-signed 2026-10-08T13:22:09Z, computationalCapture.
- `s08`: `06-phones/pixel9fold__tele5x__adb-pull__s08__1.jpg`, taken for this
  stratum on 2026-11-13 at 5× (the telephoto lens, no digital zoom), location
  off. 3000×4000, 2,194,020 bytes, camera-signed 2026-11-13T08:10:19Z,
  computationalCapture, Ultra HDR with a 6,991-byte gain map. TODO: subject in
  a few words.

Every row was saved with **Save copy**, and each saved file was brought in with
`htc android grab`.

## Rows

| File | Tool | Size | Result |
|---|---|---|---|
| `google-photos__ai-enhance__save-copy__e1__1.jpg` | AI Enhance | 705,711 B | 3000×2250 |
| `google-photos__enhance__save-copy__e1__1.jpg` | Enhance | 1,743,306 B | 3000×4000 |
| `google-photos__crop__save-copy__e1__1.jpg` | Crop | 377,021 B | 1473×1965 |
| `google-photos__magic-eraser__save-copy__e1__1.jpg` | Magic Eraser | 1,133,269 B | 3000×4000, one apple erased |
| `google-photos__zoom-enhance__save-copy__s08__1.jpg` | Zoom Enhance | 397,031 B | 1500×2004 |

TODO: how much was cropped and from which edge; which region the Zoom Enhance
zoom covered.

## Findings

All five are signed by Google LLC through the Google C2PA SDK for Android,
trusted against the built-in list, content hash intact, each keeping its parent
in the chain. Google Photos signs what it writes and says which kind of edit it
was:

| Tool | Declared | Actions |
|---|---|---|
| AI Enhance | compositeWithTrainedAlgorithmicMedia, algorithmicallyEnhanced | c2pa.opened, c2pa.edited, c2pa.adjustedColor |
| Enhance | algorithmicallyEnhanced | c2pa.opened, c2pa.adjustedColor |
| Crop | humanEdits | c2pa.opened, c2pa.cropped |
| Magic Eraser | compositeWithTrainedAlgorithmicMedia | c2pa.opened, c2pa.deleted |
| Zoom Enhance | compositeWithTrainedAlgorithmicMedia | c2pa.opened, c2pa.edited |

- The three generative tools declare the generative term; the colour-only
  Enhance declares algorithmicallyEnhanced and the crop declares humanEdits.
  The editor draws the distinction the vocabulary is for, on one photo, in one
  app. AI Enhance declares both terms.
- Enhance and Crop also record that no manifest in the parent lineage declares
  a generative term, so a reader sees the whole chain.
- No file carries an IPTC DigitalSourceType marking or a vendor AI field in
  XMP: the declaration exists only inside the signed manifest. Any route that
  strips the manifest leaves no trace of the edit — see 10-messaging, where
  every compressed send does exactly that.
- Gain maps: the `e1` input has none (Ultra HDR off), yet AI Enhance writes one
  of 11,238 bytes with ISO 21496-1 metadata. Enhance, Crop and Magic Eraser
  write none. On the `s08` input, which has a 6,991-byte gain map, Zoom Enhance
  writes a new one of 4,138 bytes, also with ISO 21496-1 metadata the capture
  did not carry. So the two tools that resize the frame also rebuild the file
  as Ultra HDR, whether or not the input was.
- EXIF is rewritten rather than carried: all five have make 'Google' with the
  MakerNote gone and are libjpeg re-encodes at quality 90. In the three rows
  that change the frame, the EXIF still claims the input's 3000×4000.
- AI Enhance returns 3000×2250 from a 3000×4000 portrait, changing the aspect
  ratio. TODO: whether the tool crops by design.
- Double-quantization shows for AI Enhance (0.93) and Magic Eraser (0.84) but
  not for Enhance, Crop or Zoom Enhance.

## Lineage: two rows start from an extra step

`ht inspect --provenance` shows Crop and Magic Eraser with three records rather
than two. An in-place Save during collection (2026-11-13T10:07:33Z) added a
step to the input whose only action is c2pa.opened; Crop and Magic Eraser were
made from that file and carry it as their parent. Reverting in Google Photos
afterwards did not restore the original bytes (sha256 differs from
00-base/photo_bowl.jpg), so the capture-signed original no longer exists on the
device. The chains are:

- AI Enhance, Enhance: capture → this file.
- Crop, Magic Eraser: capture → re-encode (c2pa.opened only) → this file.
- Zoom Enhance: capture → this file.

The pixels of the Crop and Magic Eraser rows therefore passed through one more
JPEG re-encode than the other two. Their declarations are unaffected, and the
chain states it plainly, which is itself worth having in the corpus: an
accidental save is visible as a step, not as a gap.

## Not offered

In Google Photos 8.0.0.1011730683 on this device, 2026-11-13, the editor shows
Enhance, Dynamic and AI Enhance as its suggestions. Magic Editor "Reimagine"
and any text-prompt editing ("Help me edit") were looked for and not found, so
the edit prompts E1–E3 in `../PROMPTS.md` (object replacement, background
change, extend) could not be run with the phone's own tools. They are covered
for other tools in 04-generators.

## Open

- Pixel Studio, above.
- Dynamic (the third suggestion) was not collected.
- A plain crop of `s08` to the Zoom Enhance region, as a pixel-level control
  for the January work: not collected.
- The base photo on the device is no longer the capture-signed original, so
  later Fold edit rows need the input pushed fresh rather than taken from the
  camera roll.
- Google Photos' revert as its own row (`google-photos__revert__save__e1__1`):
  not collected.

## Results

Reference collection and differential run 2026-11-13 (ground truth
2026-11-13T10:20:03Z): 5 of 5 files compared, 0 disagreements; rights and
identifier values compared by keyed fingerprint; signals rubric compared on 12
nodes in 5 files, integrity and conformance rubrics on 185 checks in 5 files.
Whole corpus: 377 of 377 files across 9 corpora, 0 disagreements.
