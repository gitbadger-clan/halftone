# 04-generators — provenance

Files in this directory are not committed; this file, `expectations.json`,
`corpus-rules.json` and `PROMPTS.md` (one level up) are. Every generator gets a
section below with how the files were obtained. Write it while the UI is still
open: plan tier, defaults, and button labels change, and a row in the survival
table needs a date.

## Naming

```
<generator>__<variant>__<path>__<prompt>__<n>.<ext>
```

- `generator`: `zai`, `google-flow`, `bing-image`, `adobe-firefly`, `meta-ai`,
  `canva`, `ideogram`, `gemini-app`, `chatgpt`, `openai-api`, `gemini-api`,
  `aggregator-<site>`, `sd-a1111`, `comfyui`, …
- `variant`: model and settings, joined with `-`: `glm-wm-off`, `nano-banana-2`,
  `base` when there is nothing to say. A derived image (upscale, edit) gets its
  own variant (`-upscaled`, `-magic-edit`) and is listed under `derived` in
  `corpus-rules.json` when it must still show its source.
- `path`: how the bytes left the generator: `web-download`, `browser-save`,
  `copy-image`, `share-*`, `app-save-open`, `whatsapp-save`, `api-png` /
  `api-jpeg` / `api-webp`, `edit-photo`. Add `-thumb` / `-open` only when the
  same action exists in both contexts and behaves differently.
- `prompt`: `p1`–`p5`, `e1`–`e3` from `../PROMPTS.md`.
- `n`: generation number. It starts at the prompt number (`p2__2`); a further
  generation of the same prompt takes the next free number (`p2__3`). Files that
  share generator, variant, prompt and `n` must show the same image: the corpus
  test checks this.
- `ext`: whatever the path produced, even when it is wrong for the bytes. The
  mismatch is a finding; the collector sniffs the real type, and
  `corpus-rules.json` lists each known mismatch with its reason.

For `copy-image`, the receiving application is the writer: name it below.
Integrity: `cargo test --release -p halftone-cli --test corpus -- --ignored`
(names, formats, same-image groups, copies); `scripts/corpus-inspect.py` shows
the images behind any row it flags; `fish scripts/check-source.fish` checks this
file against the directory.

## Z.ai — GLM-Image (image.z.ai/create)

- Date: 2026-09-09. Plan: free. Default size 1K, 1:1.
- UI has a "No Watermark" checkbox, **checked by default** (`glm-wm-off` is the
  default state; `glm-wm-on` is the opt-in).
- Paths: download button (`web-download`), right-click Save Image on the page
  image (`browser-save`), right-click Copy Image pasted into Preview and saved as
  PNG (`copy-image`).
- Finding (wm-on): p1 (960×1728, visible "Z.ai" mark bottom right) and p2
  (1280×1280): the download button hands out the page's JPEG byte-for-byte, under
  a `.png` name. Browser-save copies were removed as identical (2026-10-02).
- Finding (wm-off): downloads p1–p5 are JPEG under `.png` names; p1 is 1280×1280,
  no visible mark.
- Withdrawn 2026-10-02: "the page image is a single watermarked JPEG served
  regardless of the toggle; the opt-out applies only to the download button".
  The two files it rested on (wm-off browser-save and copy-image p1) were
  byte-identical to their wm-on p1 counterparts and a different picture from the
  wm-off p1 download (aspect 0.556 vs 1.0): they were saved from the wm-on
  generation. Removed. No page image of a wm-off generation has been collected.
- Files (9): `glm-wm-off` p1–p5 download; `glm-wm-on` p1–p2 download and
  copy-image.
- 2026-10-20: attempted the page image of the wm-off p1 generation via
  copy-image and browser-save, from an open tab after the account was
  disabled the same day (reason not stated). Both came from a square p1
  generation not in the corpus (≤ 11 dB against every Z.ai download; no
  visible mark, toggle setting at its generation not recorded), so they
  don't answer the question; both removed.
- Not offered: API access on this tier, edit of an uploaded photo, upscale.

## Google Flow — Nano Banana (flow.google, formerly ImageFX)

- Date: 2026-09-09 (generations); 2026-10-14 (edits). Plan: free on
  2026-09-09 (Nano Banana 2 only); on 2026-10-14 the model picker offered
  Nano Banana 2.1, Pro and Lite on the Google AI Pro plan.
- ImageFX was retired 2026-04-30 and folded into Flow; `imagefx` is not a valid
  generator token in this corpus.
- Paths: the download button is on the thumbnail card (`web-download-thumb`); the
  page image can be saved and copied both from the thumbnail (`-thumb`) and from
  the opened full-size view (`-open`), and the two differ in size. `copy-image`
  pasted into Preview, saved as PNG.
  - Download from thumbnail and from opened view give byte-identical files (p2,
    2026-09-09; sha256 e89a209a…3f1c3ed8, read 2026-10-05 from the kept card
    file); opened-view copy not kept.
- Finding: the download button produces a JPEG; the page (both contexts) serves a
  PNG render. Which is the stored original: see `ImageSize` / `FileSize` in
  `expectations.json` and the hash comparison in the log.
- Variants: `nano-banana-2-1x1` = p3 requested at 1:1 (filed as `n` 1, before the
  `n` rule was written); the file is 1376×768, the same size as the default
  downloads, and a separate generation (20.6 dB against p3__3): the 1:1 request
  did not produce a 1:1 file, and whether the setting applied is not recorded;
  `nano-banana-2-upscaled` = Upscale of p2, listed as derived from the p2
  download (37.3 dB).
