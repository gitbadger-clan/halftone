//! Input asset: bytes plus the little metadata every layer needs.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Coarse media type. Sources declare which they support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modality {
    /// Still image.
    Image,
    /// Video (container with video track).
    Video,
    /// Audio.
    Audio,
    /// Plain text.
    Text,
}

/// An asset loaded into memory. Kept simple on purpose; sources decode what they need.
#[derive(Debug, Clone)]
pub struct Asset {
    /// Original path, if any.
    pub path: Option<PathBuf>,
    /// Raw bytes.
    pub bytes: Vec<u8>,
    /// Detected modality.
    pub modality: Modality,
    /// Sniffed MIME type (from magic bytes, not extension).
    pub mime: String,
    /// SHA-256 of `bytes`, hex.
    pub sha256: String,
}

impl Asset {
    /// Build from bytes, sniffing modality and MIME from magic bytes.
    pub fn from_bytes(bytes: Vec<u8>, path: Option<PathBuf>) -> crate::Result<Self> {
        let (modality, mime) = sniff(&bytes)
            .ok_or_else(|| crate::Error::Unsupported("unrecognised file type".into()))?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        Ok(Self { path, bytes, modality, mime: mime.to_string(), sha256 })
    }

    /// Build from a file.
    pub fn from_path(path: impl AsRef<Path>) -> crate::Result<Self> {
        let p = path.as_ref();
        let bytes = std::fs::read(p)?;
        Self::from_bytes(bytes, Some(p.to_path_buf()))
    }

    /// Build a text asset explicitly (text has no magic bytes).
    pub fn text(s: impl Into<String>, path: Option<PathBuf>) -> Self {
        let bytes = s.into().into_bytes();
        let sha256 = hex::encode(Sha256::digest(&bytes));
        Self { path, bytes, modality: Modality::Text, mime: "text/plain".into(), sha256 }
    }

    /// Serializable summary for reports.
    pub fn info(&self) -> AssetInfo {
        AssetInfo {
            path: self.path.clone(),
            modality: self.modality,
            mime: self.mime.clone(),
            sha256: self.sha256.clone(),
            size_bytes: self.bytes.len() as u64,
        }
    }
}

/// Serializable asset summary embedded in every [`crate::Inspection`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetInfo {
    /// Original path.
    pub path: Option<PathBuf>,
    /// Modality.
    pub modality: Modality,
    /// MIME type.
    pub mime: String,
    /// SHA-256 hex.
    pub sha256: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// Minimal magic-byte sniffer. Extend as formats are added; never trust extensions.
fn sniff(b: &[u8]) -> Option<(Modality, &'static str)> {
    use Modality::*;
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some((Image, "image/jpeg"));
    }
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((Image, "image/png"));
    }
    if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        return Some((Image, "image/webp"));
    }
    if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WAVE" {
        return Some((Audio, "audio/wav"));
    }
    if b.starts_with(b"fLaC") {
        return Some((Audio, "audio/flac"));
    }
    if b.starts_with(b"ID3") || (b.len() > 1 && b[0] == 0xFF && (b[1] & 0xE0) == 0xE0) {
        return Some((Audio, "audio/mpeg"));
    }
    if b.len() > 12 && &b[4..8] == b"ftyp" {
        // MP4/MOV/HEIF family. Refine by brand later (heic/avif are images).
        let brand = &b[8..12];
        return match brand {
            b"heic" | b"heix" | b"mif1" | b"avif" => Some((Image, "image/heif")),
            _ => Some((Video, "video/mp4")),
        };
    }
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some((Video, "video/webm"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_jpeg_and_png() {
        let jpg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 0];
        assert_eq!(sniff(&jpg), Some((Modality::Image, "image/jpeg")));
        let png = b"\x89PNG\r\n\x1a\n\0\0".to_vec();
        assert_eq!(sniff(&png), Some((Modality::Image, "image/png")));
    }

    #[test]
    fn unknown_is_none() {
        assert_eq!(sniff(b"hello"), None);
    }

    #[test]
    fn asset_hashes_bytes() {
        let a = Asset::from_bytes(vec![0xFF, 0xD8, 0xFF, 0xE0], None).unwrap();
        assert_eq!(a.sha256.len(), 64);
        assert_eq!(a.modality, Modality::Image);
    }
}
