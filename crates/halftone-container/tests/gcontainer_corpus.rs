//! The container directory checked against real phone files in the differential
//! corpus (06-phones, 08-multipart). Ignored by default: the corpus images are not
//! committed. Run with
//!   cargo test -p halftone-container --test gcontainer_corpus -- --ignored --nocapture

use halftone_container::gcontainer::{Magic, analyze};
use std::path::PathBuf;

fn stratum(name: &str) -> Vec<PathBuf> {
    let dir: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "..",
        "..",
        "corpus",
        "differential",
        name,
    ]
    .iter()
    .collect();
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg")))
        .collect();
    v.sort();
    v
}

/// Print one file's directory and what the bytes say; return the disagreements.
fn report(p: &PathBuf) -> Option<Vec<String>> {
    let bytes = std::fs::read(p).unwrap();
    let name = p.file_name().unwrap().to_string_lossy();
    match analyze(&bytes) {
        Err(e) => panic!("{name}: {e}"),
        Ok(None) => {
            println!("{name}: no container directory");
            None
        }
        Ok(Some(l)) => {
            println!(
                "{name}: primary ends at {}, file {} bytes",
                l.primary_end, l.file_len
            );
            for pl in &l.placements {
                let magic = match pl.magic {
                    Magic::Matches => "ok",
                    Magic::Differs => "WRONG BYTES",
                    Magic::NotChecked => "-",
                };
                println!(
                    "    {:<12} {:<11} at {:>9}  {:>9} bytes declared, {:>9} present  {magic}",
                    pl.semantic, pl.mime, pl.offset, pl.length, pl.present_bytes
                );
            }
            for d in &l.disagreements {
                println!("    ! {d}");
            }
            Some(l.disagreements)
        }
    }
}

#[test]
#[ignore = "needs the uncommitted corpus images"]
fn phones_read_without_errors() {
    for p in stratum("06-phones") {
        report(&p);
    }
}

#[test]
#[ignore = "needs the uncommitted corpus images"]
fn multipart_copies_disagree_exactly_where_bytes_were_removed() {
    for p in stratum("08-multipart") {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let d = report(&p).unwrap_or_else(|| panic!("{name}: no directory"));
        let has = |s: &str| d.iter().any(|x| x.contains(s));
        if name.contains("still-only") || name.contains("remove-gainmap-and-clip") {
            assert!(
                has("item 1 (GainMap") && has("item 2 (MotionPhoto"),
                "{name}: {d:?}"
            );
        } else if name.contains("remove-clip") || name.contains("no-clip") {
            assert!(
                has("item 2 (MotionPhoto") && !has("item 1"),
                "{name}: {d:?}"
            );
        } else if name.contains("cut-clip") || name.contains("partial-clip") {
            assert!(has("cut short"), "{name}: {d:?}");
        } else if name.contains("flip") || name.contains("untouched") {
            // A flipped bit inside a part moves nothing: the layout still agrees.
            // (Only the C2PA part hash sees that change.)
            assert!(d.is_empty(), "{name}: {d:?}");
        } else {
            // A new operation must get its own expectation, never pass unchecked.
            panic!("{name}: no expectation for this operation; add a branch");
        }
    }
}
