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
