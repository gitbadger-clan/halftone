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

- Date: 2026-09-09. Plan: free tier, 4 generations per day, which is why p2–p5
  arrive over several days. Default aspect: 4:3, landscape. Default model on this
  tier: Firefly Image 5; Image 4 Ultra selectable, used for one p1 as a second
  variant.
- Browser context actions are not available: right-click Save Image and Copy Image
  are disabled on the canvas. Recorded as `browser-save` / `copy-image`: not offered.
  Adobe funnels every exit through its own controls.
- Paths offered:
  - `web-download-thumb`: download control on the result card. Image 5: PNG-only
    dialog, 2304×1792. Image 4 Ultra: JPEG, 2304×1792, no format dialog.
  - `web-download-open`: download control in the opened full-size view. Not yet
    exercised on an un-upscaled image (see Pending).
  - Explicit Upscale (variant `firefly-image-5-upscaled`): the UI's Upscale action
    on p1, downloaded from the opened view. JPEG, 4608×3584, 2× the native size;
    the upscale path switches the output format from PNG to JPEG.
  - `share-copy`, `share-copy-thumb`: Adobe's own Share → Copy control (not the
    browser clipboard), from the opened view and from the card. Pasted into macOS
    Preview, saved as PNG at 2304×1792; the 5 MB PNGs are a clipboard bitmap
    re-encoded by Preview, so Preview is the writer of those files.
- Finding: native card download is PNG 2304×1792; Upscale output is JPEG
  4608×3584. Compare the two manifests: whether the upscaled file records an
  upscale action with the original as ingredient, or carries a fresh manifest.
- Files so far (5): Image 5 p1 via download-thumb (PNG), upscaled via download-open
  (JPEG), share-copy (PNG); Image 4 Ultra p1 via download-thumb (JPEG),
  share-copy-thumb (PNG).
- Pending: p2–p5 Image 5 via card download (one per day); p1 via opened-view
  download without upscaling, hashed against the card download; `edit-photo`
  e1–e3 via Generative Fill once `00-base/photo_*.jpg` exist.
- Not offered / not tested: partner models in the same UI (GPT Image, Flux) —
  one p1 from one of them would show Adobe signing another provider's output;
  API (Firefly Services) not on this tier.
- Expectation to check first: manifest `Trusted` on the vendored list (Adobe's
  signing certificate is in the C2PA trust list), and whether an IPTC
  `DigitalSourceType` also appears in XMP alongside the manifest.

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
- Pending: the Meta AI app on the Fold — in-app save (`app-save`) and the share
  sheet (`share`); Meta AI inside WhatsApp, saved from the chat (`whatsapp-save`);
  `edit-photo` e1–e3 once base photos exist.
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
- Finding: C2PA manifest signed by Canva (c2pa-rs 0.89.3), `Valid` (signer not on
  the vendored 2026-08-14 list). One assertion, `c2pa.actions.v2`, one action
  `c2pa.created` declaring `compositeWithTrainedAlgorithmicMedia`; zero
  ingredients. The same manifest shape on plain generations, on the editor
  export, and on the edited variants: Canva's edits are not recorded as actions
  and the generated image is not an ingredient. The manifest asserts "created in
  Canva with generative AI" and carries no history. No IPTC field in XMP.
- Tier: `canva-ai-free` p1 was exported on the free tier before the trial and
  carries no manifest and no XMP; every Pro-tier export carries the manifest.
  Tier-dependent marking is the hypothesis; see Pending.
- Files (12): free-tier p1; Pro p1 via opened preview, editor, and JPG; p2–p5 via
  opened preview; p3 larger-size; p3 bg-removed and bg-removed-erase; p2
  magic-edit.
- Pending: after the trial ends, re-download p2 from the same Pro-era design on
  the free tier (`canva-ai-free__web-download__p2__2`) — same generation, same
  control, only the tier differs; share link → served asset; one non-default
  aspect; e1–e3 on the base photos via Canva's edit tools once they exist;
  confirm the magic-edit manifest has the same one-action, zero-ingredient shape.

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
