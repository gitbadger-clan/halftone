//! The report's "What the file says about itself" section, end to end: a file whose
//! XMP every metadata source finds something in, inspected by the real sources, read
//! back from the page. The section is drawn from each source's `details`, which the
//! verdict schema does not define; this test is what notices when a source renames a
//! key and a table would quietly go empty.

use std::path::{Path, PathBuf};
use std::process::Command;

const NAME: &str = "Ada Example";
const NOTICE: &str = "(c) 2026 Ada Example";
/// The free text a data-mining reservation defers to. It names the same person.
const CONSTRAINT: &str = "Ask Ada Example first";

/// One XMP packet with a field for each of the five sources.
fn xmp() -> String {
    format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
 <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
  <rdf:Description rdf:about=\"\"\n\
    xmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
    xmlns:Iptc4xmpExt=\"http://iptc.org/std/Iptc4xmpExt/2008-02-29/\"\n\
    xmlns:plus=\"http://ns.useplus.org/ldf/xmp/1.0/\"\n\
    xmlns:photoshop=\"http://ns.adobe.com/photoshop/1.0/\">\n\
   <Iptc4xmpExt:DigitalSourceType>http://cv.iptc.org/newscodes/digitalsourcetype/trainedAlgorithmicMedia</Iptc4xmpExt:DigitalSourceType>\n\
   <plus:DataMining>http://ns.useplus.org/ldf/vocab/DMI-PROHIBITED-SEECONSTRAINT</plus:DataMining>\n\
   <plus:OtherConstraints>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{CONSTRAINT}</rdf:li>\n    </rdf:Alt>\n   </plus:OtherConstraints>\n\
   <photoshop:Credit>Made with Google AI</photoshop:Credit>\n\
   <dc:creator>\n    <rdf:Seq>\n     <rdf:li>{NAME}</rdf:li>\n    </rdf:Seq>\n   </dc:creator>\n\
   <dc:rights>\n    <rdf:Alt>\n     <rdf:li xml:lang=\"x-default\">{NOTICE}</rdf:li>\n    </rdf:Alt>\n   </dc:rights>\n\
  </rdf:Description>\n\
 </rdf:RDF>\n\
</x:xmpmeta>\n\
<?xpacket end=\"w\"?>"
    )
}

/// A small JPEG with that packet in an APP1 segment right after the start marker.
fn jpeg_with_xmp(path: &Path) {
    let mut plain = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(16, 16, image::Rgb([120, 120, 120]))
        .write_to(&mut plain, image::ImageFormat::Jpeg)
        .unwrap();
    let plain = plain.into_inner();
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(xmp().as_bytes());
    let length = u16::try_from(payload.len() + 2).unwrap();
    let mut out = plain[..2].to_vec(); // SOI
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&plain[2..]);
    std::fs::write(path, out).unwrap();
}

fn scene(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ht-declared-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    let image = dir.join("said.jpg");
    jpeg_with_xmp(&image);
    (dir, image)
}

