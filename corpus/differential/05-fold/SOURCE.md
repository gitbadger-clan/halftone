# 05-fold — provenance

The Fold's own editing tools, applied to photos already in the corpus. One file
per tool, from two apps: Google Photos and Pixel Studio. Files are not
committed; this file and `expectations.json` are. Names follow 07, with the app
in place of the editor and the tool in place of the operation:
`<app>__<tool>__<route>__<subject>__<n>.<ext>`.

Collected 2026-11-13 with `scripts/htc.py` (`HT_STRATUM=05-fold`).

## Device

- Pixel 9 Pro Fold, Android 17.
- Google Photos 8.0.0.1011730683.
- Pixel Studio 7.67.0.882706237. On this device it is an annotation and text
  tool, not a prompt-driven generator (checked 2026-11-13), so the p1–p3
  generation rows once planned for it do not apply. The Gemini app on the Fold
  is collected in 04-generators as `gemini-app__nano-banana__*`.

## Inputs

- `e1`: `00-base/photo_bowl.jpg`, the glass bowl of green apples, 3000×4000,
  Ultra HDR off, so **a single JPEG with no gain map**. On the phone it is
  `DCIM/Camera/PXL_20261008_132208267.RAW-01.jpg`, sha256 `69a6cd51…3404a4`,
  the same bytes as the corpus file (checked 2026-11-13). Identified by eye,
  confirmed by hash. Camera-signed 2026-10-08T13:22:09Z, computationalCapture.
  The copy on the device was later changed by an in-place Save and a revert
  (see Lineage); the corpus file in 00-base is unaffected.
- `s08`: `06-phones/pixel9fold__tele5x__adb-pull__s08__1.jpg`, taken for this
  stratum on 2026-11-13 at 5× (the telephoto lens, no digital zoom), location
  off. 3000×4000, 2,194,020 bytes, camera-signed 2026-11-13T08:10:19Z,
  computationalCapture, Ultra HDR with a 6,991-byte gain map. Subject: a tiled
  roof.

The edit rows were saved with **Save copy**; the revert row is an in-place
save, which its `save` token records. Each file was brought in with
`htc android grab`.

## Rows

| File | Tool | Size | Result |
|---|---|---|---|
| `google-photos__ai-enhance__save-copy__e1__1.jpg` | AI Enhance | 705,711 B | 3000×2250, the middle band of the frame |
| `google-photos__enhance__save-copy__e1__1.jpg` | Enhance | 1,743,306 B | 3000×4000 |
| `google-photos__crop__save-copy__e1__1.jpg` | Crop | 377,021 B | 1473×1965, the middle of the frame |
| `google-photos__magic-eraser__save-copy__e1__1.jpg` | Magic Eraser | 1,133,269 B | 3000×4000, one apple erased |
| `google-photos__zoom-enhance__save-copy__s08__1.jpg` | Zoom Enhance | 397,031 B | 1500×2004 |
| `google-photos__revert__save__e1__1.jpg` | Revert to original | 1,595,806 B | 3000×4000. The device copy of `e1`, reverted after the in-place Save |
| `pixel-studio__annotate__app-save__e1__1.jpg` | Pixel Studio | 3,600,914 B | 3000×4000. Marks added over the middle of the picture, on the reverted copy of `e1` |

Where each edit sits in its input was measured afterwards by template matching
(OpenCV, on a quarter-size copy, so positions are good to about 8 pixels):

- Crop: a 1473×1965 region starting about 752 px from the left and 948 px from
  the top of the 3000×4000 input, at the input's own scale. All four edges are
  cut; about a quarter of the area is kept (match score 0.992).
- AI Enhance: the full width and rows 876 to 3126 of the input, at the input's
  own scale (0.986). See the findings.
- Zoom Enhance: a region about 390×520 starting near x = 1310, y = 188, in the
  upper middle of the 3000×4000 input, enlarged about 3.85× to 1500×2004
  (match score 0.936; searched on a half-size copy in steps of 30 px of
  width). The step is coarser than the answer: a 375×501 region at exactly 4×
  gives the same reading, and is the likelier one. Either way the output shows
  under 2% of the input's area, at about four times its linear size.

## Findings: the five Google Photos edits

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
- AI Enhance returns 3000×2250 from a 3000×4000 portrait: a plain crop to 4:3
  landscape, not a rescale. About 875 rows are cut from the top and as many
  from the bottom, so the crop is centred to within the measurement. That fits
  an automatic reframing better than a crop drawn by hand, but one file cannot
  show which it was.
- Double-quantization shows for AI Enhance (0.93) and Magic Eraser (0.84) but
  not for Enhance, Crop or Zoom Enhance.

## Findings: revert

Reverting in Google Photos gives back the original's lineage, not the original.

- The file is not the capture: sha256 `64f6fef9…d80ce2` against
  `69a6cd51…3404a4` for `00-base/photo_bowl.jpg`, and 1,595,806 bytes against
  2,088,972. It is a libjpeg re-encode at quality 90 with the MakerNote and the
  embedded thumbnail gone, and `jpeg_double` sees the second encoding (0.33).
