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
- Pending: browser-save and copy-image of a wm-off generation (the 2026-09-09
  p1 if still in history, else a new p1 as `__p1__2` via all three paths); that
  is what tests whether the toggle reaches the page image.
- Not offered: API access on this tier, edit of an uploaded photo, upscale.

## Google Flow — Nano Banana 2 (flow.google, formerly ImageFX)

- Date: 2026-09-09. Plan: free (free tier serves Nano Banana 2; paid tiers serve
  other variants, which would be a different `variant` token).
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
- Files (10): p1 via browser-save-open, copy-image-open, copy-image-thumb,
  web-download-thumb; p2–p5 web-download-thumb; p3 1:1; p2 upscaled.
- Pending: `edit-photo` e1–e3 once `00-base/photo_*.jpg` exist (Flow edits uploaded
  images with Nano Banana).

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
- Files (7): p1 download; p1 copy-image as `__p1__2` (copied from a regenerated
  image, 12.1 dB against p1__1); p2–p5 download; p2 at 1:1.
- Pending: download of the p1__2 generation, so the copy route has a
  same-generation partner (marking kept or lost through the clipboard);
  re-collect after 2026-10-09, when p1's certificate expires (p1 and p4 should
  then read `expired` too, if the signature time is not used); which TSA signed
  the Bing timestamps and whether it is in `C2PA-TSA-TRUST-LIST.pem`; the
  certificate serials of p3, p5 and the 1:1 p2.

## Adobe Firefly — Image 5 and Image 4 Ultra (firefly.adobe.com)

- Date: 2026-09-09 to 2026-09-11 (Adobe models), 2026-10-01 (remote-manifest
  pass, partner models), 2026-10-02 (p1 variant 2, route check). Plan: free tier,
  4 generations per day, which is why prompts arrive over several days. Default
  aspect: 4:3 landscape as labelled in the UI; the files are 9:7 (2304×1792,
  1152×896). Default model on this tier: Firefly Image 5; Image 4 Ultra
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
  4608×3584, 2× variant 1.
- Finding (credentials): Adobe-model downloads (Image 5 PNG, Image 4 Ultra JPEG)
  carry no embedded manifest. XMP holds only a remote-manifest reference at
  cai-manifests.adobe.com and no IPTC DigitalSourceType. Each card download of an
  image returns one stored manifest reference; each opened-view download creates
  a new one, so one image can have any number of valid manifests on Adobe's
  server. Fetched 2026-10-01, manifests are from Adobe Firefly, signed by Adobe
  Inc., cryptographically valid, content unchanged, trainedAlgorithmicMedia, but
  the signer does not chain to the vendored C2PA trust anchors (see Pending).
  Offline, an Adobe-model file verifies nothing. The earlier expectation that
  Adobe's certificate is in the vendored trust list did not hold.
- Withdrawn 2026-10-02: a 2026-10-01 "redownload" of p1 was a copy of the upscaled
  file (identical bytes) and was removed; the "delivery pipeline changed between
  dates" reading came from comparing variant 1 with variant 2 and does not hold.
- Files (12): Image 5 p1 variant 1 card download and share-copy; p1 variant 2
  card and opened-view downloads (`p1__2`); p2–p5 card downloads; p1 upscaled
  card download; Image 4 Ultra p1–p2 card downloads, p1 share-copy-thumb.
- Pending: which trust list Adobe's signer is on; compare the upscaled and native
  manifests (upscale action with the original as ingredient, or a fresh
  manifest); why Image 5 p1 variant 1 is 2304×1792 when every later Image 5
  download is 1152×896 (a same-day pair of new downloads would separate date
  from generation); `edit-photo` e1–e3 via Generative Fill once
  `00-base/photo_*.jpg` exist.
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
- Pending: `edit-photo` e1–e3 once base photos exist.
- Files (11): instant p1 web-download-thumb, browser-save-open, copy-image-open,
  share-instagram-browser-save; instant p2–p5 web-download-thumb; thinking p1
  web-download-thumb, browser-save-thumb, copy-image-thumb.