fn inspect(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_ht"))
        .env("HALFTONE_HOME", dir.join("home"))
        .arg("inspect")
        .args(args)
        .output()
        .expect("run ht");
    assert!(
        matches!(out.status.code(), Some(0 | 2)),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// The part of the page that is the section.
fn section(html: &str) -> &str {
    let start = html
        .find("<details><summary>What the file says about itself")
        .expect("the section is on the page");
    let end = html[start..].find("</details>").unwrap();
    &html[start..start + end]
}

#[test]
fn every_metadata_source_fills_its_table_and_free_text_stays_masked() {
    let (dir, image) = scene("masked");
    let image = image.to_str().unwrap();

    // The premise: all five sources find their field in this file.
    let json: serde_json::Value =
        serde_json::from_str(inspect(&dir, &["--json", "--only", "container", image]).trim())
            .unwrap();
    for name in [
        "marking_metadata",
        "data_mining_reservation",
        "vendor_ai_flag",
        "rights_statements",
        "embedded_identifiers",
    ] {
        let e = json["evidence"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["source"]["name"] == name)
            .unwrap_or_else(|| panic!("{name} did not run"));
        assert_eq!(e["status"], "present", "{name}: {}", e["rationale"]);
    }
    // Masked at the source: the inspection every report is drawn from, page or PDF,
    // does not hold the free text.
    assert!(!json.to_string().contains(NAME));
    assert!(!json.to_string().contains(NOTICE));
    assert!(!json.to_string().contains(CONSTRAINT));

    inspect(&dir, &["--report", "--only", "container", image]);
    let html = std::fs::read_to_string(dir.join("said.halftone.html")).unwrap();
    let s = section(&html);
    for title in [
        "Digital source type (IPTC)",
        "Data-mining reservation (PLUS)",
        "Vendor AI field",
        "Rights statements",
        "Identifiers",
    ] {
        assert!(
            s.contains(&format!("<h3>{title}</h3>")),
            "{title} missing in {s}"
        );
    }
    // A row from each table, as the source reported it.
    assert!(
        s.contains("<td><code>trainedAlgorithmicMedia</code></td>"),
        "{s}"
    );
    assert!(
        s.contains("<td><code>DMI-PROHIBITED-SEECONSTRAINT</code></td>"),
        "{s}"
    );
    // The text that term defers to is free text: its length, not its words.
    assert!(
        s.contains(&format!(
            "<p>Other constraints: masked, {} characters</p>",
            CONSTRAINT.len()
        )),
        "{s}"
    );
    assert!(
        s.contains(
            "<td><code>photoshop:Credit</code></td><td>Made with Google AI</td>\
             <td>the content is AI-generated</td>"
        ),
        "{s}"
    );
    assert!(
        s.contains(&format!(
            "<td><code>dc:rights</code></td><td>copyright notice</td>\
             <td>masked, {} characters</td>",
            NOTICE.len()
        )),
        "{s}"
    );
    assert!(
        s.contains(&format!(
            "<td><code>dc:creator</code></td><td>person</td><td>masked, {} characters</td>",
            NAME.len()
        )),
        "{s}"
    );
    assert!(s.contains("free-text values masked"), "{s}");
    // Masked means absent from the whole page, not only from this section: the
    // evidence table's rationale for the reservation does not quote the text either.
    assert!(!html.contains(NAME), "the name is in a masked report");
    assert!(html.contains("which is masked here (--reveal includes it)"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reveal_puts_the_values_in_the_tables() {
    let (dir, image) = scene("revealed");
    inspect(
        &dir,
        &[
            "--report",
            "--reveal",
            "--only",
            "container",
            image.to_str().unwrap(),
        ],
    );
    let html = std::fs::read_to_string(dir.join("said.halftone.html")).unwrap();
    let s = section(&html);
    assert!(
        s.contains(&format!(
            "<td><code>dc:creator</code></td><td>person</td><td>{NAME}</td>"
        )),
        "{s}"
    );
    assert!(
        s.contains(&format!(
            "<td><code>dc:rights</code></td><td>copyright notice</td><td>{NOTICE}</td>"
        )),
        "{s}"
    );
    assert!(
        s.contains(&format!(
            "<p>Other constraints, as written: {CONSTRAINT}</p>"
        )),
        "{s}"
    );
    assert!(!s.contains("masked"), "{s}");
    // The note at the top of the page says the report now repeats what the file says.
    assert!(html.contains("<li>Values are shown in full."));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "pdf")]
#[test]
fn the_pdf_typesets_the_same_section() {
    let (dir, image) = scene("pdf");
    inspect(
        &dir,
        &["--pdf", "--only", "container", image.to_str().unwrap()],
    );
    // The PDF is drawn from the same model as the page, so this only checks that the
    // template typesets a report whose section has all five tables.
    let pdf = std::fs::read(dir.join("said.halftone.pdf")).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let _ = std::fs::remove_dir_all(&dir);
}
