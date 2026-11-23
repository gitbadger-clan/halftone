//! Trust lists, model packs and licences.
//!
//! Pack = `<name>-<version>.tar.zst` + detached Ed25519 signature + `pack.json`
//! (name, version, files with sha256, `calibration.json`). Verified on install and
//! on every load. The subscription is "current packs"; installed packs never expire.
//!
//! License = signed JSON (customer, plan, expiry, features). [`license`] holds its
//! shape only: nothing verifies a licence yet.
//! No network calls except an explicit `ht packs update`.

pub mod index;
pub mod license;
pub mod manifest;
pub mod store;
pub mod update;

pub use manifest::{Calibration, PackManifest, Tier};
