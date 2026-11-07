//! The parts a file declares itself to consist of: named byte ranges, read from
//! the file's own description of its structure: the Motion Photo / Ultra HDR
//! container directory ([`crate::gcontainer`]) first, then the MPF index
//! ([`crate::mpf`]). A Pixel Ultra HDR file declares its gain map in both; an
//! iPhone JPEG only in MPF. Callers that take the first declaration at a place
//! (C2PA part naming) therefore see the container directory's name wherever it
//! has one, exactly as before MPF was read.
//!
//! A declaration is what the file's writer said, unsigned. Callers use it to name
//! things (a C2PA part at the same place, for instance), never as evidence that
//! the bytes there are what the name says.

use crate::gcontainer::{self, Layout};
use crate::mpf;

/// One byte range the file declares, with the name its declaration gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredPart {
    /// Where the part starts, in bytes from the start of the file.
    pub offset: u64,
    /// The part's declared length in bytes.
    pub length: u64,
    /// The declaration's own name for it, e.g. `MotionPhoto` (see [`describe`]).
    pub name: String,
    /// Which declaration it comes from, e.g. `container directory`.
    pub declared_by: &'static str,
}

/// What a declared part's name means, in plain words: `MotionPhoto` is a "motion
/// clip", MPF's `Undefined` an "image of undefined MPF type". Unknown names are
/// returned as written.
pub fn describe(name: &str) -> &str {
    match name {
        "Primary" => "primary image",
        "GainMap" => "gain map",
        "MotionPhoto" => "motion clip",
        other => mpf::describe(other),
    }
}

/// Every part the file declares: the container directory's parts in file order,
/// then the MPF index's in index order. Empty when the file declares none (most
/// files); a declaration that cannot be read contributes nothing.
pub fn declared_parts(file: &[u8]) -> Vec<DeclaredPart> {
    let mut out = match gcontainer::analyze(file) {
        Ok(Some(layout)) => parts_of(&layout),
        _ => Vec::new(),
    };
    if let Ok(Some(index)) = mpf::index(file) {
        out.extend(mpf_parts(&index));
    }
    out
}

/// The parts an MPF index declares, at the places it gives them. Entries of zero
/// size declare no range.
pub fn mpf_parts(index: &mpf::Index) -> Vec<DeclaredPart> {
    index
        .entries
        .iter()
        .filter(|e| e.size > 0)
        .map(|e| DeclaredPart {
            offset: e.offset,
            length: e.size,
            name: e.type_name.to_string(),
            declared_by: "MPF index",
        })
        .collect()
}

