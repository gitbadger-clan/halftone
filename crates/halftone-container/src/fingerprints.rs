//! Writer fingerprint database for JPEG structure (Kee–Johnson–Farid style).
//!
//! A fingerprint is the SHA-256 of everything the *writer* fixes independent of image
//! content: quantization tables, Huffman tables, sampling factors, component ids and
//! pre-SOS marker order (see [`crate::jpeg::JpegStructure::fingerprint`]). Two files
//! written by the same software at the same settings share it; a camera firmware
//! shares it across every photo it takes.
//!
//! The built-in DB is deliberately small and only contains entries harvested from real
//! writers. Standard libjpeg-family output is *not* in it — that family is recognised
//! analytically by [`crate::jpeg::classify_encoder`]. The DB is for what the analytic
//! rules cannot name: camera firmwares, Photoshop presets, specific service export
//! paths. Grow it with `halftone fingerprint --writer <name> --class <class> <files>`.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::jpeg::JpegStructure;
use crate::png::PngInfo;

/// Coarse class of the software that produced a fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriterClass {
    /// Dedicated camera firmware.
    Camera,
    /// Phone camera app.
    Phone,
    /// Image editor (Photoshop, GIMP, Lightroom export...).
    Editor,
    /// Encoding library at specific settings (Pillow, OpenCV, mozjpeg, libjpeg-turbo).
    Library,
    /// Messaging / social re-encode path (WhatsApp, Telegram, Facebook...).
    Messaging,
    /// OS screenshot pipeline.
    Screenshot,
    /// Image-generation service or pipeline export path.
    Generator,
    /// Known but unclassified.
    Unknown,
}

impl WriterClass {
    /// Short human description used in rationales.
    pub fn describe(self) -> &'static str {
        match self {
            Self::Camera => "camera firmware",
            Self::Phone => "phone camera app",
            Self::Editor => "image editor",
            Self::Library => "encoding library at known settings",
            Self::Messaging => "messaging/social re-encode path",
            Self::Screenshot => "screenshot pipeline",
            Self::Generator => "generation-tool export path",
            Self::Unknown => "unclassified writer",
        }
    }
}

/// One known writer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriterEntry {
    /// Hex SHA-256 fingerprint.
    pub fingerprint: String,
    /// Container the fingerprint was computed over: `jpeg` or `png`.
    #[serde(default = "default_format")]
    pub format: String,
    /// Human name, e.g. `Canon EOS R5 fw 1.8.1`, `Pillow 10 q=75 4:2:0`.
    pub writer: String,
    /// Class.
    pub class: WriterClass,
    /// Free-form notes (settings, how it was obtained).
    #[serde(default)]
    pub notes: String,
    /// Who/what harvested it (provenance of the reference data itself).
    #[serde(default)]
    pub source: String,
}

fn default_format() -> String {
    "jpeg".into()
}

/// On-disk format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DbFile {
    version: u32,
    entries: Vec<WriterEntry>,
}

/// In-memory fingerprint database.
#[derive(Debug, Clone)]
pub struct FingerprintDb {
    entries: HashMap<String, WriterEntry>,
}

impl Default for FingerprintDb {
    fn default() -> Self {
        Self::builtin()
    }
}

impl FingerprintDb {
    /// Empty database.
    pub fn empty() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// The database compiled into the binary.
    pub fn builtin() -> Self {
        Self::from_json(include_str!("fingerprints.json"))
            .expect("built-in fingerprints.json must be valid")
    }

    /// Parse the JSON format.
    pub fn from_json(s: &str) -> Result<Self, String> {
        let f: DbFile = serde_json::from_str(s).map_err(|e| e.to_string())?;
        if f.version != 1 {
            return Err(format!("unsupported fingerprint db version {}", f.version));
        }
        let mut db = Self::empty();
        for e in f.entries {
            db.insert(e);
        }
        Ok(db)
    }

    /// Load from a file and merge over the built-in DB (file entries win).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut db = Self::builtin();
        db.merge(Self::from_json(&s)?);
        Ok(db)
    }

    /// Serialise to the JSON format (sorted for stable diffs).
    pub fn to_json(&self) -> String {
        let mut entries: Vec<WriterEntry> = self.entries.values().cloned().collect();
        entries.sort_by(|a, b| {
            a.writer
                .cmp(&b.writer)
                .then(a.fingerprint.cmp(&b.fingerprint))
        });
        serde_json::to_string_pretty(&DbFile {
            version: 1,
            entries,
        })
        .unwrap_or_default()
    }

    /// Add or replace an entry.
    pub fn insert(&mut self, e: WriterEntry) {
        self.entries.insert(e.fingerprint.clone(), e);
    }

    /// Merge another DB in; its entries win on conflict.
    pub fn merge(&mut self, other: Self) {
        self.entries.extend(other.entries);
    }

    /// Look up a fingerprint.
    pub fn lookup(&self, fingerprint: &str) -> Option<&WriterEntry> {
        self.entries.get(fingerprint)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the DB is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over every entry, in no particular order.
    pub fn entries(&self) -> impl Iterator<Item = &WriterEntry> {
        self.entries.values()
    }

    /// Build an entry from a parsed structure. This is how the DB grows: run it over
    /// files whose writer you *know*, review, and commit the JSON.
    pub fn harvest(
        s: &JpegStructure,
        writer: impl Into<String>,
        class: WriterClass,
        source: impl Into<String>,
    ) -> WriterEntry {
        WriterEntry {
            fingerprint: s.fingerprint(),
            format: "jpeg".into(),
            writer: writer.into(),
            class,
            notes: format!(
                "{} subsampling, {} tables, {:?} huffman, ids={}, progressive={}, jfif={}, adobe={}",
                s.subsampling(),
                s.tables.len(),
                s.huffman_class(),
                s.component_id_style(),
                s.progressive,
                s.has_jfif,
                s.has_adobe
            ),
            source: source.into(),
        }
    }

    /// Build an entry from a parsed PNG.
    pub fn harvest_png(
        info: &PngInfo,
        writer: impl Into<String>,
        class: WriterClass,
        source: impl Into<String>,
    ) -> WriterEntry {
        let mut chunks: Vec<&str> = Vec::new();
        for c in &info.chunks {
            if c == "IDAT" && chunks.last() == Some(&"IDAT") {
                continue;
            }
            chunks.push(c);
        }
        WriterEntry {
            fingerprint: info.fingerprint(),
            format: "png".into(),
            writer: writer.into(),
            class,
            notes: format!(
                "chunks {}, depth {}, colour type {}, text keys [{}]{}",
                chunks.join(" "),
                info.bit_depth,
                info.color_type,
                info.text
                    .iter()
                    .map(|t| t.keyword.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                match &info.icc_name {
                    Some(n) => format!(", ICC `{n}`"),
                    None => String::new(),
                }
            ),
            source: source.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_parses() {
        let db = FingerprintDb::builtin();
        // May legitimately be small, but must parse and dedupe.
        assert!(db.len() <= 1000);
    }

    #[test]
    fn roundtrip_and_lookup() {
        let mut db = FingerprintDb::empty();
        db.insert(WriterEntry {
            fingerprint: "ab".repeat(32),
            format: "jpeg".into(),
            writer: "Test Writer".into(),
            class: WriterClass::Camera,
            notes: String::new(),
            source: "unit test".into(),
        });
        let back = FingerprintDb::from_json(&db.to_json()).unwrap();
        assert_eq!(back.lookup(&"ab".repeat(32)).unwrap().writer, "Test Writer");
        assert!(back.lookup("nope").is_none());
    }
}