- Its chain has two records: the camera capture (2026-10-08T13:22:09Z) and a
  new Google Photos step signed 2026-11-13T12:15:43Z whose only action is
  `c2pa.opened`. The accidental Save step of 10:07:33Z is not in it: the revert
  went back to the capture as its parent and dropped the step in between.
- So the edit history is undone and the pixels are re-encoded. The manifest is
  valid and trusted, and it says "opened", which is true as far as it goes.
- Conformance: the integrity rubric is met 6 of 6; the Conformance 0.2 /
  Spec 2.4 rubric 28 of 31. Not met: `all_actions_included_opened`,
  `mandatory_all_actions_included` and `mandatory_spec_version`. Credentio
  reads the same: the differential run below compares both rubrics check by
  check and finds no disagreement.
- Only the first of the three is the revert's own. The collected expectations
  show the other two not met by all five edit rows here as well, and by the
  `s08` capture in 06-phones: every manifest the Google C2PA SDK for Android
  wrote in these files lacks both, on one device and one SDK build.
  `mandatory_all_actions_included` is Conformance Program policy rather than a
  specification requirement (D-016 in `DIFFERENTIAL.md`).
- `all_actions_included_opened` is the one *shall* in C2PA 2.4 §18.15.3: a
  manifest that opens an asset and re-saves it with no other change, recording
  `c2pa.opened` alone, must set `allActionsIncluded` to true. None of the other
  six files here is that case, and none of the manifests examined in D-016 was;
  this one is recorded there as a follow-up. The accidental Save step in the
  Crop and Magic Eraser chains has the same action list, but as a parent, which
  the rubric does not examine.

## Findings: Pixel Studio

The annotated file has no C2PA manifest at all: the camera's signed record is
gone. What it carries is unsigned XMP.

- `marking_metadata`: IPTC DigitalSourceType =
  compositeWithTrainedAlgorithmicMedia.
- `rights_statements`: `photoshop:Credit` = "Edited with Google AI".
- `vendor_ai_flag` is `Absent`: it knows the credit "Made with Google AI", not
  this one.
- XMP `photoshop:DateCreated` is 2026-11-13T13:00:28Z, 45 minutes after the
  revert.
- The file is a libjpeg re-encode at quality 100, 3,600,914 bytes against the
  input's 2,088,972, at the input's 3000×4000; `jpeg_double` sees the second
  encoding (0.84). No identifiers, no gain map.

So two Google apps on one phone mark an edit in opposite ways. Google Photos
signs a manifest and writes no XMP marking; Pixel Studio drops the manifest and
writes only the XMP marking, which any re-save removes and nothing verifies.

What was added was not written down at the time. Compared pixel by pixel with
its candidates, the file is closer to the reverted copy than to the capture
(mean difference outside the marks 0.36 against 1.12), so Pixel Studio started
from the reverted file. 6.13% of the pixels differ, in one connected group
within x = 446 to 2498 and y = 880 to 3359, typically rgb(50, 106, 125). The
comparison cannot tell text from a drawn stroke. If the edit was annotation
only, the file declares a composite with generative AI, and credits Google AI,
for an edit that used none; that is not established here.

## Lineage: two rows start from an extra step

`ht inspect --provenance` shows Crop and Magic Eraser with three records rather
than two. An in-place Save during collection (2026-11-13T10:07:33Z) added a
step to the input whose only action is `c2pa.opened`; Crop and Magic Eraser were
made from that file and carry it as their parent. Reverting in Google Photos
afterwards removed that step from the chain but did not restore the original
bytes (see the revert findings), so the capture-signed original no longer
exists on the device. The chains are:

- AI Enhance, Enhance: capture → this file.
- Crop, Magic Eraser: capture → re-encode (c2pa.opened only) → this file.
- Zoom Enhance: capture → this file.
- Revert: capture → re-encode (c2pa.opened only) = this file.

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

- Dynamic (the third suggestion) was not collected.
- "Edited with Google AI" as a second phrase for `CREDIT_PHRASES` in
  `vendor_ai.rs`: measured here, a separate code change.
- Whether AI Enhance reframes every portrait to 4:3 landscape: needs a second
  photo.
- What exactly was added in Pixel Studio, and whether the same marking appears
  on an edit known to be annotation only: needs a second, documented run.
- The base photo on the device is no longer the capture-signed original, so
  later Fold edit rows need the input pushed fresh rather than taken from the
  camera roll.
- A plain crop of `s08` to the Zoom Enhance region, as a pixel-level control
  for the January work: not collected.
- During collection the base photos were pushed to `Pictures/ht-base/` and
  copied into `DCIM/Camera/`; both sets of copies have been removed
  (2026-11-16).

## Results

Differential run 2026-11-16 (ground truth collected 2026-11-16T11:31:49Z): 7 of
7 files compared, 0 disagreements; rights and identifier values compared by
keyed fingerprint; signals rubric compared on 14 nodes in 6 files, integrity
and conformance rubrics on 222 checks in 6 files (the six files that carry a
manifest: all but the Pixel Studio annotation). Whole corpus: 389 of 389 files
across 10 corpora, 0 disagreements.