- Edits (E1–E3, 2026-10-14): base photo uploaded as a file from 00-base,
  prompt verbatim, downloaded from the opened edit view (Flow's download
  button, default size). Model `nano-banana-2.1` for E1–E3; E1 also on
  `nano-banana-pro` and `nano-banana-lite`, same manifest shape on all three,
  so E2–E3 were not repeated. For E2 and E3 Flow replied that the edit was not
  possible at the current aspect and proposed another; answered "yes" both
  times, and Flow changed the frame itself. E2's output kept the photo's 3:4
  (896×1200, as E1); E3's is 1376×768 (16:9). No XMP DigitalSourceType, no
  camera EXIF. Each chain contains the camera manifest of the uploaded base
  photo (E1 …4969aa2caf8f `photo_bowl`, E2 …4665cbaddaa9 `photo_flask`, E3
  …4c914b5479b0 `photo_path`), which confirms the input.
  Finding: Flow keeps the camera provenance chain, every manifest signed by
  Google LLC and trusted: camera capture (`c2pa.created`,
  `computationalCapture`, "Created by Pixel Camera.") → upload (`c2pa.opened`
  + `c2pa.transcoded`, `algorithmicallyEnhanced`, capture as `parentOf`) →
  generation (`c2pa.created`, `trainedAlgorithmicMedia`, "Created by Google
  Generative AI.", plus `c2pa.edited` "Applied imperceptible SynthID
  watermark.", upload as `inputTo`). The edit is recorded as a new creation
  that took the photo as input, not as an edit of a parent; SynthID is
  declared in the manifest. None of the five uses
  `compositeWithTrainedAlgorithmicMedia`. Contrast with Canva, which replaces
  the camera manifest (zero ingredients).
  E2's first download carries one more layer: `c2pa.opened` +
  `c2pa.transcoded` (`algorithmicallyEnhanced`) with the generation as
  `parentOf`, 4 manifests against 3. A second download of the same result
  (`edit-photo-redownload__e2__2`, 2026-10-14, same opened view, 896×1200)
  carries 3 manifests without the extra layer and differs in 0.63 % of pixels
  (PSNR 40.6 dB, differences spread across the whole image, consistent with
  one extra JPEG re-encode). Both share the generation and upload manifests,
  so the layer came with the first download, not with the edit; what differed
  between the two downloads is not known. The first download's active
  manifest therefore declares only `algorithmicallyEnhanced`, with the
  generative step one ingredient down (DIFFERENTIAL.md D-013).
- Files (17): p1 via browser-save-open, copy-image-open, copy-image-thumb,
  web-download-thumb; p2–p5 web-download-thumb; p3 1:1; p2 upscaled; edits
  E1–E3 on nano-banana-2.1, E1 on nano-banana-pro and nano-banana-lite; E2
  re-download.
- Control (`upload-unedited`, E1 slot, 2026-10-20): `photo_bowl` uploaded
  to Flow with no prompt and downloaded back unedited; 3000×4000 JPEG (the
  photo's own size; edits return 896×1200), `Trusted`. Two records, both
  signed by Google LLC: the camera's (`c2pa.created`, computationalCapture,
  "Created by Pixel Camera") as `parentOf` ingredient, and Flow's active
  record, `c2pa.opened` + `c2pa.transcoded` declaring algorithmicallyEnhanced.
  The upload step alone adds a record and an "enhanced" label, with no edit.

## Gemini app — Nano Banana (gemini.google.com)

- Date: 2026-10-20. Plan: Google AI Pro (the same subscription as Flow).
  Chat model 3.8 Flash and thinking Low (both defaults); the image tool shows
  "Nano Banana" without a version → variant `nano-banana`; "Redo with Pro"
  → `nano-banana-pro`. Each prompt in a new chat. Default output 2752×1536.
  The sidebar's Images entry: a separate creation view, not a gallery
  (checked 2026-10-22): "Create images" with the Images tool pre-selected
  in the prompt box, a model menu (Flash shown), an Aspect ratio control,
  and style templates (Bronze, Sommelier, Mural, Studio, Clay, …); the
  subtitle names Nano Banana.
- Paths: download button (`web-download`, JPEG); Gemini's copy button
  (`ui-copy`) and browser Copy Image (`copy-image`), both pasted into Preview
  and saved as PNG (Preview is the writer); right-click Save Image
  (`browser-save`, JPEG); Share → link opened in a private window, not logged
  in, the share page's download (`share-link-download`).
- Finding (downloads): C2PA manifest signed by Google LLC, trusted; no XMP
  DigitalSourceType: manifest only, as Flow web, unlike the API path and the
  Firefly Nano Banana partner row (D-006). Two records: the generation
  (`c2pa.created` + `c2pa.edited` "Applied imperceptible SynthID watermark.",
  trainedAlgorithmicMedia), and active `c2pa.opened` ("Opened by Google
  Generative AI.") + `c2pa.resized` ("Resized by Google Generative AI.",
  trainedAlgorithmicMedia) with the generation as `parentOf`: every download
  is a generative resize of the generation. Pro p1: same shape, 2816×1536.
  JPEG with libjpeg tables at Q100.
- Finding (page and clipboard): the page shows a 1024×572 preview;
  browser-save (JPEG, 93,788 B, non-libjpeg tables) and both copies carry no
  manifest and no XMP field.
- Edits (E1–E3): photo attached as a file from 00-base, prompt verbatim, new
  chat each, download button. E1/E2 1792×2390 (3:4 kept), E3 2528×1696 (3:2;
  Gemini chose the aspect itself). Three records: the
  Fold's camera capture (bowl …4969aa2caf8f, flask …4665cbaddaa9, path
  …4c914b5479b0) as `inputTo` of the generation (created + SynthID), then the
  resize as active. Same model as Flow (a new creation that took the photo as
  input), with a resize instead of a transcode on top. No camera EXIF.
- Finding (share link): E3 through the share link, opened in a private
  window: JPEG 1264×848, different bytes. The chain keeps the camera record
  and the same generation record (…4f5229595fd3), but the top layer is
  replaced: active `c2pa.opened` ("Opened by Google") + `c2pa.transcoded`,
  algorithmicallyEnhanced. The file a recipient gets has an active manifest
  that declares only a re-encode, with the generative step one record down:
  the D-013 shape on a sharing path. Halftone's lineage walk reports it as
  generative.
- Files (13): p1 via web-download, ui-copy, copy-image, browser-save; Pro p1
  web-download; p2–p5 web-download; E1–E3 edit-photo; E3 share-link-download.
- Pending: the Fold app row (`app-save-open`).


## Gemini API — Nano Banana 2 and Pro (generativelanguage.googleapis.com, `scripts/api-gen.py`)

- Date: 2026-10-21. Key from Google AI Studio, billing on. Image models
  listed for the key: gemini-2.5-flash-image, gemini-3-pro-image (+ preview),
  gemini-3.1-flash-image (+ preview), gemini-3.1-flash-lite-image. Rows use
  `gemini-3.1-flash-image` (Nano Banana 2); p1 also on `gemini-3-pro-image`.
  Request: `responseModalities: ["IMAGE"]`, nothing else. The first p1 by
  hand with curl (same request), the rest with `api-gen.py`; every call in
  `target/api-gen/log.jsonl`.
- Path: `api-jpeg`: the API returned image/jpeg for every call; no format was
  requested.
