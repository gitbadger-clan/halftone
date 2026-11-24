# 10-messaging — provenance

Corpus files after one trip through a messaging app, one section per app, and
since 2026-11-23 through AirDrop, which is a transfer between two devices and
not a messaging app. Files are not committed; this file and `expectations.json`
are. Names follow 07, with
the app in place of the editor and the send mode in place of the operation:
`<app>__<mode>__<input generator>-<input variant>__<subject>__<n>.<ext>`. The
extension is the one the app delivered, also where it differs from the input's.

Collected from 2026-11-12 with `scripts/htc.py` (`HT_STRATUM=10-messaging`).

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

- Direction: every messaging row was sent from the app's macOS desktop client
  to the account's own self-chat and pulled from the Pixel 9 Pro Fold over
  adb. The same direction for all three apps and all modes, with one
  exception: the Signal `file-android` row ran the other way (2026-11-13).
  The AirDrop rows go from the same Mac to an iPad; see that section.
- Receiving on a second device is deliberate: the sending client can show its
  own cached original.
- Devices: macOS 27.2.1; Pixel 9 Pro Fold, Android 17.
- Modes: `document` / `file` = the app's send-as-file path; `photo` = its
  default compressed image path; `photo-hd` = its HD option, collected for the
  Gemini and Fold inputs only, in all three apps.
- Hash check of the file-mode rows against the inputs (sha256, 2026-11-12):
  WhatsApp `document` 5 of 5 and Telegram `file` 5 of 5 are byte-identical to
  their inputs. Signal `file` 0 of 5: see the Signal section. AirDrop `file`
  5 of 5 (sha256, 2026-11-24).

## Summary

| Mode | WhatsApp | Telegram | Signal | AirDrop |
|---|---|---|---|---|
| file / document | byte-identical, 5 of 5 | byte-identical, 5 of 5 | sent as file from Desktop, delivered re-encoded as JPEG; no manifest, no XMP (5 of 5) | byte-identical, 5 of 5 (Mac to iPad) |
| file, phone to Mac | not collected | not collected | re-encoded and scaled down, format kept; no manifest (5 of 5); gain map rebuilt on the Fold capture | not collected |
| photo | JPEG; no manifest, no XMP (5 of 5) | JPEG; no manifest, no XMP (5 of 5) | JPEG; no manifest, no XMP (5 of 5) | no such mode |
| photo-hd | JPEG; no manifest, no XMP (2 of 2) | JPEG; no manifest, no XMP (2 of 2) | JPEG; no manifest, no XMP (2 of 2) | no such mode |

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
| Telegram `photo-hd` | Gemini 2560×1429; the Fold file is 960×1280, the pixel size of its standard photo, in different bytes (sha256, 2026-11-17). Whether the HD option took effect for that send was not recorded |
| Signal `photo` and `file` | Gemini 2048×1143, Fold 1536×2048; ChatGPT, Meta and Canva keep their size |
| Signal `photo-hd` | larger: Gemini 2752×1536, Fold 2304×3072 |
| Signal `file-android` | Gemini 2048×1143, Fold 1536×2048, ChatGPT 1024×768; Meta and Canva unchanged |

WhatsApp HD is the one route that keeps every pixel and still removes the
manifest and the XMP: the image is intact, its provenance is not.

Signal Desktop's `file` and `photo` rows are byte-identical for all five inputs
(sha256, 2026-11-17): choosing File changed nothing about what that client
delivered. Both sets of rows are kept, as the record of two sends.

EXIF and the gain map (`ht inspect`, 2026-11-17). Two inputs carry an EXIF
block, the Canva PNG and the Fold capture; the Fold capture alone carries a
gain map, read here as its container directory and MPF index.

| Route | EXIF | Gain map |
|---|---|---|
| WhatsApp `document`, Telegram `file`, AirDrop `file` (byte-identical) | kept on both | kept |
| every `photo` and `photo-hd` route, all three apps | gone from both | gone |
| Signal `file` from Desktop | gone from both | gone |
| Signal `file-android` | an EXIF block on the Fold capture, none on the Canva PNG | present, rebuilt (see Signal) |

So the gain map goes wherever the manifest goes, WhatsApp HD included. Whether
the EXIF block on the Signal `file-android` Fold file is the capture's own or
a new one was not compared.

## WhatsApp — desktop 26.44.74, Android 2.26.43.76

- Chat: "Message yourself".
- Media auto-download was on (default) and unchanged throughout. Small files
  were on the phone before the message was opened; larger ones were opened
  first. Both are the copy the server delivered.
- `document`: sent as a document. Pulled from
  `Android/media/com.whatsapp/WhatsApp/Media/WhatsApp Documents/Sent/`, where
  the app files a self-chat message from a linked device; the original file
  name is kept.
- `photo`: sent as a photo, standard quality, and saved on the phone after
  opening the image in the chat. The folder it was pulled from and the menu
  path were not recorded.
- `photo-hd`: the HD button in the send preview. WhatsApp Desktop offers it.
- Finding (`document`): all five read as their inputs do. The three trusted
  manifests validate with the content hash intact; the Canva manifest's hash
  still matches (expired as on the input); the Meta XMP marking and tracking
  record, the Canva vendor flag and its five records, and the Fold gain map
  with both indexes are all present. All five are byte-identical to their
  inputs (sha256).
