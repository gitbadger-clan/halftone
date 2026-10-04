# 04-generators — provenance

Files in this directory are not committed; this file, `expectations.json` and
`PROMPTS.md` (one level up) are. Every generator gets a section below with how the
files were obtained. Write it while the UI is still open: plan tier, defaults, and
button labels change, and a row in the survival table needs a date.

## Naming

```
<generator>__<variant>__<path>__<prompt>__<n>.<ext>
```

- `generator`: `zai`, `google-flow`, `gemini-app`, `chatgpt`, `openai-api`,
  `aggregator-<site>`, `sd-a1111`, `comfyui`, …
- `variant`: model and settings, joined with `-`: `glm-wm-off`, `nano-banana-2`,
  `base` when there is nothing to say.
- `path`: how the bytes left the generator: `web-download`, `browser-save`,
  `copy-image` (add `-thumb` / `-open` only when the same action exists in both
  contexts and behaves differently), `api-png` / `api-jpeg` / `api-webp`, `share`,
  `edit-photo`.
- `prompt`: `p1`–`p5`, `e1`–`e3` from `../PROMPTS.md`.
- `n`: generation number for that prompt.
- `ext`: whatever the path produced, even when it is wrong for the bytes. The
  mismatch is a finding; the collector sniffs the real type.

For `copy-image`, the receiving application is the writer: name it below.

## Z.ai — GLM-Image (image.z.ai/create)

- Date: 2026-09-09. Plan: free. Default size 1K, 1:1.
- UI has a "No Watermark" checkbox, **checked by default** (`glm-wm-off` is the
  default state; `glm-wm-on` is the opt-in).
- Paths: download button (`web-download`), right-click Save Image on the page
  image (`browser-save`), right-click Copy Image pasted into Preview and saved as
  PNG (`copy-image`).
- Finding (sha256 identical across four files):
  `wm-off` browser-save == `wm-on` browser-save == `wm-on` web-download.
  The page image is a single watermarked JPEG served regardless of the toggle; the
  `wm-on` download button hands out that same JPEG with a `.png` extension. Only the
  `wm-off` download produces a different file. The opt-out applies to the download
  button and to nothing else.
- Downloads named `.png` are JPEG bytes (see `expectations.json` MIME).
- Files: p1–p5 `glm-wm-off` download; p1, p2 `glm-wm-on` download; p1 and p2
  browser-save and copy-image in `wm-on`; p1 browser-save and copy-image in `wm-off`.
- Not offered: API access on this tier, edit of an uploaded photo, upscale.

## Google Flow — Nano Banana 2 (flow.google, formerly ImageFX)

- Date: 2026-09-09. Plan: free (free tier serves Nano Banana 2; paid tiers serve
  other variants, which would be a different `variant` token).
- ImageFX was retired 2026-04-30 and folded into Flow; `imagefx` is not a valid
  generator token in this corpus.
- Paths: the download button is on the thumbnail card (`web-download`); the page
  image can be saved and copied both from the thumbnail (`-thumb`) and from the
  opened full-size view (`-open`), and the two differ in size. `copy-image` pasted
  into Preview, saved as PNG.
  - `web-download` from thumbnail and from opened view give byte-identical files
  (p2, sha256 <first 8>…<last 8>); opened-view copy not kept.
- Finding: the download button produces a JPEG; the page (both contexts) serves a
  PNG render. Which is the stored original: see `ImageSize` / `FileSize` in
  `expectations.json` and the hash comparison in the log.
- Files: p1 all five paths; p2–p5 `web-download`.
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
- Files: p1 via all three paths. p2–p5 not generated (credits).

## Adobe Firefly — Image 5 and Image 4 Ultra (firefly.adobe.com)

- Date: 2026-09-09 to 2026-09-11 (Adobe models), 2026-10-01 (opened-view p1,
  remote-manifest pass, partner models). Plan: free tier, 4 generations per day,
  which is why prompts arrive over several days. Default aspect: 4:3, landscape.
  Default model on this tier: Firefly Image 5; Image 4 Ultra selectable, used as
  a second variant for p1–p2.
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
    on p1, downloaded from the opened view. JPEG, 4608×3584, 2× the native size;
    the upscale path switches the output format from PNG to JPEG.
  - `share-copy`, `share-copy-thumb`: Adobe's own Share → Copy control (not the
    browser clipboard), from the opened view and from the card. Pasted into macOS
    Preview, saved as PNG at 2304×1792; the 5 MB PNGs are a clipboard bitmap
    re-encoded by Preview, so Preview is the writer of those files.
