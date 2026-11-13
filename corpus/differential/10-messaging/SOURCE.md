# 10-messaging — provenance

Corpus files after one trip through a messaging app, one section per app. Files
are not committed; this file and `expectations.json` are. Names follow 07, with
the app in place of the editor and the send mode in place of the operation:
`<app>__<mode>__<input generator>-<input variant>__<subject>__<n>.<ext>`. The
extension is the one the app delivered, also where it differs from the input's.

Collected 2026-11-12 with `scripts/htc.py` (`HT_STRATUM=10-messaging`).

## Inputs

Five files, one per kind of marking. Each row below is the input's own state,
as `ht inspect` reads it.

| Token in the name | Input file | Carries |
|---|---|---|
| `gemini-app-nano-banana` p1 | `04-generators/gemini-app__nano-banana__web-download__p1__1.jpg` | C2PA manifest signed by Google LLC, trusted, trainedAlgorithmicMedia; no XMP |
| `chatgpt-base` p1 | `04-generators/chatgpt__base__web-download__p1__1.png` | C2PA manifest (`caBX`) signed by OpenAI OpCo, LLC, trusted, trainedAlgorithmicMedia; no XMP |
| `meta-ai-instant` p1 | `04-generators/meta-ai__instant__web-download-thumb__p1__1.jpg` | no manifest; XMP IPTC DigitalSourceType trainedAlgorithmicMedia (unsigned); 1 tracking record; progressive JPEG |
| `canva-canva-ai` p1 | `04-generators/canva__canva-ai__web-download__p1__2.png` | C2PA manifest signed by Canva, certificate expired 2026-09-17, no time-stamp; vendor flag `ContainsAiGeneratedContent = Yes`; 5 account or tracking records |
| `pixel9fold-default` s01 | `06-phones/pixel9fold__default__adb-pull__s01__1.jpg` | C2PA manifest from Google C2PA SDK for Android, trusted, computationalCapture; Ultra HDR gain map (container directory and MPF index) |

## Method

- Direction: every row was sent from the app's macOS desktop client to the
  account's own self-chat and pulled from the Pixel 9 Pro Fold over adb. The
  same direction for all three apps and all modes, with one exception: the
  Signal `file-android` row ran the other way (2026-11-13).
- Receiving on a second device is deliberate: the sending client can show its
  own cached original.
- Devices: macOS 27.2.1; Pixel 9 Pro Fold, Android 17.
- Modes: `document` / `file` = the app's send-as-file path; `photo` = its
  default compressed image path; `photo-hd` = its HD option, collected for the
  Gemini and Fold inputs only, in all three apps.
- Hash check of the file-mode rows against the inputs (sha256, 2026-11-12):
  WhatsApp `document` 5 of 5 and Telegram `file` 5 of 5 are byte-identical to
  their inputs. Signal `file` 0 of 5: see the Signal section.

## Summary

| Mode | WhatsApp | Telegram | Signal |
|---|---|---|---|
| file / document | byte-identical, 5 of 5 | byte-identical, 5 of 5 | sent as file from Desktop, delivered re-encoded as JPEG; no manifest, no XMP (5 of 5) |
| file, phone to Mac | not collected | not collected | re-encoded and scaled down, format kept; no manifest (5 of 5); gain map rebuilt on the Fold capture |
| photo | JPEG; no manifest, no XMP (5 of 5) | JPEG; no manifest, no XMP (5 of 5) | JPEG; no manifest, no XMP (5 of 5) |
| photo-hd | JPEG; no manifest, no XMP (2 of 2) | JPEG; no manifest, no XMP (2 of 2) | JPEG; no manifest, no XMP (2 of 2) |

Every compressed or HD send removed the C2PA manifest and all XMP, and with the
XMP the Meta DigitalSourceType marking and the Canva vendor flag. Both PNG
inputs came back as JPEG on every such route. Only a byte-identical file send
kept anything.

Delivered pixel sizes (inputs: Gemini 2752×1536, ChatGPT 1448×1086, Meta
1920×1280, Canva 1264×1264, Fold 3000×4000):