- Finding (`photo`): all five delivered as JPEG, the two PNG inputs included.
  No C2PA manifest and no XMP on any of them.
- Finding (`photo-hd`): no C2PA manifest and no XMP on either file. HD keeps
  the full pixel size (Gemini 2752×1536, Fold 3000×4000, against 1600×893 and
  1200×1600 for the standard photo) and nothing else the standard photo
  loses: no EXIF, no gain map.
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
  `Android/data/org.telegram.messenger/cache/`. Whether more than one size was
  cached for a send, and which was taken, was not recorded.
- `photo-hd`: the HD option.
- "Without compression" has no row of its own: not collected as a separate
  mode.
- Finding (`file`): all five byte-identical to their inputs (sha256), so they
  read exactly as the WhatsApp `document` rows do.
- Finding (`photo`): all five delivered as JPEG, the two PNG inputs included.
  No C2PA manifest and no XMP on any of them.
- Finding (`photo-hd`): no C2PA manifest and no XMP on either file. Gemini is
  2560×1429 against 1280×714 for the standard photo; the Fold file is 960×1280
  on both, in different bytes.
- Files (12): `file` ×5, `photo` ×5, `photo-hd` ×2.

## Signal — desktop 8.35.0, Android 8.34.2

- Chat: "Note to Self".
- Signal keeps attachments inside the app; each one was saved on the Fold
  before the pull. The menu path and the folder the file landed in were not
  recorded.
- `file`: attached as a file in Signal Desktop. The control used was not
  recorded.
- `photo`: sent as an image from Signal Desktop.
- `photo-hd`: the higher-quality choice in Signal Desktop. Where it sits and
  what it is called were not recorded.
- Finding (`file`): choosing to send an image as a file from Signal Desktop
  does not deliver the file. None of the five is byte-identical to its input
  (sha256); all five arrived as JPEG, the two PNG inputs included, with no
  C2PA manifest and no XMP. The names keep `file` because that is what was
  chosen; the result is a re-encoded image, as on the photo route. Where the
  re-encoding happens (Desktop before sending, or later) was not determined.
  These five are the same bytes as the five `photo` rows (sha256, 2026-11-17).
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
  provenance data. How each message showed in the chat was not recorded.
- Finding (`photo`, `photo-hd`): all delivered as JPEG; no C2PA manifest and no
  XMP on any of them.
- Files (17): `file` ×5, `file-android` ×5, `photo` ×5, `photo-hd` ×2.

## AirDrop — macOS to iPadOS 18.7.3, 2026-11-23 and 2026-11-24

- Not a messaging app: the file goes from one device to the other with no
  chat, account or server in between. It is here because it is a common way
  for an image to change hands, and the question is the same.
- Direction: sent from Finder on the Mac (Share → AirDrop) to an iPad Pro
  12.9-inch, M1 (`iPad13,10`), accepted there and received into Photos. The
  Mac's macOS version was not noted for these sends.
- `file`: the only mode. Sharing a file from Finder offers no choice of
  quality.
- Pulled from the iPad's camera roll (`/DCIM/100APPLE/`) with `htc ios grab`
  over Wi-Fi; `htc` read each file twice and compared the two reads. The
  Gemini row was collected on 2026-11-23, the other four on 2026-11-24.
- So each file passed through one step more than the Fold rows did: Photos
  took it in and filed it before the pull.
- Photos files a received image under its own running name (`IMG_0027.JPG`
  for the Gemini row), so the original file name is gone while the bytes are
  not. The upper-case `.JPG` and `.PNG` in these five names come from there.
- Finding (`file`): all five byte-identical to their inputs (sha256,
  2026-11-24), the two PNG inputs as PNG. They read exactly as the WhatsApp
  `document` rows do: the three trusted manifests validate, the Canva
  manifest is expired as on the input, the Meta XMP marking, the Canva vendor
  flag and the Fold gain map are all present.
- What this measures is the copy in the camera roll. What Photos hands on
  when the image is shared again from the iPad, by AirDrop, Messages or its
  share sheet, is another route and was not measured.
- Files (5): `file` ×5.

## Open

- All rows are self-chat: one account, its desktop client to its phone. Not
  yet checked: that a copy delivered to a different account is identical.
  Deferred because the received file could not be retrieved without a second
  transfer.
- iMessage: not collected.
- AirDrop in the other direction (iPad to Mac), from Photos with its sharing
  options, and to or from an iPhone: not collected.
- Signal: a control with a non-image file (one input inside a zip), to show
  whether the change is specific to files Signal recognises as images.
- Telegram's "without compression" option as a row of its own.
- Whether the EXIF block on the Signal `file-android` Fold file is the
  capture's own.
- The details marked "not recorded" above: folders, menu paths and the names
  of the controls. A second pass should note them at the time.
- A WebP input and a third signer (Adobe Firefly) were considered for a second
  pass and not sent.

## Results

Differential run 2026-11-24 (ground truth collected 2026-11-24T13:22:36Z): 46
of 46 files compared, 0 disagreements; rights and identifier values compared by
keyed fingerprint; signals rubric compared on 15 nodes in 12 files, integrity
and conformance rubrics on 444 checks in 12 files (the twelve files that still
carry a manifest: WhatsApp `document`, Telegram `file` and AirDrop `file`,
without the Meta input). Whole corpus: 410 of 410 files across 10 corpora, 0
disagreements.
