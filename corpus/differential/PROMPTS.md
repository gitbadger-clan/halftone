# Prompt set for strata 04 (generators), 05 (Fold) and their derivatives

Every generator gets the same prompts, verbatim, at the generator's default size
and default aspect (whatever it is; note it in SOURCE.md). The survival table is
about what a user gets without touching settings. The point is that two files from the
same generator differ only by exit path, and files across generators share content,
so later steps (survival ladder, messaging, blind-detector evaluation, watermark
robustness) can pair them. Do not tune a prompt per generator. If a generator
refuses or mangles it, record that in SOURCE.md and move on: a refusal is a row.

Filename token: `p1`…`p5`, `e1`…`e3`.

## Generation prompts (text-to-image)

P1 · still life, smooth + specular
```
A ceramic bowl of lemons on a wooden table by a window, soft morning light, photographic.
```

P2 · landscape, high-frequency texture
```
A gravel path through a pine forest after rain, overcast sky, mist between the trees, photographic.
```

P3 · product, large flat regions + hard edges
```
A matte black thermos flask on a plain grey surface, studio lighting, no text, product photograph.
```

P4 · macro texture, periodic detail
```
Close-up of undyed linen fabric folded on a table, natural window light, every thread visible, photographic.
```

P5 · flat illustration, non-photographic control
```
A flat vector-style illustration of a lighthouse on a rocky coast at dusk, limited palette, clean shapes, no text.
```

Why these five: P1–P3 are the three scenes any phone can re-shoot for real/generated
pairs (Layer 3, January). P2 and P4 carry the texture a frequency-domain watermark
hides in; P3 and P5 carry the flat regions where it does not, which is what a
robustness ladder needs to show both ends of. P5 also gives the blind layer one
non-photo negative per generator.

## Edit prompts (image-to-image, on your own phone photos)

Shoot the three base photos once on the Fold, plain camera, no HDR effects, and reuse
them for every generator that offers editing. Keep the originals in
`corpus/differential/00-base/photo_{bowl,path,flask}.jpg`.

E1 · inpaint, object replacement (photo_bowl)
```
Replace the lemons in the bowl with green apples. Keep everything else unchanged.
```

E2 · background change (photo_flask)
```
Change the background to a wooden workbench with soft daylight. Keep the flask exactly as it is.
```

E3 · outpaint / extend (photo_path)
```
Extend the image to the left and right to show more of the forest, matching the light and mist.
```

Expected declaration for E1–E3 is `compositeWithTrainedAlgorithmicMedia`; a
generator that writes `trainedAlgorithmicMedia` for an edit of a real photo is a
finding, not a bug in Halftone.

## Local models (SD 1.5 / SDXL / Flux), for reproducibility

Record model checkpoint hash, sampler, steps, CFG, seed in SOURCE.md. Suggested
fixed settings so a re-run reproduces the file: seed 20260909, 30 steps, CFG 6,
default sampler of the UI, 1024×1024 (SDXL/Flux) or 512×512 (SD 1.5).
Negative prompt for SD 1.5/SDXL only:
```
text, watermark, logo, signature, blurry, deformed
```

## Sizes and variants

- Default size and aspect for p1–p5. One additional non-default aspect of p3 per
  generator that offers it (aspect in the variant token, e.g. `nano-banana-2-1x1`
  or `-portrait`), because some writers change encoder path with dimensions.
- Transparent-background variant of p3 where offered (`__alpha`): PNG-with-alpha is
  a different writer path in several tools.
- One upscale of p2 where offered (`__upscale`): upscalers are separate models and
  frequently drop the marking of the input.

## What not to do

- No people, faces, brands, logos, or rendered text: filters differ by service and
  the blind layer should not learn on likeness.
- No prompt edits to "make it work" on a stubborn generator.
- No re-saving before filing: the file that goes in the corpus is the one the exit
  path produced.