- Finding (formats): native download is PNG 2304×1792 (Image 5) or JPEG (Ultra),
  format fixed per model; Upscale output is JPEG 4608×3584.
- Finding (credentials): Adobe-model downloads (Image 5 PNG, Image 4 Ultra JPEG)
  carry no embedded manifest. XMP holds only a remote-manifest reference at
  cai-manifests.adobe.com and no IPTC DigitalSourceType. Each download gets its
  own manifest (card p1 urn-c2pa-db34c8cc…, opened-view p1 urn-c2pa-063e1335…).
  Fetched 2026-10-01, both are from Adobe Firefly, signed by Adobe Inc.,
  cryptographically valid, content unchanged, trainedAlgorithmicMedia, but the
  signer does not chain to the vendored C2PA trust anchors (<which list it is on,
  once checked>). Offline, an Adobe-model file verifies nothing. The earlier
  expectation that Adobe's certificate is in the vendored trust list did not hold.
- Files (11): Image 5 p1–p5 card download (p1–p2 `-thumb`, p3–p5 unsuffixed),
  p1 opened-view download, p1 upscaled via download-open, p1 share-copy; Image 4
  Ultra p1–p2 card download, p1 share-copy-thumb.
- Pending: same-day card re-download of p1 to separate route from date (outside
  the corpus until it's clear whether it's a new row); TrustMark decode of the
  card and opened-view p1 (hypothesis: per-download watermark carrying the
  manifest pointer); which trust list Adobe's signer is on; compare the upscaled
  and native manifests (upscale action with the original as ingredient, or a
  fresh manifest); `edit-photo` e1–e3 via Generative Fill once
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
  Bare IHDR/IDAT/IEND + caBX chunk layout. No XMP.
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
    served it, saved from the browser. 27 KB JPEG, i.e. a platform thumbnail.
    First platform-served file in the corpus; a stratum-10 row kept here for now.
- Finding (hash and size check): the four download JPEGs (`web-download-thumb/
  -open`, `share-download-thumb/-open`) are byte-identical; the two WebPs
  (`browser-save-open`, `share-browser-save`) are byte-identical. Meta serves one
  JPEG through every download control and one lossy WebP render of the same
  1920×1280 pixels to the page — re-encoded, not downscaled. Future Meta AI
  downloads need only `web-download-thumb`.
- Files (16): instant p1 via all nine paths; instant p2–p5 via
  `web-download-thumb`; thinking p1 via web-download-thumb, share-download-thumb,
  browser-save-thumb, copy-image-thumb.
- Pending: Meta AI inside WhatsApp, saved from the chat (`whatsapp-save`);
  `edit-photo` e1–e3 once base photos exist. The app moved to its own section
  below (collected on the Pixel 5, not the Fold).
- Expectation to check: Meta's stated policy is an IPTC `DigitalSourceType` in
  XMP; whether a manifest is present at all; whether the Instagram thumbnail kept
  either.

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
  "generated → edited in Canva → exported", separate from e1–e3.
- Finding: C2PA manifest signed by Canva (c2pa-rs 0.89.3), `Valid` on 2026-09-10
  (signer not on the vendored 2026-08-14 list); `Invalid` once the certificate
  expired 2026-09-17 (observed 2026-09-20) with no `c2pa.time-stamp` (see D-010).
  No IPTC field in XMP.
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
  stays the control.
- Files (12): free-tier p1; Pro p1 via opened preview, editor, and JPG; p2–p5 via
  opened preview; p3 larger-size; p3 bg-removed and bg-removed-erase; p2
  magic-edit.
- Pending: after the trial ends, re-download p2 from the same Pro-era design on
  the free tier (`canva-ai-free__web-download__p2__2`) — same generation, same
  control, only the tier differs; share link → served asset; one non-default
  aspect; e1–e3 on the base photos via Canva's edit tools once they exist.

## Ideogram — Ideogram 3 (ideogram.ai), rendering "Medium"