| Route | What it does to the frame |
|---|---|
| WhatsApp `photo` | longest side capped at 1600 (Gemini 1600×893, Fold 1200×1600); ChatGPT and Canva, already smaller, keep their size |
| WhatsApp `photo-hd` | full size kept (Gemini 2752×1536, Fold 3000×4000) |
| Telegram `photo` | longest side capped at 1280 (Gemini 1280×714, Fold 960×1280) |
| Telegram `photo-hd` | Gemini 2560×1429; the Fold file is 960×1280, the same as its standard photo, so HD changed nothing for it. TODO: confirm the HD option was used for that send |
| Signal `photo` and `file` | Gemini 2048×1143, Fold 1536×2048; ChatGPT, Meta and Canva keep their size |
| Signal `photo-hd` | larger: Gemini 2752×1536, Fold 2304×3072 |
| Signal `file-android` | Gemini 2048×1143, Fold 1536×2048, ChatGPT 1024×768; Meta and Canva unchanged |

WhatsApp HD is the one route that keeps every pixel and still removes the
manifest and the XMP: the image is intact, its provenance is not.

Signal Desktop's `file` and `photo` rows have the same dimensions and the same
file sizes for all five inputs, so choosing File appears to change nothing on
that client. TODO: confirm by sha256 and, if they match, drop one of the two
rows. TODO: whether the Fold gain map survives any route (`dev htc layout`).

## WhatsApp — desktop 26.44.74, Android 2.26.43.76

- Chat: "Message yourself".
- Media auto-download was on (default) and unchanged throughout. Small files
  were on the phone before the message was opened; larger ones were opened
  first. Both are the copy the server delivered.
- `document`: sent as a document. Pulled from
  `Android/media/com.whatsapp/WhatsApp/Media/WhatsApp Documents/Sent/`, where
  the app files a self-chat message from a linked device; the original file
  name is kept.
- `photo`: sent as a photo, standard quality. Pulled from TODO folder
  (expected `…/WhatsApp Images/Sent/`). TODO: menu path used on the phone
  ("download after opening the image in the chat").
- `photo-hd`: the HD button in the send preview. WhatsApp Desktop offers it.
- Finding (`document`): all five read as their inputs do. The three trusted
  manifests validate with the content hash intact; the Canva manifest's hash
  still matches (expired as on the input); the Meta XMP marking and tracking
  record, the Canva vendor flag and its five records, and the Fold gain map
  with both indexes are all present. All five are byte-identical to their
  inputs (sha256).
- Finding (`photo`): all five delivered as JPEG, the two PNG inputs included.
  No C2PA manifest and no XMP on any of them.
- Finding (`photo-hd`): no C2PA manifest and no XMP on either file; HD keeps
  nothing the standard photo loses. TODO: pixel size against the standard
  photo.
- Files (12): `document` ×5, `photo` ×5, `photo-hd` ×2.

## Telegram — Telegram.app (the native macOS client) 13.1.0, Android 13.2.1

- Chat: "Saved Messages".
- The send dialog offers three options for an image: with compression
  (default), without compression, and HD.
- adb can read Telegram's app folders on the Fold, so files were pulled from
  there without "Save to gallery" / "Save to Downloads".
- `file`: sent uncompressed, as a file; which of the dialog's options was used
  was not recorded. Each arrived as a document under its original file name.
  Pulled from
  `Android/data/org.telegram.messenger/files/Telegram/Telegram Files/`. Each
  file send also leaves a preview image in `…/cache/`, which is not collected.
- `photo`: with compression. Pulled from
  `Android/data/org.telegram.messenger/cache/`. TODO: where more than one
  size was cached, the largest was taken.
- `photo-hd`: the HD option.
- "Without compression": TODO. Either "same bytes as `file` (checked on TODO
  input), no row of its own" or its own mode with files listed below.
- Finding (`file`): all five byte-identical to their inputs (sha256), so they
  read exactly as the WhatsApp `document` rows do.
- Finding (`photo`): all five delivered as JPEG, the two PNG inputs included.
  No C2PA manifest and no XMP on any of them.
- Finding (`photo-hd`): no C2PA manifest and no XMP on either file. TODO:
  pixel size against the standard photo.