- Pending: `edit-photo` e1–e3 once base photos exist; whether the Instagram
  thumbnail kept the XMP field.
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

- Date: 2026-09-10. Plan: Pro trial (one free-tier file predates it, see below).
  Model as shown in the UI: not stated → variant `canva-ai`. Default aspect: 1:1.
- Purpose in the corpus: agency tooling. Canva is a C2PA member; whether its
  export writes a manifest, and whether its own edits keep one, is the row.
- Paths: all downloads taken from the opened preview (`web-download`). The
  download dialog offers PNG (default) and JPG (`web-download-jpg`), plus a
  "larger size" option (variant `canva-ai-larger`, an export-time upscale). The
  editor-canvas export after "Edit image" is `web-download-editor`. The
  result-card download was not exercised. Right-click Save / Copy on the canvas:
  not available.
- Edits (variant token), all on Canva's own generations: `bg-removed` =
  Background Remover on p3; `bg-removed-erase` = Background Remover then Magic
  Eraser on a segment of p3; `magic-edit` = Magic Edit (generative inpaint) on p2.
  Expand (outpaint): not found in this UI on 2026-09-10. Survival rows for
  "generated → edited in Canva → exported", separate from e1–e3. `larger` p3 and
  `magic-edit` p2 are listed as derived from their sources.
- Finding: C2PA manifest signed by Canva (c2pa-rs 0.89.3). Date-dependent:
  `Valid` on 2026-09-10 (signer not on the vendored 2026-08-14 list); `Invalid`
  since the signing certificate expired on 2026-09-17 (first observed
  2026-09-20, unchanged in the 2026-10-04 collection), because the manifest
  carries no `c2pa.time-stamp` (DIFFERENTIAL.md D-010). No IPTC field in XMP.
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
- Tier: `canva-ai-free` p1 was exported on the free tier before the trial and
  carries no manifest and no XMP; every Pro-tier export carries the manifest.
  Tier-dependent marking is the hypothesis; see Pending.
  2026-10-04: the corpus test found the free-tier p1 and the Pro editor export
  (`web-download-editor__p1__1`) pixel-identical (AE 0, full resolution, both
  1200×1200): same generation, manifest only on the Pro export. Tier and path both
  differ, so this supports the hypothesis without isolating it; the p2 re-download
  stays the control. Listed under `known_duplicates` in corpus-rules.
- p1 generation 2 (Pro): opened-preview PNG `__p1__2` and its JPG export
  `web-download-jpg__p1__2` (57.6 dB, same image).
- Files (12): free-tier p1; Pro p1 editor export (generation 1); Pro p1 PNG and
  JPG (generation 2); p2–p5 via opened preview; p3 larger-size; p3 bg-removed and
  bg-removed-erase; p2 magic-edit.
- Pending: after the trial ends, re-download p2 from the same Pro-era design on
  the free tier (`canva-ai-free__web-download__p2__2`) — same generation, same
  control, only the tier differs; share link → served asset; one non-default
  aspect; e1–e3 on the base photos via Canva's edit tools once they exist.

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
- Files (12): medium p1 via download, browser-save, copy-image, ui-copy; p2–p5
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
- Pending: Upscale is offered in the asset view (checked 2026-10-05), not yet
  collected; edit-photo via Ideogram's Edit once base photos exist.

## Midjourney
- Not tested: paid-only as of 2026-09-09, no subscription. Row shows "not tested".

## Not yet collected
- ChatGPT web and OpenAI API (`scripts/collect-api.py openai`): closes D-001 and
  is the direct counterpart of the Firefly GPT Image 2.5 Flare row.
- Gemini app (web, Fold) and Gemini API: the direct counterpart of the Flow and
  Firefly Nano Banana 2 rows.
- Local Stable Diffusion: A1111 with invisible-watermark on/off, ComfyUI,
  InvokeAI (the known-key Layer 2 fixture).
- `00-base/photo_{bowl,path,flask}`: blocks every `edit-photo` row.

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
