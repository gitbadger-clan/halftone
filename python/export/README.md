Export scripts: PyTorch checkpoint → ONNX with static shapes → int8 where accuracy holds.
Output goes to `packs/<name>/<version>/` together with `pack.json` and `calibration.json`
produced by `halftone bench`. Rust never imports anything from here.