/// The parts a container directory declares, placed by its layout rule. The
/// primary image spans the primary encoding; when the directory gives it padding,
/// a second entry spans encoding plus padding, since a writer may count the padding
/// with either neighbour. Items sharing the previous item's bytes (`Length` 0) add
/// no range of their own.
pub fn parts_of(layout: &Layout) -> Vec<DeclaredPart> {
    const BY: &str = "container directory";
    let mut out = Vec::new();
    if let Some(first) = layout
        .directory
        .items
        .first()
        .filter(|i| i.semantic == "Primary")
    {
        out.push(DeclaredPart {
            offset: 0,
            length: layout.primary_end,
            name: first.semantic.clone(),
            declared_by: BY,
        });
        if let Some(pad) = first.padding.filter(|p| *p > 0) {
            out.push(DeclaredPart {
                offset: 0,
                length: layout.primary_end.saturating_add(pad),
                name: first.semantic.clone(),
                declared_by: BY,
            });
        }
    }
    for p in layout.placements.iter().filter(|p| p.length > 0) {
        out.push(DeclaredPart {
            offset: p.offset,
            length: p.length,
            name: p.semantic.clone(),
            declared_by: BY,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcontainer::{Directory, Item, resolve};

    fn item(semantic: &str, length: Option<u64>, padding: Option<u64>) -> Item {
        Item {
            semantic: semantic.into(),
            mime: "x/y".into(),
            length,
            padding,
        }
    }

    fn summary(parts: &[DeclaredPart]) -> Vec<(u64, u64, &str)> {
        parts
            .iter()
            .map(|p| (p.offset, p.length, p.name.as_str()))
            .collect()
    }

    #[test]
    fn the_fold_motion_photo_declares_three_parts() {
        let dir = Directory {
            items: vec![
                item("Primary", Some(0), None),
                item("GainMap", Some(9_718), None),
                item("MotionPhoto", Some(3_296_847), None),
            ],
        };
        let l = resolve(dir, 2_047_094, &[]);
        assert_eq!(
            summary(&parts_of(&l)),
            [
                (0, 2_047_094, "Primary"),
                (2_047_094, 9_718, "GainMap"),
                (2_056_812, 3_296_847, "MotionPhoto")
            ]
        );
        assert!(
            parts_of(&l)
                .iter()
                .all(|p| p.declared_by == "container directory")
        );
    }

    #[test]
    fn primary_padding_declares_both_spans_and_shifts_the_rest() {
        let dir = Directory {
            items: vec![
                item("Primary", Some(0), Some(8)),
                item("MotionPhoto", Some(64), None),
            ],
        };
        let l = resolve(dir, 1_000, &[]);
        assert_eq!(
            summary(&parts_of(&l)),
            [
                (0, 1_000, "Primary"),
                (0, 1_008, "Primary"),
                (1_008, 64, "MotionPhoto")
            ]
        );
    }

    #[test]
    fn shared_items_add_no_range_and_a_missing_primary_adds_no_primary() {
        let dir = Directory {
            items: vec![
                item("Primary", Some(0), None),
                item("MotionPhoto", Some(64), None),
                item("Depth", Some(0), None),
            ],
        };
        assert_eq!(
            summary(&parts_of(&resolve(dir, 100, &[]))),
            [(0, 100, "Primary"), (100, 64, "MotionPhoto")]
        );
        let odd = Directory {
            items: vec![item("GainMap", Some(10), None)],
        };
        assert!(
            parts_of(&resolve(odd, 100, &[]))
                .iter()
                .all(|p| p.name != "Primary")
        );
    }

    #[test]
    fn files_without_a_readable_declaration_declare_nothing() {
        assert!(declared_parts(b"not a jpeg").is_empty());
        assert!(declared_parts(&[0xFF, 0xD8, 0xFF, 0xD9]).is_empty());
    }

    #[test]
    fn names_read_as_plain_words() {
        assert_eq!(describe("MotionPhoto"), "motion clip");
        assert_eq!(describe("GainMap"), "gain map");
        assert_eq!(describe("Undefined"), "image of undefined MPF type");
        assert_eq!(describe("Depth"), "Depth");
    }

    #[test]
    fn mpf_entries_become_parts_at_their_file_offsets() {
        let e = |index, type_code, size, offset| mpf::Entry {
            index,
            attribute: type_code,
            type_code,
            type_name: mpf::type_name(type_code),
            data_format: 0,
            dependent_parent: false,
            dependent_child: false,
            representative: index == 0,
            size,
            raw_offset: 0,
            offset,
            dependents: [0, 0],
        };
        let index = mpf::Index {
            header_offset: 10,
            big_endian: true,
            version: Some("0100".into()),
            number_of_images: Some(3),
            entries: vec![
                e(0, 0x030000, 1_000, 0),
                e(1, 0, 200, 1_000),
                e(2, 0, 0, 1_200),
            ],
            extra_segments: 0,
        };
        let parts = mpf_parts(&index);
        assert_eq!(
            summary(&parts),
            [
                (0, 1_000, "Baseline MP Primary Image"),
                (1_000, 200, "Undefined")
            ]
        );
        assert!(parts.iter().all(|p| p.declared_by == "MPF index"));
    }
}
