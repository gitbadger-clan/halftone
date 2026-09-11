# Contributing

Thanks for the interest. A few rules that keep the project shippable.

## Licensing of contributions

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in Halftone by you, as defined in the Apache-2.0
license, shall be dual licensed as above (MIT OR Apache-2.0), without
any additional terms or conditions.

Sign off your commits (`git commit -s`) to certify the Developer
Certificate of Origin (https://developercertificate.org).

## What we take

- Bug fixes, new container fingerprints (with a reproducible source:
  encoder name, version, quality setting, and the file that produced
  the DQT), new `EvidenceSource` implementations, test assets you have
  the rights to.
- Model packs are **not** accepted into this repo. Publish your own;
  the pack format is open.

## Checks

`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
must pass on Linux, macOS and Windows (CI runs all three).
Shipped binaries: `cargo install --path crates/halftone-cli --features c2pa --locked`
(the lockfile is part of the tested surface; see DIFFERENTIAL.md D-007).
Differential test: `cargo test -p halftone-cli --features c2pa --test differential -- --ignored`
with the corpus on disk; CI runs strata 01–03 on every push.
