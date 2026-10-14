# 00-base — base images

Inputs, not test cases: `scripts/differential.py` does not collect this folder.

## Synthetic bases

- `base.jpg`, `base.png`, `base.webp`: written by <script from rg>; inputs for
  03-synthetic.

## Photos for the edit rows (E1–E3 in ../PROMPTS.md)

Pixel 9 Pro Fold, Pixel Camera 10.4.117, Android 17; main camera at 1×; Top
Shot off; location off. Each file is signed by the camera (C2PA,
computationalCapture), so every edit row also shows what a tool does to a
signed original. No people, text or location: these files are uploaded to
third-party generators. Not committed.

- `photo_bowl.jpg`: 2026-10-08. Glass bowl of green apples on a dark granite
  worktop, induction hob behind. Ultra HDR off: single JPEG. For E1.
- `photo_flask.jpg`: 2026-10-08. Black steel flask on a wooden grid board,
  white wall behind. Ultra HDR off: single JPEG. For E2.
- `photo_path.jpg`: 2026-10-09. Grass aisle between vine rows, framed by two
  wooden posts, trellis wires across the frame, overcast sky; single JPEG,
  signed. For E3.
- Verifying edit rows: an edit that keeps the provenance chain carries the
  uploaded photo's camera manifest; its ID proves which base photo went in
  (`photo_bowl` …4969aa2caf8f, `photo_flask` …4665cbaddaa9, `photo_path`
  …4c914b5479b0; last 12 characters of the active manifest's URN).