- Files (12): `file` ×5, `photo` ×5, `photo-hd` ×2.

## Signal — desktop 8.35.0, Android 8.34.2

- Chat: "Note to Self".
- Signal keeps attachments inside the app; each one was saved on the Fold
  before the pull. TODO: save menu path and the folder the file landed in.
- `file`: attached as a file in Signal Desktop. TODO: the exact control used,
  and how it differs from the `photo` send.
- `photo`: sent as an image from Signal Desktop.
- `photo-hd`: TODO where the quality choice is in Signal Desktop and what it
  is called.
- Finding (`file`): choosing to send an image as a file from Signal Desktop
  does not deliver the file. None of the five is byte-identical to its input
  (sha256); all five arrived as JPEG, the two PNG inputs included, with no
  C2PA manifest and no XMP. The names keep `file` because that is what was
  chosen; the result is a re-encoded image, as on the photo route. Where the
  re-encoding happens (Desktop before sending, or later) was not determined.
  TODO: whether these five are the same bytes as the `photo` rows.
- `file-android` (2026-11-13): the reverse direction. The five inputs were
  pushed to the Fold with `adb push` into `Download/ht-inputs/` and their
  sha256 checked against the inputs on the phone. Sent from Signal for Android
  with attach → File, saved from Signal Desktop into `~/Downloads`, brought in
  with `htc desk take`.
- Finding (`file-android`): all five were re-encoded and made smaller; none
  carries a C2PA manifest. The format is kept, unlike the Desktop `file` row:
  both PNG inputs arrived as PNG. Sizes in bytes, input → delivered:
  Canva 2,061,831 → 1,164,886; ChatGPT 2,101,196 → 1,050,317; Gemini
  2,888,720 → 202,343; Meta 164,373 → 153,185; Fold 2,497,313 → 324,957. The
  ChatGPT PNG went from 1448×1086 to 1024×768 pixels, so the image was scaled
  down, not only rewritten. XMP is gone from the Canva and Meta files, and
  with it the Canva vendor flag and the Meta DigitalSourceType marking.
- The Fold capture on this route is a new Ultra HDR JPEG: standard libjpeg
  tables at quality 75, a container directory and MPF index that agree with
  the file, and a 1,706-byte gain map (9,718 bytes on the input) that now
  also carries ISO 21496-1 metadata, which the input's gain map did not. Its
  XMP is that new container directory, not anything kept from the capture.
  A gain map can only have been rebuilt by an encoder that knows the format,
  which points at the sending phone; that is an inference, not a measurement.
- Finding (`file`, `file-android` together): 10 of 10 file sends through
  Signal were re-encoded, from either client, also when a file send was
  chosen. In these versions no route tried delivers an image file with its
  provenance data. TODO: how the message showed in the chat on each route.
- Finding (`photo`, `photo-hd`): all delivered as JPEG; no C2PA manifest and no
  XMP on any of them.
- Files (17): `file` ×5, `file-android` ×5, `photo` ×5, `photo-hd` ×2.

## Open

- All rows are self-chat: one account, its desktop client to its phone. Not
  yet checked: that a copy delivered to a different account is identical.
  Deferred because the received file could not be retrieved without a second
  transfer.
- iMessage and AirDrop: wait for the iPhone and iPad.
- Signal: a control with a non-image file (one input inside a zip), to show
  whether the change is specific to files Signal recognises as images.
- Pixel sizes of the `photo` and `photo-hd` rows, and the fate of the Fold
  gain map and of EXIF on those routes.
- A WebP input and a third signer (Adobe Firefly) were considered for a second
  pass and not sent.

## Results

Differential run 2026-11-13 (ground truth 2026-11-13T06:49:43Z): 41 of 41 files
compared, 0 disagreements; rights and identifier values compared by keyed
fingerprint; signals rubric compared on 10 nodes in 8 files, integrity and
conformance rubrics on 296 checks in 8 files (the eight files that still carry
a manifest: WhatsApp `document` and Telegram `file`, without the Meta input).
Whole corpus: 377 of 377 files across 9 corpora, 0 disagreements.