- Date: 2026-09-10. Plan: free. Model/quality as shown in the UI: "p-image-ideogram-medium", rendering speed "Medium" → variant
  `p-image-ideogram-medium` (Ideogram's internal model id, as labelled in the UI).
  Default aspect: 1:1, 2048×2048.
- Paths: download button (`web-download`, JPG 2048×2048); right-click Save Image
  on the page (`browser-save`, WebP 2048×2048 — same pixels re-encoded, not a
  downscale); right-click Copy Image into macOS Preview (`copy-image`, PNG).
- Variants: `-3x1` = 3:1 aspect of p2 (JPG 3072×1024); `-no-bg` = the
  transparent-background option on p1 (PNG with alpha, **1024×1024** — a
  different render at half resolution, the only PNG the download path produces).
- Files (10): p1 via all three paths (copy-image twice); p2–p5 via download; p2
  at 3:1; p1 no-bg.
- Finding: C2PA manifest signed by "Ideogram, Inc." on every download JPG,
  `Valid` (signer not on the vendored 2026-08-14 list), one `c2pa.actions.v2`
  assertion with a single `c2pa.created` action declaring
  `trainedAlgorithmicMedia`, zero ingredients; no IPTC field in XMP. The
  `-no-bg` PNG carries no manifest and no XMP: a separate export pipeline that
  signs nothing. Browser-save WebP: nothing. Copy-image: Preview's XMP, no field.
- Pending: upscale if offered; edit-photo via Ideogram's canvas/inpaint once base
  photos exist.

### Meta AI — Android app (collected 2026-09-30)
- Device: Pixel 5, Android 14; package com.facebook.stella 290.1.0.42.163; account tier free
- Default mode in app: instant; model label shown: instant
- Routes offered: <exact labels>; clipboard routes not collected (receiving app re-encodes)
- Transfer: `adb pull` from shared storage, sha256 checked on device and host.
  Clipboard routes not collected: on Android the app you paste into re-encodes
  the bitmap, so the row would describe the receiving app.
- Paths offered: download button
  - `app-save-open`: save control in the opened view. Lands in `Download/` under
    a CDN-style name `828912701_2030331367622725_5122853495433315818_n.webp`.
- Finding: the `.webp` file is JPEG bytes (161,244 B, progressive, 4:2:0,
  optimised Huffman). XMP `DigitalSourceType` = `trainedAlgorithmicMedia`
  present; no C2PA manifest. Same marking as the web downloads; `jpeg_double`
  is Inconclusive because Halftone doesn't yet read progressive JPEGs.
- `share`: Share → Android system share sheet. The app shares text/plain
  (an 89-character link), not the image; Files by Google receives text and
  saves nothing. No file produced. The link leads to the web share page,
  already covered by the web section's `share-*` rows.
- `copy-image`: Copy puts an image on the clipboard (the app's own input
    rejects the paste as an image; `dumpsys clipboard` is not readable by the
    shell on Android 14). Not collected: no byte-exact way to read it, and any
    paste target would be the writer.
- Files (6): instant p1–p5 `app-save-open`; thinking p1 `app-save-open`.
  p3–p5 collected for completeness, though the rule only requires them when
  the app behaves differently from the web.
- Finding: all five JPEG under a `.webp` name, XMP present, no manifest;
  quantization tables identical. Only the save control
  produces a file: Share sends a link, Copy puts an image only another app can read.

## Meta AI — inside WhatsApp (Android, Pixel 9 Pro Fold)

- Date: 2026-10-01. Device: Pixel 9 Pro Fold, Android 17. WhatsApp 2.26.37.73.
  Region: CH. adb over wireless debugging.
- Settings: Enter is send off; Media visibility on; auto-download Wi-Fi all
  media, mobile data photos, roaming none; auto-download quality Auto;
  media upload quality Standard.
- Image generation: offered.
- Paths:
  - `whatsapp-autosave`: not produced. Despite media visibility on and Wi-Fi
    auto-download "All media", the Meta AI image was not written to shared
    storage on arrival.
  - `whatsapp-save`: open image → ⋮ → Save. Lands as
    `WhatsApp Images/IMG-20261001-WA8214.jpg`.
  - `whatsapp-share-files`: open image → Share → Files by Google → Save to
    Downloads. Lands as `Download/image.jpg` (generic name). Byte-identical to
    `whatsapp-save` (sha256 c389b240…276155e); file not kept.
- Finding: JPEG, 1,030,259 B, standard libjpeg tables at Q100, 4:2:0, baseline.
  No XMP DigitalSourceType and no C2PA: the trainedAlgorithmicMedia field that
  every web and app save carries is gone. Both file routes deliver the same
  bytes, so the re-encode happens before either (bot send pipeline or WhatsApp
  receive/download; not distinguishable here). Auto-save never wrote the image
  despite the settings. Invisible watermarking, if Meta applies any, is not
  readable by Halftone and untested here.

## Midjourney
- Not tested: paid-only as of 2026-09-09, no subscription. Row shows "not tested".

## Template for the next generator
```
## <Name> — <model> (<url>)
- Date, plan tier, default size/aspect, any toggle and its default.
- Paths available and what each button/menu is called in the UI.
- Findings from the hash/MIME check.
- Files present; variants not offered; anything pending.
```
