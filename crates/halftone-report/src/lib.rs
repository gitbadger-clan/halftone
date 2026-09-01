//! Per-file report. Renders an `Inspection` to HTML (and later PDF) with the
//! tool/pack versions, hashes, and the fixed notice that no layer's output is
//! sufficient evidence on its own. The report is what legal/newsroom buyers file.

use halftone_core::Inspection;

/// Fixed notice included in every report.
pub const NOTICE: &str = "Each layer is reported separately with its own calibration. \
No single verdict here is evidence of authorship on its own, and this report must not \
be the sole basis for any disciplinary, employment, or admissions decision.";

/// Render to a minimal HTML string.
pub fn to_html(insp: &Inspection) -> String {
    use std::fmt::Write as _;
    let mut rows = String::new();
    for e in &insp.evidence {
        // Writing to a String cannot fail.
        let _ = write!(
            rows,
            "<tr><td>{:?}</td><td>{} {}</td><td>{:?}</td><td>{}</td></tr>",
            e.layer,
            e.source.name,
            e.source.version,
            e.status,
            html_escape(&e.rationale)
        );
    }
    format!(
        "<!doctype html><meta charset=utf-8><title>Halftone report</title>\
<h1>Halftone report</h1><p>{} {} · schema {} · {}</p><p>sha256 {}</p>\
<table><tr><th>Layer</th><th>Source</th><th>Status</th><th>Rationale</th></tr>{rows}</table>\
<p><small>{}</small></p>",
        insp.tool.name,
        insp.tool.version,
        insp.schema_version,
        insp.created_at,
        insp.asset.sha256,
        NOTICE
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
