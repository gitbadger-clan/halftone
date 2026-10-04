//! D-011: why does c2pa 0.91 log `claim.malformed` on Bing's soft-binding assertion?
//!
//! 0.91 added `Claim::verify_soft_binding_alg` (claim.rs). It decodes every
//! `c2pa.soft-binding` assertion with `SoftBinding::from_assertion`, and on any
//! error logs "soft binding assertion could not be decoded" and throws the cause
//! away (`Err(_)`). 0.90 never decoded soft bindings during validation, which is
//! why the code is new in 0.91 regardless of what changed in the type.
//!
//! This probe pulls the raw assertion bytes out of the file's JUMBF, runs the same
//! decode (`c2pa_cbor::from_slice::<SoftBinding>`, as `AssertionCbor` does), and
//! prints what c2pa discards:
//!   1. the exact error,
//!   2. the path of the field that failed,
//!   3. the CBOR shape of the assertion (major type of every field),
//!   4. a control (re-encoded unchanged) and the #2689 test (every text-typed
//!      `blocks[*].value` flipped to a byte string).
//!
//!   cargo run -q --manifest-path tools/d011-softbinding/Cargo.toml -- <file>...

use std::{fs::File, io::Cursor, path::Path};

use anyhow::{Context, Result, bail};
use c2pa::assertions::SoftBinding;
use c2pa_cbor::Value;

/// One soft-binding assertion found in a manifest store.
struct Found {
    manifest: String,
    label: String,
    cbor: Vec<u8>,
}

fn main() -> Result<()> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    if files.is_empty() {
        bail!("usage: d011-softbinding <file>...");
    }
    for f in &files {
        probe(Path::new(f))?;
    }
    Ok(())
}

fn probe(path: &Path) -> Result<()> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .context("file has no extension")?
        .to_ascii_lowercase();
    let mut f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    println!("{}", path.display());
    // No embedded store (remote-only, XMP/IPTC-only, stripped) is a result, not
    // an error, so one run can cover a whole stratum.
    let jumbf = match c2pa::jumbf_io::load_jumbf_from_stream(&ext, &mut f) {
        Ok(j) => j,
        Err(e) => {
            println!("  no embedded C2PA store ({e})\n");
            return Ok(());
        }
    };

    let mut found = Vec::new();
    walk(&jumbf, &mut Vec::new(), &mut found)?;

    if found.is_empty() {
        println!("  no {} assertion", SoftBinding::LABEL);
    }
    for a in &found {
        report(a)?;
    }
    Ok(())
}

fn report(a: &Found) -> Result<()> {
    println!(
        "  {} / {}  ({} bytes of CBOR)",
        a.manifest,
        a.label,
        a.cbor.len()
    );

    // 1. The decode verify_soft_binding_alg runs, with the error kept.
    let exact = c2pa_cbor::from_slice::<SoftBinding>(&a.cbor);
    println!("  c2pa's decode: {}", outcome(&exact));

    // 2. The same typed decode, tracking which field failed. from_slice also
    //    rejects trailing bytes after the CBOR item; this path does not, so a
    //    success here with a failure above points at trailing data.
    if exact.is_err() {
        let mut de = c2pa_cbor::Decoder::new(Cursor::new(a.cbor.as_slice()))
            .with_max_allocation(c2pa_cbor::DEFAULT_MAX_ALLOCATION);
        match serde_path_to_error::deserialize::<_, SoftBinding>(&mut de) {
            Ok(_) => {
                println!("  failing field: none; the fields decode, so look at trailing bytes")
            }
            Err(e) => println!("  failing field: {}  ({})", e.path(), e.inner()),
        }
    }

    // 3. Shape, read generically so it works whatever the typed decode says.
    let mut de = c2pa_cbor::Decoder::new(Cursor::new(a.cbor.as_slice()));
    let v: Value = de
        .decode()
        .context("assertion is not decodable as generic CBOR")?;
    println!("  shape:");
    shape(&v, 4);

    // 4. Control, then the #2689 hypothesis on its own.
    let control = c2pa_cbor::to_vec(&v)?;
    println!(
        "  control, re-encoded unchanged: {}",
        outcome(&c2pa_cbor::from_slice::<SoftBinding>(&control))
    );
    let mut flipped = v.clone();
    let n = flip_block_values(&mut flipped);
    if n == 0 {
        println!("  #2689 test: no text-typed blocks[*].value to flip");
    } else {
        let bytes = c2pa_cbor::to_vec(&flipped)?;
        println!(
            "  #2689 test, {n} blocks[*].value tstr -> bstr: {}",
            outcome(&c2pa_cbor::from_slice::<SoftBinding>(&bytes))
        );
    }
    println!();
    Ok(())
}

