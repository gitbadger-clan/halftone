//! The parts a file declares itself to consist of: named byte ranges, read from
//! the file's own description of its structure. Today that description is the
//! Motion Photo / Ultra HDR container directory ([`crate::gcontainer`]); another
//! reader (MPF, say) can add its declarations behind the same function.
//!
//! A declaration is what the file's writer said, unsigned. Callers use it to name
//! things (a C2PA part at the same place, for instance), never as evidence that
//! the bytes there are what the name says.

use crate::gcontainer::{self, Layout};

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
/// clip". Unknown names are returned as written.
pub fn describe(name: &str) -> &str {
    match name {
        "Primary" => "primary image",
        "GainMap" => "gain map",
        "MotionPhoto" => "motion clip",
        other => other,
    }
}

/// Every part the file declares, in file order. Empty when the file declares none
/// (most files) or its declaration cannot be read.
pub fn declared_parts(file: &[u8]) -> Vec<DeclaredPart> {
    match gcontainer::analyze(file) {
        Ok(Some(layout)) => parts_of(&layout),
        _ => Vec::new(),
    }
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
        assert_eq!(describe("Depth"), "Depth");
    }
}