- Finding (generations): JPEG 1408×768, one record, Google LLC, trusted:
  `c2pa.created` (trainedAlgorithmicMedia, "Created by Google Generative
  AI.") + `c2pa.edited` ("Applied imperceptible SynthID watermark."). No
  resize layer: the app's `c2pa.resized` and its 2752×1536 come from the
  app's delivery, not the model. No XMP DigitalSourceType. 1408×768 is the
  size of Firefly's Nano Banana 2 partner row, consistent with Firefly
  calling this API.
- Edits (E1–E3): base photo bytes sent unchanged as inline data. Three
  records: the camera capture as `parentOf` of a Google upload record
  (`c2pa.opened` "Opened by Google" + `c2pa.transcoded`,
  algorithmicallyEnhanced), which is the `inputTo` of the generation
  (created + SynthID, active). The same chain as Flow's edits; the Gemini
  app instead puts the camera directly under the generation and adds a
  resize on top; the OpenAI API replaces the camera record. All three edits
  896×1195 (the photo's 3:4; E3 not widened). No XMP, no camera EXIF.
- Files (9): Flash p1–p5 and E1–E3; Pro p1.

## aggregator-zimage (third-party reseller UI)

- Date: 2026-09-09. Plan: free tier ("basic model only + Cloudflare verification +
  watermark"). Model selected: reseller's own "ImgFX"; also offers Nano Banana 2,
  Nano Banana Pro, Seedream 4.0, GPT Image 2 behind credits.
- Purpose in the corpus: the reseller-pipeline row. Any marking here is the
  reseller's, not the upstream model's; compare against the same model from its
  own provider.
- Paths: download button (`web-download`, PNG 1.6 MB), right-click Save on the
  page image (`browser-save`, WebP 91 KB — CDN preview), Copy Image into Preview
  (`copy-image`).
- Files (3): p1 via all three paths. p2–p5 not generated (credits).

## Bing Image Creator — MAI-Image-2.5-flash (bing.com/images/create/ai-image-generator)

- Date: 2026-09-09; copy-image 2026-10-01. Plan: free. Model as shown in the
  UI: MAI-Image-2.5-flash. Default aspect: 3:2 (1248×832); `-1x1` = p2 at 1:1
  (1024×1024).
- Paths: download button (`web-download`, JPEG); right-click Save Image
  (`browser-save`); right-click Copy Image into Preview (`copy-image`, PNG).
- Finding (routes): browser-save p1 is byte-identical to the download; the
  browser-save copy was removed (2026-10-02). Copy-image (PNG, 1248×832) carries
  no marking.
- Finding (credentials): C2PA manifest signed by Microsoft, claim v2, no
  declared spec version, generator "Microsoft 1.0" built with c2pa-rs 0.84.1;
  assertions c2pa.actions.v2, c2pa.hash.data, c2pa.soft-binding
  (`com.microsoft.invismark.1`, i.e. an invisible watermark Halftone cannot
  read). Under c2pa 0.91 the soft-binding assertion fails to decode (unit
  "percentage") → `claim.malformed`; all six downloads read `Invalid` in the
  2026-10-04 collection (c2pa 0.91.1). The signer carries only the MS C2PA
  Signing EKU, so it is outside the C2PA Trust List. Full analysis:
  DIFFERENTIAL.md D-011.
- Finding (expiry, 2026-10-04 collection): `signingCredential.expired` on p2,
  p3, p5 and the 1:1 p2; not on p1 and p4. One download session (signature
  times 2026-09-09, 19:24–19:37 UTC) was signed by at least two Microsoft leaf
  certificates, each valid for one year: p1's (serial 33000000…000096) from
  2025-10-09 to 2026-10-09, p2's (serial 33000000…00007D) from 2025-10-01 to
  2026-10-01. Both signatures carry a time inside their certificate's validity
  (p1 2026-09-09T19:24:33Z, p2 19:37:13Z), yet c2pa 0.91.1 reports p2 expired:
  with the vendored inputs, expiry is judged against the date of validation,
  not the signature time. Cause not established: a timestamp authority outside
  the vendored TSA list, or a time that is not a TSA timestamp.
- XMP DigitalSourceType: none on any Bing file (2026-10-04 collection).
- Files (8): p1 download; p1 copy-image as `__p1__2` (copied from a regenerated
  image, 12.1 dB against p1__1); `web-download__p1__2` (2026-10-20); p2–p5
  download; p2 at 1:1.
- Finding (expiry): `signingCredential.expired` on all six signed downloads
  since 2026-10-09. One download session (2026-09-09, 19:24–19:50 UTC) was
  signed by two Microsoft leaf certificates, each valid for one year: serial
  33000000…00007D (p2, p3, p5, 1:1 p2; notAfter 2026-10-01 17:43:59Z) and
  33000000…000096 (p1, p4; notAfter 2026-10-09 18:27:37Z). Every signature
  carries an RFC 3161 time-stamp inside its certificate's validity
  (`timeStamp.validated`, "Microsoft C2PA TimeStamping"), but that TSA is not
  on the vendored C2PA TSA list (22 certificates, none Microsoft's), so c2pa
  reports `timeStamp.untrusted` and judges expiry against the validation
  date: the …00007D files read `expired` from the 2026-10-04 collection, the
  …000096 files from the first collection after 2026-10-09, as predicted on
  2026-10-06. Content hash matches on all six; `claim.malformed` (soft-binding,
  D-011) unchanged. Unlike Canva (D-010), Bing time-stamps its signatures; the
  files read `expired` because of the TSA trust list, not a missing
  time-stamp. DIFFERENTIAL.md D-012.
- 2026-10-20: download of the Oct 1 p1 generation (`web-download__p1__2`),
  same generation as the copy-image row: pixel-identical (ImageMagick PSNR
  120 dB = no difference), so the clipboard copy keeps every pixel and drops
  the whole manifest. Signature time 2026-10-01T11:58:24Z, the generation, not
  the download: Bing stores the manifest at generation and serves it again
  (Canva signs at export). Third Microsoft leaf certificate, serial
  33000000D873…0000D8, valid 2026-09-17 to 2027-09-17; validation shows only
  `claim.malformed` (D-011), no `expired` until 2027-09-17 (D-012 logic, TSA
  still untrusted). Download file name is the prompt as a slug.

## Adobe Firefly — Image 5 and Image 4 Ultra (firefly.adobe.com)

- Date: 2026-09-09 to 2026-09-11 (Adobe models), 2026-10-01 (remote-manifest
  pass, partner models), 2026-10-02 (p1 variant 2, route check), 2026-10-20
  and 2026-10-22 (edits), 2026-10-22 (p1 generation 3, p1 at 2K). Plan: free
  tier, 4 generations per day, which is why prompts arrive over several days.
  Default aspect: 4:3 landscape as labelled in the UI; the files are 9:7
  (2304×1792, 1152×896). Default resolution 1K; `firefly-image-5-2k` = the 2K
  setting. Default model on this tier: Firefly Image 5; Image 4 Ultra
  (`firefly-image-4-ultra`) selectable, used as a second variant for p1–p2.
- Browser context actions are not available: right-click Save Image and Copy Image
  are disabled on the canvas. Recorded as `browser-save` / `copy-image`: not offered.
  Adobe funnels every exit through its own controls.
- Paths offered:
  - `web-download-thumb` / `web-download-open`: card and opened-view downloads of
    the same image have identical pixels and differ in 32 bytes, the UUID of the
    `dcterms:provenance` remote-manifest URN (p1 variant 2, 2026-10-02:
    urn-c2pa-c67e06a8… card, urn-c2pa-d7fe5531… opened view). The card returns the
    stored file with the same URN every time (upscaled p1: byte-identical on
    2026-09-09, 2026-10-01 and 2026-10-02; variant 2: twice on 2026-10-02). The
    opened view mints a new remote manifest per download. Context suffix restored
    for all Firefly Adobe-model downloads.
  - Explicit Upscale (variant `firefly-image-5-upscaled`): the UI's Upscale action
    on p1 variant 1. JPEG, 4608×3584, 2× the native size; the upscale path
    switches the output format from PNG to JPEG. The 2026-09-09 notes say it was
    downloaded from the opened view, but its bytes match the card download of
    2026-10-01/02; unresolved, filed as `-thumb` by its bytes. Listed as derived
    from p1 variant 1 (42.1 dB).
  - `share-copy`, `share-copy-thumb`: Adobe's own Share → Copy control (not the
    browser clipboard), from the opened view and from the card. Pasted into macOS
    Preview, saved as PNG at 2304×1792; the 5 MB PNGs are a clipboard bitmap
    re-encoded by Preview, so Preview is the writer of those files.
- Finding (formats): Image 5 downloads are PNG, Image 4 Ultra JPEG, format fixed
  per model. Image 5 p1 variant 1 (2026-09-09) is 2304×1792; every later Image 5
  download (p2 2026-09-10, p3–p5 2026-09-11, p1 variant 2 2026-10-02) is
  1152×896. Image 4 Ultra p1–p2 are 2304×1792. Upscale output is JPEG
  4608×3584, 2× variant 1. A fresh p1 on 2026-10-22
  (`web-download-thumb__p1__3`, card download, all defaults) is 1152×896 as
  well; the settings panel offers a resolution of 1K or 2K, default 1K.
  Variant 1's 2304×1792 is exactly twice 1152×896: a 2K generation, confirmed
  2026-10-22 (see below).
- Finding (credentials): Adobe-model downloads (Image 5 PNG, Image 4 Ultra JPEG)
  carry no embedded manifest. XMP holds only a remote-manifest reference at
  cai-manifests.adobe.com and no IPTC DigitalSourceType. Each card download of an
  image returns one stored manifest reference; each opened-view download creates
  a new one, so one image can have any number of valid manifests on Adobe's
  server. Fetched 2026-10-01, manifests are from Adobe Firefly, signed by Adobe
  Inc., cryptographically valid, content unchanged, trainedAlgorithmicMedia, but
  the signer does not chain to the vendored C2PA trust anchors (why: see
  2026-10-22 below).
  Offline, an Adobe-model file verifies nothing. The earlier expectation that
  Adobe's certificate is in the vendored trust list did not hold.
- Withdrawn 2026-10-02: a 2026-10-01 "redownload" of p1 was a copy of the upscaled
  file (identical bytes) and was removed; the "delivery pipeline changed between
  dates" reading came from comparing variant 1 with variant 2 and does not hold.
- Files (14): Image 5 p1 variant 1 card download and share-copy; p1 variant 2
  card and opened-view downloads (`p1__2`); p1 generation 3 card download
  (`p1__3`, 2026-10-22); p1 at 2K card download (`firefly-image-5-2k`,
  2026-10-22); p2–p5 card downloads; p1 upscaled card download; Image 4 Ultra
  p1–p2 card downloads, p1 share-copy-thumb.
- 2026-10-20 (remote manifests fetched deliberately, `edit-chain.fish
  --fetch`): native p1 variant 1 holds one Adobe record (`c2pa.created`,
  trainedAlgorithmicMedia, …97dc4f7afdb1). The upscale holds three: that same
  record unchanged, then `c2pa.opened` with it as `parentOf` (…03724826f46d),
  then the active `c2pa.created`, trainedAlgorithmicMedia, with the opened
  step as `inputTo` (…84e367de8456). So the upscale links to its exact source
  generation, and Adobe records it as a new AI creation from that input, not
  as a resize (no `c2pa.resized`). Same shape as the Image 4 Ultra edit.
- 2026-10-22: p1 at 2K (`firefly-image-5-2k__web-download-thumb__p1__1`) is
  2304×1792: the 2K setting gives variant 1's size, so variant 1 was a 2K
  generation. Adobe's signer, read from the remote manifest fetched
  2026-10-22: leaf "Adobe Firefly C2PA" (OU Content Authenticity Initiative,
  Adobe Inc., valid to 2027-08-20), issued by "Adobe Product Services G4",
  which chains to "Adobe Root CA G2" (to 2044-11-19). Adobe Root CA G2 is not
  in the vendored C2PA Trust List (2026-08-14 snapshot), so the chain fails
  there (`openssl verify`: unable to get local issuer certificate at depth 1),
  which is why Firefly records read as untrusted under that list. The list
  does hold an Adobe CA ("Adobe Product Issuing CA vault-a-or2.adobe.net
  cai", Adobe Inc), so Adobe has a C2PA-listed CA, but Firefly's remote
  manifests are signed through a different chain.
- Not offered: API (Firefly Services) not on this tier.

### Partner models (collected 2026-10-01)

- Picker lists Adobe models ("Commercially safe": Firefly Image 5, 4 Ultra, 4, 3)
  and partner models ("Models created by others"). Partner models offered:
  Gemini 3.1 (Nano Banana 2), GPT Image 2.5 Flare, GPT Image 2, FLUX.1 Kontext
  [max] (no crown); GPT Image 2.5 Sunburst, GPT Image 1.5, Gemini 3 (Nano Banana
  Pro), FLUX.2 [pro], GPT Image 1, FLUX1.1 [pro] Ultra Raw (premium).
- Paths: generation view → Download, named `web-download`. Browser saves as
  `Firefly_<model label>_<prompt prefix> <number>.png`.
- `gpt-image-2.5-flare` p1: PNG, 1,975,956 B. Active C2PA manifest from
  "OpenAI Media Service API", signed by OpenAI OpCo, LLC; chains to a trusted
  anchor; content hash matches; digitalSourceType trainedAlgorithmicMedia.
  Bare IHDR/IDAT/IEND + caBX chunk layout. No XMP. The signer's EKUs
  (c2pa-kp-claimSigning, id-kp-documentSigning) are accepted only with the
  vendored EKU policy (DIFFERENTIAL.md D-011, 2026-10-02).
- `nano-banana-2` p1 (UI label "Gemini 3.1 (Nano Banana 2)"; download names it
  "Gemini Flash"): PNG, 1,803,493 B. Active C2PA manifest from "Google C2PA Core
  Generator Library", signed by Google LLC; trusted anchor; hash matches;
  trainedAlgorithmicMedia. XMP DigitalSourceType trainedAlgorithmicMedia also
  present (PNG with text chunks).
- Finding: Firefly delivers partner outputs with the vendor's own manifest
  active and the content hash intact: Adobe neither re-encodes nor re-signs.
  c2patool: no Adobe manifest anywhere in the store; issuers are OpenAI OpCo,
  LLC and Google LLC only. Both files match the vendors' API-path pattern (the
  Google file carries manifest + XMP like the aggregator row in D-006),
  consistent with Firefly calling the vendor APIs and passing results through.
  Only the download file name identifies Firefly. The earlier expectation that a
  partner p1 "would show Adobe signing another provider's output" did not hold.
  Contrast: offline, a partner image verifies against trusted anchors; an
  Adobe-model image does not.
- Files (2): `gpt-image-2.5-flare` p1, `nano-banana-2` p1 `web-download`.

### Edits (2026-10-20 and 2026-10-22, free tier)

- Base photo uploaded as a file from 00-base, prompt verbatim, a single result
  each, Firefly's Download button. Image → Edit image → Fill, or Expand
  background → Expand.
  - Fill, default model Firefly Image 5 (`firefly-fill`): E1 (apples brushed), E2
    (work surface and wall brushed, flask left out). PNG 880×1184, 3:4 kept,
    camera EXIF kept (Make Google).
  - Fill with Firefly Image 4 Ultra (`firefly-image-4-ultra-fill`): E1. JPEG
    2304×1792, landscape from a portrait photo, no camera EXIF.
  - Expand (`firefly-expand`): E3, `photo_path.jpg` uploaded from disk,
    16:9, photo centred, no prompt field shown, a single result, 2026-10-22.
    JPEG 7111×4000 (full height, widened to 16:9), camera EXIF dropped.
    Remote manifest only (urn-c2pa-a265c0ac…), fetched 2026-10-22 with
    `edit-chain.fish --fetch`: two Adobe records: `c2pa.opened` +
    `c2pa.edited` (compositeWithTrainedAlgorithmicMedia) with a `parentOf`
    and an `inputTo` ingredient that carry no manifest (both untitled), then
    an active `c2pa.opened` with that record as `parentOf`. The camera record
    is not in the chain: unlike Fill, Expand names its inputs but does not
    carry their provenance. The active record declares no source type; the
    generative step is one record down (the D-013 shape).
- Finding: no edit carries an embedded manifest. XMP `dcterms:provenance`
  points to a remote manifest at cai-manifests.adobe.com. Fetched 2026-10-20
  (unintentionally, by c2patool's default remote fetch; `edit-chain.fish`
  now runs offline unless `--fetch`):
  - Fill: two records. The Fold's camera record (Google LLC,
    `computationalCapture`, "Created by Pixel Camera.") as `parentOf`, and
    Adobe's (Adobe Inc.) with `c2pa.opened` + `c2pa.edited`,
    `compositeWithTrainedAlgorithmicMedia`: the edit recorded as an edit of
    the camera photo, the C2PA-shaped case.
  - Image 4 Ultra: `c2pa.created`, `trainedAlgorithmicMedia`, the camera
    record as `inputTo`: a new creation that took the photo as input, like
    Flow.
  Offline none of it verifies: Halftone reports a remote reference, not
  fetched (D-009).
- Files (4): Fill E1, E2; Image 4 Ultra Fill E1; Expand E3.

## ChatGPT — website (chatgpt.com)

- Date: 2026-10-21. Plan: Pro. Prompts with the "Create image" tool, each
  in a new chat; thinking effort Medium (the default), unchanged. The UI names
  no image model (the editor's menu shows "Image") → variant `base`. The
  manifest's softwareAgent is a generator-info map {"name": "ChatGPT",
  "version": "gpt-image"}; the API rows carry the string "API
  chatgpt-image-latest" instead.
- Paths: download icon in the opened view (`web-download`, PNG, named after
  ChatGPT's own title for the image); browser Copy Image pasted into Preview
  (`copy-image`); ChatGPT's copy button, right-click Save Image, and the
  image's own share icon → link → private window → download were also taken
  (see below).
- Finding (downloads): one manifest from "OpenAI Media Service API", signed by
  OpenAI OpCo, LLC, trusted: `c2pa.created` (trainedAlgorithmicMedia) +
  `c2pa.converted` + `c2pa.watermarked.unbound`. The third action is absent
  from all 11 API files: the website declares a watermark the API does not.
  No soft-binding assertion (`c2pa.actions.v2` is the only assertion), so the
  watermark is declared but not tied to the record, as "unbound" says. No
  XMP. PNG with a bare IHDR/IDAT/IEND layout plus caBX, like the API. Sizes
  differ from the API's: 1448×1086 (p1, p2, p4), 1278×1230 (p3), 1254×1254
  (p5). The same two alternating certificates as the API (expiring 2027-03-24
  and 2027-04-23, TSA not on the vendored list).
- Finding (page, links, clipboard): right-click Save Image on p1 gave the
  download's bytes exactly, and the share link of E3 served the E3
  download's bytes exactly: unlike Flow and Gemini, the page and the share
  link serve the signed file itself. ChatGPT's copy button and the browser's
  Copy Image gave byte-identical PNGs via Preview, with no manifest and no
  XMP field: the clipboard drops the record. The three duplicates
  (browser-save, share-link, ui-copy) were removed on 2026-10-21; one file
  per distinct output kept.
- Edits (E1–E3): photo attached as a file, prompt verbatim, new chat each.
  One OpenAI record each (created + converted + watermarked.unbound), zero
  ingredients; camera-record IDs and camera strings absent from the bytes; no
  camera EXIF. The camera record is replaced, as with the API. E1 and E2
  1086×1448 (3:4 kept), E3 1764×891 (widened to about 2:1; the API kept 3:2).
- Remove BG on p3 (`base-bg-removed`): one click, no prompt. 1279×1230,
  signed, trusted, a fresh record (created + converted + watermarked.unbound)
  with no ingredient: the derived image does not link back to p3.
- Files (10): p1–p5 download; p1 copy-image; E1–E3; p3 bg-removed.

## OpenAI API — GPT Image (api.openai.com, `scripts/api-gen.py`)

- Date: 2026-10-21. Paid API key (project key). Models listed for the key:
  chatgpt-image-latest, gpt-image-1, -1-mini, -1.5, -2 (+ 2026-04-21),
  -2.5-flare and -2.5-sunburst (+ 2026-09-08 snapshots). Rows use
  `chatgpt-image-latest` (ChatGPT's model); the transparent p3 uses
  `gpt-image-1.5` (gpt-image-2 does not support transparency). Defaults
  everywhere: no size, quality or compression sent. Every call in
  `target/api-gen/log.jsonl` (request id, duration, response metadata).
- Paths: `api-png` / `api-jpeg` / `api-webp` = the `output_format` requested;
  the file is the base64 payload decoded, unmodified. Each call is its own
  generation, so p1 in three formats is p1__1 (png), p1__2 (jpeg),
  p1__3 (webp).
- Finding (formats, closes D-001): PNG, JPEG and WebP each carry the same
  signed record: one manifest from "OpenAI Media Service API", signed by
  OpenAI OpCo, LLC, trusted, content hash intact, `c2pa.created`
  (trainedAlgorithmicMedia, softwareAgent "API chatgpt-image-latest") + `c2pa.converted`.
  No XMP DigitalSourceType on any format. The delivery format does not
  decide the marking on the API path.
- Finding (transparent p3): 1024×1536 PNG with alpha, signed the same way.
  Unlike Ideogram's no-background PNG, the alpha path does not drop the
  record.
- Finding (certificates): two OpenAI signing certificates alternate within
  one session (not_after 2027-03-24T02:53:02Z and 2027-04-23T15:51:04Z).
  Signatures carry RFC 3161 time-stamps from a TSA outside the vendored list,
  so under this list the files read `expired` after those dates (the D-012
  case). A stapled OCSP response says the certificate was not revoked: the
  first generator in the corpus with one.
- Edits (E1–E3, `/images/edits`, base photo bytes uploaded unchanged):
  one record each, OpenAI's (`created`, trainedAlgorithmicMedia, +
  `converted`), zero ingredients; the camera-record IDs and camera strings
  are absent from the file bytes; no XMP, no camera EXIF. The camera record
  is replaced and the edit is labelled a generation, as with Ideogram.
  Frames are the API's own: E1 1024×1024 (square from a 3:4 photo), E2
  1024×1536, E3 1536×1024.
- Files (11): `chatgpt-image-latest` p1 png/jpeg/webp, p2–p5 png, E1–E3 png;
  `gpt-image-1.5-transparent` p3 png.

## Meta AI — meta.ai (web), modes "instant" and "thinking"

- Date: 2026-09-10. Plan: free (Meta account). Default mode: instant;
  the other mode used for one p1 as a second variant. Default aspect: 3:2
  landscape (1920×1280, read from the download; the UI does not state it).
- Controls and the paths they map to:
  - `web-download-thumb` / `web-download-open`: download control on the result
    card and in the opened view. JPEG, 1920×1280.
  - `share-download-thumb` / `share-download-open`: Share menu → Download, both
    contexts. JPEG.
  - `browser-save-thumb` / `browser-save-open`: right-click Save Image on the page
    image. WebP, 1920×1280.
  - `share-browser-save`: Share menu → opens the image in a new tab → browser
    Save Image. WebP.
  - `copy-image-thumb` / `copy-image-open`: right-click Copy Image, pasted into
    macOS Preview, saved as PNG (Preview is the writer).
  - `share-instagram-browser-save`: Share menu → Instagram → the image as Instagram
    served it, saved from the browser. 27 KB JPEG, cropped to 1.91:1
    (`known_different` in corpus-rules). First platform-served file in the
    corpus; a stratum-10 row kept here for now.
- Finding (hash and size check): the four download JPEGs (`web-download-thumb/
  -open`, `share-download-thumb/-open`) are byte-identical; the two WebPs
  (`browser-save-open`, `share-browser-save`) are byte-identical. Meta serves one
  JPEG through every download control and one lossy WebP render of the same
  1920×1280 pixels to the page — re-encoded, not downscaled. Duplicates removed
  2026-10-02; one file per distinct output kept.
- Finding (marking): downloads carry XMP DigitalSourceType
  trainedAlgorithmicMedia and no C2PA manifest. The page WebPs (browser-save,
  both modes) keep the field too; the copy-image PNGs (Preview as writer) lose
  it. The Instagram-served thumbnail (27 KB, cropped, re-encoded) kept it.
- Files (11): instant p1 web-download-thumb, browser-save-open, copy-image-open,
  share-instagram-browser-save; instant p2–p5 web-download-thumb; thinking p1
  web-download-thumb, browser-save-thumb, copy-image-thumb.
- Edits (E1–E3, 2026-10-20, free, mode instant): base photo attached as a
  file from 00-base, prompt verbatim, Meta's download control. E3: Meta
  extended to 16:9 by itself.
  JPEG, progressive; E1/E2 1376×1824 (3:4 kept), E3 2048×1152.
  Finding: the Fold's signed record is dropped; no C2PA manifest of Meta's own;
  XMP DigitalSourceType trainedAlgorithmicMedia, the same unsigned label Meta
  puts on its generations, so the edit declares itself a generation; camera
  EXIF gone; no camera-record ID or camera string in the file bytes.
  The download name carries the uploaded file's name and the prompt
  (`photo_bowl-replace-the-green-apples-…-k.jpg`) for E1 and E2; E3 came as
  `image_20261020_072046.jpg`.
- Files (3): E1, E2, E3 edit downloads (`instant`, 2026-10-20).
- See also: the Android app and WhatsApp sections below.

## Meta AI — Android app (com.facebook.stella)

- Date: 2026-09-30 to 2026-10-01. Device: Pixel 5, Android 14 (no SIM); package
  com.facebook.stella 290.1.0.42.163; account tier free. Default mode in the app:
  instant (label shown: instant).
- Transfer: `adb pull` from shared storage, sha256 checked on device and host.
- Controls offered in the opened image: save (download icon), Share, Copy.
  - `app-save-open`: the save control. Lands in `Download/` under a CDN-style name
    (`<digits>_n.webp`) for p1–p4; p5 and thinking p1 arrived with `.jpg` names.
  - `share`: Android system share sheet; the app shares text/plain (an
    89-character link), not the image. Files by Google receives text and saves
    nothing. No file. The link leads to the web share page, covered by the web
    section's `share-*` rows.
  - `copy-image`: Copy puts an image on the clipboard (the app's own input
    rejects the paste as an image; `dumpsys clipboard` is not readable by the
    shell on Android 14). Not collected: no byte-exact way to read it, and any
    paste target would be the writer.
- Finding: every save is a JPEG (p1: 161,244 B, progressive, 4:2:0, optimised
  Huffman), p1–p4 under `.webp` names (`extension_mismatch` in corpus-rules); XMP
  DigitalSourceType trainedAlgorithmicMedia present; no C2PA manifest;
  quantization tables identical across prompts. Same marking as the web
  downloads. `jpeg_double` is Inconclusive because Halftone does not yet read
  progressive JPEGs. Only the save control produces a file.
- These are separate generations from the web rows (11–18 dB against them), so
  they carry the next free `n`.
- Files (6): instant p1__2, p2__3, p3__4, p4__5, p5__6 `app-save-open`;
  thinking p1__2 `app-save-open`. p3–p5 collected for completeness, though the
  rule only requires them when the app behaves differently from the web.

## Meta AI — inside WhatsApp (Android, Pixel 9 Pro Fold)

- Date: 2026-10-01. Device: Pixel 9 Pro Fold, Android 17. WhatsApp 2.26.37.73.
  Region: CH. adb over wireless debugging.
- Settings: Enter is send off; Media visibility on; auto-download Wi-Fi all
  media, mobile data photos, roaming none; auto-download quality Auto;
  media upload quality Standard.
- Image generation: offered. Variant `base`.
- Paths:
  - `whatsapp-autosave`: not produced. Despite media visibility on and Wi-Fi
    auto-download "All media", the Meta AI image was not written to shared
    storage on arrival.
  - `whatsapp-save`: open image → ⋮ → Save. Lands as
    `WhatsApp Images/IMG-20261001-WA8214.jpg`.
  - `whatsapp-share-files`: open image → Share → Files by Google → Save to
    Downloads. Lands as `Download/image.jpg` (generic name). Byte-identical to
    `whatsapp-save` (sha256 c389b240…276155e); file not kept.
- Finding: p1 JPEG, 1,030,259 B, standard libjpeg tables at Q100, 4:2:0,
  baseline. No XMP DigitalSourceType and no C2PA: the trainedAlgorithmicMedia
  field that every web and app save carries is gone. Both file routes deliver
  the same bytes, so the re-encode happens before either (bot send pipeline or
  WhatsApp receive/download; not distinguishable here). Auto-save never wrote the
  image despite the settings. p5: JPEG, 387,197 B, 1920×1280, standard libjpeg
  tables at Q100, 4:2:0, baseline, optimised Huffman; no XMP DigitalSourceType
  and no C2PA, same as p1. Invisible watermarking, if Meta applies any, is not
  readable by Halftone and untested here.
- Files (2): base p1, p5 `whatsapp-save`.

## Canva — Canva AI image generator (canva.com)

- Date: 2026-09-10 to 2026-10-20. Plan: Pro trial from 2026-09-10 (one
  free-tier file predates it, see below), cancelled 2026-10-10 09:43;
  account on Free from 09:43 2026-10-10.
  Model as shown in the UI: not stated → variant `canva-ai`. Default aspect: 1:1.
- Purpose in the corpus: agency tooling. Canva is a C2PA member; whether its
  export writes a manifest, and whether its own edits keep one, is the row.
- Paths: all downloads taken from the opened preview (`web-download`). The
  download dialog offers PNG (default) and JPG (`web-download-jpg`), plus a
  "larger size" option (variant `canva-ai-larger`, an export-time upscale). The
  editor-canvas export after "Edit image" is `web-download-editor`; edit rows
  are exported from the editor via Share → Download (PNG, 1×). The tier at
  export goes in the variant (`canva-ai-free`). The result-card download was
  not exercised. Right-click Save / Copy on the canvas: not available.
- Finding: C2PA manifest signed by Canva (c2pa-rs 0.89.3), built at export,
  not at generation (see the p2 re-download under Tier). Date-dependent:
  `Valid` on 2026-09-10 (signer not on the vendored 2026-08-14 list); `Invalid`
  since the signing certificate expired on 2026-09-17 (first observed
  2026-09-20, unchanged in the 2026-10-04 collection), because the signature
  carries no time-stamp (no `sigTst`/`sigTst2` token in the COSE header, no
  `timeStamp.*` code in `validation_results`), so a verifier judges the
  certificate at the time it checks (DIFFERENTIAL.md D-010). XMP: no IPTC field in
  the September downloads; downloads from October on carry
  XMP-iptcExt:DigitalSourceType = compositeWithTrainedAlgorithmicMedia, on
  plain generations and edits alike, so Canva added the field between
  September and October.
- 2026-10-03: Manifest shape confirmed on all 11 signed Canva files (plain p1–p5,
  editor, JPG, larger, bg-removed, bg-removed-erase, magic-edit) with
  `c2patool --detailed`: one `c2pa.actions.v2` with one `c2pa.created`,
  `compositeWithTrainedAlgorithmicMedia`, no parameters, zero ingredients,
  `allActionsIncluded` omitted. The free-tier p1 carries no claim. Structurally
  conformant with C2PA 2.4 §18.15.2: `c2pa.created` first and a
  `digitalSourceType` recorded; ingredients are required only for
  `c2pa.opened`/`c2pa.placed` (§18.15.4.7) and optional for `c2pa.created`
  (§18.15.4.5). The term follows CAI's authoring docs; the spec's own example for
  text-to-media generation uses `trainedAlgorithmicMedia`, and "appropriate value"
  is neither defined nor validated. The magic-edit inpaint matches IPTC's
  definition of the term; the plain generations do not, and the two are
  indistinguishable in the manifest. Edits are not recorded as actions; with
  `allActionsIncluded` omitted the manifest does not claim to be complete
  (§18.15.3: unrecorded actions may have been performed). `softwareAgent` is the
  plain string "Canva AI"; §18.15.4.4 describes a generator-info-map for v2
  actions, and c2pa-rs accepts both. Closes the magic-edit pending item.
- Edits (E1–E3, 2026-10-09, Pro trial), three routes, all with the base
  photo uploaded as a file from 00-base:
  - Canva AI chat (`canva-ai`): prompt verbatim; the chat chose the
    operation and names no tool.
  - Magic Edit (`canva-ai-magic-edit`): E1 (page 1200×1600 and native
    3000×4000), E2. E3 not possible: Magic Edit works inside the frame and
    returned the image unchanged; not filed.
  - Magic Expand (`canva-ai-magic-expand`): E3, photo centred on a wider
    page and expanded to its edges. Deviation: Expand takes no text prompt.
  Finding, identical across all three routes: the camera's C2PA manifest is
  replaced by Canva's (issuer Canva) with zero ingredients, so the camera
  original is not recorded; `compositeWithTrainedAlgorithmicMedia` in both
  the manifest and XMP. Camera EXIF (make, model, capture time) not carried
  over. Valid when collected, `signingCredential.untrusted` only; two new
  signing certificates, issued 104 s apart on 2026-10-07, each valid 8 days:
  the chat route on serial 167403…226101 (hex 1D529F…C0CEF5, not_after
  2026-10-15T23:53:55Z), Magic Edit and Magic Expand on 600751…481979 (hex
  693A9E…C71ABB, not_after 2026-10-15T23:52:11Z); September files
  129430…675375 (hex 615F57…21862F). No time-stamp on either (D-010;
  `scripts/cert-shelf-life.sh`, 2026-10-12), so expected to turn `Invalid`
  like the September rows. Serials: decimal as c2pa prints them, hex as
  `openssl x509 -serial` prints them.
- Tier: `canva-ai-free` p1 was exported on the free tier before the trial and
  carries no manifest and no XMP; every Pro-tier export carries the manifest.
  2026-10-04: the corpus test found the free-tier p1 and the Pro editor export
  (`web-download-editor__p1__1`) pixel-identical (AE 0, full resolution, both
  1200×1200): same generation, manifest only on the Pro export. Listed under
  `known_duplicates` in corpus-rules.
  2026-10-10, the control: p2 re-downloaded on Free (11:03, after the
  cancellation) from the same Pro-era design
  (`canva-ai-free__web-download__p2__2`). It carries a fresh Canva manifest,
  valid when collected (`signingCredential.untrusted` only, certificate
  167403…226101, the same as the chat-route edits, not Magic Edit or
  Expand), plus the XMP field. So
  Canva signs at export, and the export tier does not decide marking now;
  the September downloads from this design are `Invalid` because their
  certificate expired. Pixels: not the September plain p2 (PSNR 13.0 dB,
  16.8 % of pixels differing, nearly all by more than 1 %); against the
  September Magic Edit of p2 (`canva-ai-magic-edit__web-download__p2__2`),
  PSNR 13.11 dB. The re-download matches neither September file from this
  design, so the export is a different image from anything downloaded in
  September; why (the design changed after September, or Canva re-rendered
  on export) is not known. It shows that a Free export is signed and carries
  the XMP field; it is not a same-pixels control, so it does not isolate the
  tier from the date.
  2026-10-10: a fresh free-tier generation of p1
  (`canva-ai-free__web-download__p1__3`, generated and downloaded on Free)
  carries the Canva manifest (`compositeWithTrainedAlgorithmicMedia`, valid
  when collected, `signingCredential.untrusted` only, certificate
  167403…226101, no time-stamp) and the XMP field. So
  the unmarked September free-tier p1 most likely reflects a change at Canva
  between September and October, not the tier; the one variable this does
  not isolate is account history (the September file came from an account
  never on Pro, p1__3 from one back on Free after a trial). Every Canva
  export collected in October, Free or Pro, generation or edit, carries both.
- p1 generation 2 (Pro): opened-preview PNG `__p1__2` and its JPG export
  `web-download-jpg__p1__2` (57.6 dB, same image).
- Files (23): free-tier p1; Pro p1 editor export (generation 1); Pro p1 PNG and
  JPG (generation 2); p2–p5 via opened preview; p3 larger-size; p3 bg-removed and
  bg-removed-erase; p2 magic-edit; edits E1–E3 via chat (3), Magic Edit (3),
  Magic Expand (1); p2 free-tier re-download; p1 fresh free-tier generation;
  p1 Free generation p1__4 and editor re-export of p1__3 (2026-10-20).
- 2026-10-19 collection: every October Canva export (edits, Free p1__3, Free
  p2__2) reads `Invalid` with `signingCredential.expired`; the two October
  certificates expired 2026-10-15T23:52Z and 23:53Z, no time-stamp (D-010).
  Content hashes still match.
- 2026-10-20, on Free: a new p1 generation (`web-download__p1__4`), from the
  preview, and a re-export of the Oct 10 p1__3 image from the design editor
  (`web-download-editor__p1__3`, 1012×1012; 40.2 dB against the Oct 10 preview
  download, so the same picture re-rendered at a smaller size). The design is
  named after p2 ("Rainy pine forest path"); the name reaches only the
  download file name. Both carry fresh Canva manifests on 8-day certificates
  without time-stamp: p1__4 serial 391F7E…A7591 (hex), valid
  2026-10-19T23:53:56Z to 10-27T23:53:56Z; the re-export serial 3DFFDF…F0E3BD
  (hex), valid 2026-10-19T23:52:12Z to 10-27T23:52:12Z: again two
  certificates issued about 100 s apart, as in October. The re-export is the
  same image signed twice, 10 days apart, on different certificates. Card vs
  preview: not determined. Share link: not allowed on Free. Non-default
  aspect: no setting on Free.

## Ideogram — Ideogram 3 and v4 (ideogram.ai)

- Date: 2026-09-10; v4 2026-09-11. Plan: free. Model/quality as shown in the UI:
  "p-image-ideogram-medium", rendering speed "Medium" → variant
  `p-image-ideogram-medium` (Ideogram's internal model id, as labelled in the UI).
  Default aspect: 1:1, 2048×2048.
- Paths: download button (`web-download`, JPG 2048×2048); right-click Save Image
  on the page (`browser-save`, WebP 2048×2048 — same pixels re-encoded, not a
  downscale); right-click Copy Image into macOS Preview (`copy-image`, PNG);
  Ideogram's own copy control (`ui-copy`, PNG via Preview), byte-identical to
  `copy-image` (sha256 cee8c4cc…): Preview writes both from the same bitmap.
- Variants: `-3x1` = 3:1 aspect of p2 (JPG 3072×1024); `-no-bg` = the
  transparent-background option on p1 (PNG with alpha, **1024×1024** — a
  different render at half resolution, the only PNG the download path produces);
  `ideogram-v4` = "Ideogram 4.0 (latest)" in the asset's details panel (checked
  2026-10-05: Generation, 1:1, 2048×2048, seed 354120087, created 2026-09-11
  06:03 local), same free account, p1 via download and copy-image; the
  manifest's claim_generator is `Ideogram V_4_0`.
- Files (13): medium p1 via download, browser-save, copy-image, ui-copy; p2–p5
  via download; p2 at 3:1; p1 no-bg; v4 p1 download and copy-image.
- Finding (Ideogram 3): C2PA manifest signed by "Ideogram, Inc." on every
  download JPG, `Valid` (signer not on the vendored 2026-08-14 list), one
  `c2pa.actions.v2` assertion with a single `c2pa.created` action declaring
  `trainedAlgorithmicMedia`, zero ingredients; no IPTC field in XMP. The
  `-no-bg` PNG carries no manifest and no XMP: a separate export pipeline that
  signs nothing. Browser-save WebP: nothing. Copy-image: Preview's XMP, no field.
- Finding (v4): download JPEG 2048×2048, manifest signed by "Ideogram, Inc.",
  `Valid` (`signingCredential.untrusted`), `trainedAlgorithmicMedia`; unlike the
  Ideogram 3 files it carries a second assertion, `com.ideogram.generation`. No
  XMP field. The v4 copy-image PNG carries nothing.
- Upscale: offered in the asset view but requires a paid plan (checked
  2026-10-20); not collected.
- Edits (2026-10-20, free account): E1 via Canvas Magic Fill on Ideogram V4.5
  (`ideogram-v4.5`, after the September `ideogram-v4`), the only model that
  accepts an uploaded photo for Magic Fill; the p-image model does not take
  it. `photo_bowl.jpg` uploaded from disk, apples masked, prompt verbatim,
  Magic Prompt off, no Style option shown. Ideogram then asked for a paid
  plan before E2 and E3 (Magic Fill, Extend), so those were not tested
  (editing uploaded images requires Ideogram Plus or higher, checked
  2026-10-20).
  Finding: one record, signed by Ideogram, Inc.: `c2pa.created`,
  trainedAlgorithmicMedia, zero ingredients. The Fold's record is replaced,
  as with Canva, but the edit is labelled a generation, not a composite. No
  XMP DigitalSourceType, no camera EXIF, no camera-record ID or camera string
  in the file bytes. 1760×2336, the photo's 3:4 kept.

## Midjourney
- Not tested: paid-only as of 2026-09-09, no subscription. Row shows "not tested".

## Not yet collected
- Gemini app on the Fold (`app-save-open`), p1.
- Local Stable Diffusion: A1111 with invisible-watermark on/off, ComfyUI,
  InvokeAI (the known-key Layer 2 fixture).

## Template for the next generator
```
## <Name> — <model> (<url>)
- Date, plan tier, default size/aspect, any toggle and its default.
- Paths available and what each button/menu is called in the UI.
- Findings from the hash/MIME check.
- Files (N): every file present, by variant and path; variants not offered;
  anything pending.
- Renames or deletions after collection: a dated "Withdrawn" or "removed" line
  saying what and why.
```