fn outcome<T>(r: &c2pa_cbor::Result<T>) -> String {
    match r {
        Ok(_) => "decodes".to_string(),
        Err(e) => format!("FAILS: {e}"),
    }
}

/// Replace every text-typed `blocks[i].value` with a byte string of its UTF-8
/// bytes. Returns how many were replaced.
fn flip_block_values(v: &mut Value) -> usize {
    let Value::Map(m) = v else { return 0 };
    let Some(Value::Array(blocks)) = m.get_mut(&Value::Text("blocks".into())) else {
        return 0;
    };
    let key = Value::Text("value".into());
    let mut n = 0;
    for b in blocks {
        if let Value::Map(bm) = b
            && let Some(Value::Text(s)) = bm.get(&key)
        {
            let bytes = s.clone().into_bytes();
            bm.insert(key.clone(), Value::Bytes(bytes));
            n += 1;
        }
    }
    n
}

fn shape(v: &Value, indent: usize) {
    let pad = " ".repeat(indent);
    let line = |name: String, child: &Value| {
        println!("{pad}{name}: {}", kind(child));
        if matches!(child, Value::Map(_) | Value::Array(_)) {
            shape(child, indent + 2);
        }
    };
    match v {
        Value::Map(m) => {
            for (k, child) in m {
                let name = match k {
                    Value::Text(s) => s.clone(),
                    other => format!("{other:?}"),
                };
                line(name, child);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                line(format!("[{i}]"), child);
            }
        }
        _ => {}
    }
}

fn kind(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => format!("bool {b}"),
        Value::Integer(i) => format!("int {i}"),
        Value::Float(f) => format!("float {f}"),
        Value::Bytes(b) => format!("bstr, {} bytes", b.len()),
        Value::Text(s) => format!("tstr {:?}", s.chars().take(48).collect::<String>()),
        Value::Array(a) => format!("array, {} items", a.len()),
        Value::Map(m) => format!("map, {} keys", m.len()),
        Value::Tag(t, inner) => format!("tag {t} ({})", kind(inner)),
    }
}

// ---- minimal JUMBF walk ------------------------------------------------------

/// Split a buffer into ISO BMFF-style boxes: (type, payload).
fn boxes(mut buf: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        if buf.len() < 8 {
            bail!("truncated box header ({} bytes left)", buf.len());
        }
        let size32 = u32::from_be_bytes(buf[0..4].try_into()?);
        let typ: [u8; 4] = buf[4..8].try_into()?;
        let (header, size) = match size32 {
            0 => (8, buf.len()),
            1 => {
                if buf.len() < 16 {
                    bail!("truncated extended box header");
                }
                (
                    16,
                    usize::try_from(u64::from_be_bytes(buf[8..16].try_into()?))?,
                )
            }
            n => (8, usize::try_from(n)?),
        };
        if size < header || size > buf.len() {
            bail!(
                "box {:?} claims {size} bytes, {} available",
                String::from_utf8_lossy(&typ),
                buf.len()
            );
        }
        out.push((typ, &buf[header..size]));
        buf = &buf[size..];
    }
    Ok(out)
}

/// Label of a JUMBF description box: 16-byte type UUID, one toggles byte, then a
/// null-terminated UTF-8 label if toggles bit 1 is set.
fn jumd_label(payload: &[u8]) -> Option<String> {
    let toggles = *payload.get(16)?;
    if toggles & 0x02 == 0 {
        return None;
    }
    let rest = payload.get(17..)?;
    let end = rest.iter().position(|&b| b == 0)?;
    String::from_utf8(rest[..end].to_vec()).ok()
}

/// Walk superboxes; `path` holds the labels from the store down, so path[1] is
/// the manifest label. Soft-binding assertions match by label prefix, which
/// covers instance suffixes (`__1`) and versions.
fn walk(buf: &[u8], path: &mut Vec<String>, out: &mut Vec<Found>) -> Result<()> {
    for (typ, payload) in boxes(buf)? {
        if &typ != b"jumb" {
            continue;
        }
        let children = boxes(payload)?;
        let label = match children.first() {
            Some((t, d)) if t == b"jumd" => jumd_label(d).unwrap_or_default(),
            _ => continue,
        };
        path.push(label.clone());
        if label.starts_with(SoftBinding::LABEL) {
            for (t, p) in &children[1..] {
                if t == b"cbor" {
                    out.push(Found {
                        manifest: path.get(1).cloned().unwrap_or_default(),
                        label: label.clone(),
                        cbor: p.to_vec(),
                    });
                }
            }
        } else {
            walk(payload, path, out)?;
        }
        path.pop();
    }
    Ok(())
}
