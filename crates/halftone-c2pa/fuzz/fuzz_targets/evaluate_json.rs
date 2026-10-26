//! Raw-bytes fuzzing of the C2PA 2.4 §15.12.4 procedure: the input is the
//! multi-asset assertion and the assertion store as two JSON documents separated
//! by a NUL byte (which cannot occur inside JSON), as an attacker-written manifest
//! could carry them. Whatever parses must evaluate without panicking, and every
//! report must pass `check_invariants`.
//!
//! Run with the dictionary and seeds, or the fuzzer never learns the key names
//! and stays in the JSON parser:
//!   cargo fuzz run evaluate_json corpus/evaluate_json seeds/evaluate_json -- -dict=evaluate_json.dict
#![no_main]

use halftone_c2pa::multi_asset::{check_invariants, evaluate, part_labels, PartHash, PartHasher};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

/// The part hash itself says the answer: `[1]` match, `[0]` mismatch, anything
/// else an error.
struct Oracle;

impl PartHasher for Oracle {
    fn verify(&self, part: &PartHash, _bytes: &[u8]) -> Result<bool, String> {
        match part.hash.as_slice() {
            [1] => Ok(true),
            [0] => Ok(false),
            _ => Err("unsupported".into()),
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let (a, b) = match data.iter().position(|&c| c == 0) {
        Some(i) => (&data[..i], &data[i + 1..]),
        None => (data, &[][..]),
    };
    let Ok(multi_asset) = serde_json::from_slice::<Value>(a) else {
        return;
    };
    let store = serde_json::from_slice::<Value>(b).unwrap_or(Value::Null);
    let _ = part_labels(&multi_asset);
    for len in [0usize, 1, 9, 50, 64, 4096] {
        let file = vec![0u8; len];
        let report = evaluate(&multi_asset, &store, &file, &Oracle);
        if let Err(e) = check_invariants(&report, len as u64) {
            panic!("{e}\nfile length {len}\n{report:?}");
        }
    }
});
