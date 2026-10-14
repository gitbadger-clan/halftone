//! Certificate shelf life of a C2PA signature (DIFFERENTIAL.md D-010, D-012).
//!
//! A verifier judges the signing certificate's validity window at one moment: the
//! time a trusted time-stamp fixes, or, without one, the time of checking. A file
//! whose signature carries no trusted time-stamp therefore validates until the
//! certificate's `notAfter` and reads `signingCredential.expired` after it, with
//! not a byte changed. Which time-stamps count as trusted depends on the TSA list
//! the verifier loads, so everything here is stated relative to that list: the date
//! belongs to the verifier, not to the file, and no text here says a file
//! "expires".
//!
//! Where the time-stamp lives: a token in the COSE signature's unprotected header
//! (`sigTst` / `sigTst2`). c2pa reports it through `validation_results` for the
//! active manifest: `timeStamp.validated` / `timeStamp.trusted` as success codes,
//! `timeStamp.untrusted` as informational, so the failures-only
//! `validation_status` never holds them. The `c2pa.time-stamp` assertion is a
//! different structure (time-stamps for other manifests, as in an update manifest)
//! and says nothing about the active signature.
//!
//! The status, serial formatting and rationale wording build without the `c2pa`
//! feature, so their tests run on every build; reading the certificate needs
//! `x509-parser`, which comes with the feature.

/// Time-stamp status of the active manifest's signature, as c2pa reported it under
/// the TSA list it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeStampStatus {
    /// `timeStamp.trusted`: the time-stamp's authority chains to the loaded TSA list,
    /// so the signing time is fixed and the certificate is judged at that time.
    Trusted,
    /// `timeStamp.untrusted`: a time-stamp is present, but its authority is not on the
    /// loaded TSA list, so it does not fix the signing time here.
    Untrusted,
    /// Another `timeStamp.*` code and no trust verdict (`timeStamp.mismatch`,
    /// `timeStamp.malformed`, `timeStamp.outsideValidity`, …): a time-stamp is
    /// present and did not validate.
    Failed,
    /// `timeStamp.validated` alone: the token validated, but no verdict on its
    /// authority was reported.
    ValidatedOnly,
    /// No `timeStamp.*` code: the signature carries no time-stamp.
    NoTimeStamp,
}

impl TimeStampStatus {
    /// Derive the status from the active manifest's codes, all three buckets
    /// (success, informational, failure); codes outside `timeStamp.*` are ignored.
    ///
    /// Precedence: trusted, untrusted, any other code but `timeStamp.validated`,
    /// `timeStamp.validated` alone, none. `scripts/cert-shelf-life.sh` uses the same
    /// order, so the two agree on every combination.
    pub fn from_codes<'a, I>(codes: I) -> Self
    where
        I: IntoIterator<Item = &'a str>,
    {
        let ts: Vec<&str> = codes
            .into_iter()
            .filter(|c| c.starts_with("timeStamp."))
            .collect();
        if ts.contains(&"timeStamp.trusted") {
            Self::Trusted
        } else if ts.contains(&"timeStamp.untrusted") {
            Self::Untrusted
        } else if ts.iter().any(|c| *c != "timeStamp.validated") {
            Self::Failed
        } else if ts.is_empty() {
            Self::NoTimeStamp
        } else {
            Self::ValidatedOnly
        }
    }

    /// Name used in `details.time_stamp.status` and the batch row.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
            Self::Failed => "failed",
            Self::ValidatedOnly => "validated_only",
            Self::NoTimeStamp => "none",
        }
    }
}

/// The certificate that signed the active manifest: what a later check depends on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SigningCert {
    /// Serial number in uppercase hex without leading zero bytes, as
    /// `openssl x509 -serial` prints it.
    pub serial: String,
    /// The same serial in decimal, as c2pa prints `signature_info.cert_serial_number`.
    pub serial_decimal: String,
    /// Start of the validity window, RFC 3339 UTC.
    pub not_before: String,
    /// End of the validity window, RFC 3339 UTC.
    pub not_after: String,
}

/// A DER serial's content bytes as uppercase hex, leading zero bytes dropped (DER
/// adds one when the high bit is set; `openssl` does not print it). An all-zero
/// serial keeps one byte.
pub fn serial_hex(raw: &[u8]) -> String {
    let start = raw
        .iter()
        .position(|b| *b != 0)
        .unwrap_or(raw.len().saturating_sub(1));
    hex::encode_upper(&raw[start..])
}

/// Read the signing certificate from the PEM chain c2pa reports for the active
/// manifest (`SignatureInfo::cert_chain`, leaf first). The caller cross-checks the
/// serial against c2pa's own, so a chain in another order cannot report another
/// certificate's window.
#[cfg(feature = "c2pa")]
pub fn leaf_from_pem(chain: &str) -> Result<SigningCert, String> {
    use x509_parser::pem::Pem;
    let pem = Pem::iter_from_buffer(chain.as_bytes())
        .next()
        .ok_or_else(|| "the signature carries no certificate chain".to_string())?
        .map_err(|e| format!("the certificate chain is not readable PEM: {e}"))?;
    let cert = pem
        .parse_x509()
        .map_err(|e| format!("the signing certificate does not parse: {e}"))?;
    let raw = cert.raw_serial();
    let validity = cert.validity();
    Ok(SigningCert {
        serial: serial_hex(raw),
        serial_decimal: x509_parser::num_bigint::BigUint::from_bytes_be(raw).to_string(),
        not_before: halftone_core::rfc3339_from_unix(validity.not_before.timestamp()),
        not_after: halftone_core::rfc3339_from_unix(validity.not_after.timestamp()),
    })
}

/// Sentence appended to a `Trusted` or `Valid` rationale: when nothing fixes the
/// signing time under the loaded TSA list, the time after which a check reports
/// the certificate expired. `None` when a trusted time-stamp fixes it.
///
/// `ts_time` is the time the token states (`details.signed_at`), trusted or not.
pub fn validity_note(
    cert: &SigningCert,
    ts: TimeStampStatus,
    ts_time: Option<&str>,
) -> Option<String> {
    let after = &cert.not_after;
    match ts {
        TimeStampStatus::Trusted => None,
        TimeStampStatus::NoTimeStamp => Some(format!(
            " The signature carries no time-stamp, so nothing fixes when it was made; a \
             conformant verifier checking after {after} reports the signing certificate \
             expired."
        )),
        TimeStampStatus::Untrusted => Some(format!(
            " Its time-stamp's authority is not on the loaded TSA list, so the time-stamp \
             does not fix the signing time here; under this list a check after {after} \
             reports the signing certificate expired.{}",
            tsa_alternative(ts_time)
        )),
        TimeStampStatus::Failed => Some(format!(
            " Its time-stamp did not validate, so it does not fix the signing time; a check \
             after {after} reports the signing certificate expired."
        )),
        TimeStampStatus::ValidatedOnly => Some(format!(
            " Its time-stamp validated but no verdict on its authority was reported, so it \
             is not relied on to fix the signing time; a check after {after} may report the \
             signing certificate expired."
        )),
    }
}

/// What follows "but the signing certificate" in the expired rationale: the end of
/// the window when known, and why nothing placed the signature inside it. `ts` is
/// `None` when the time-stamp status could not be read; the clause is then left
/// out rather than guessed.
pub fn expired_phrase(cert: Option<&SigningCert>, ts: Option<TimeStampStatus>) -> String {
    let base = match cert {
        Some(c) => format!("has expired (its validity ended {})", c.not_after),
        None => "has expired".to_string(),
    };
    let why = match ts {
        Some(TimeStampStatus::NoTimeStamp) => " and the signature carries no time-stamp",
        Some(TimeStampStatus::Untrusted) => {
            " and its time-stamp's authority is not on the loaded TSA list"
        }
        Some(TimeStampStatus::Failed) => " and its time-stamp did not validate",
        Some(TimeStampStatus::ValidatedOnly) => {
            " and no verdict on its time-stamp's authority was reported"
        }
        Some(TimeStampStatus::Trusted) | None => "",
    };
    format!("{base}{why}")
}

/// For an untrusted time-stamp: the answer a verifier with a different TSA list
/// gives. Empty when the token states no time.
pub fn tsa_alternative(ts_time: Option<&str>) -> String {
    match ts_time {
        Some(t) => format!(
            " A verifier that trusts this time-stamp's authority judges the certificate as \
             of {t} instead."
        ),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canva_october() -> SigningCert {
        SigningCert {
            serial: "1D529F6E94B0FF2F728E5A20F5E91098A3C0CEF5".into(),
            serial_decimal: "167403281942339131879710527931510703633591226101".into(),
            not_before: "2026-10-07T23:53:55Z".into(),
            not_after: "2026-10-15T23:53:55Z".into(),
        }
    }

    #[test]
    fn status_follows_the_codes() {
        use TimeStampStatus as T;
        assert_eq!(T::from_codes(Vec::<&str>::new()), T::NoTimeStamp);
        assert_eq!(
            T::from_codes(["claimSignature.validated", "signingCredential.untrusted"]),
            T::NoTimeStamp
        );
        assert_eq!(
            T::from_codes(["timeStamp.validated", "timeStamp.trusted"]),
            T::Trusted
        );
        // D-012: Microsoft's time-stamp on the Bing files.
        assert_eq!(
            T::from_codes(["timeStamp.validated", "timeStamp.untrusted"]),
            T::Untrusted
        );
        assert_eq!(T::from_codes(["timeStamp.mismatch"]), T::Failed);
        assert_eq!(
            T::from_codes(["timeStamp.validated", "timeStamp.outsideValidity"]),
            T::Failed
        );
        assert_eq!(T::from_codes(["timeStamp.validated"]), T::ValidatedOnly);
    }

    #[test]
    fn precedence_matches_the_script() {
        use TimeStampStatus as T;
        assert_eq!(
            T::from_codes(["timeStamp.outsideValidity", "timeStamp.trusted"]),
            T::Trusted
        );
        assert_eq!(
            T::from_codes(["timeStamp.mismatch", "timeStamp.untrusted"]),
            T::Untrusted
        );
    }

    #[test]
    fn status_names_are_stable() {
        use TimeStampStatus as T;
        let names: Vec<&str> = [
            T::Trusted,
            T::Untrusted,
            T::Failed,
            T::ValidatedOnly,
            T::NoTimeStamp,
        ]
        .iter()
        .map(|t| t.as_str())
        .collect();
        assert_eq!(
            names,
            ["trusted", "untrusted", "failed", "validated_only", "none"]
        );
    }

    #[test]
    fn serial_drops_der_sign_byte() {
        assert_eq!(serial_hex(&[0x00, 0x9D, 0x52, 0x9F]), "9D529F");
        assert_eq!(serial_hex(&[0x1D, 0x52]), "1D52");
        assert_eq!(serial_hex(&[0x00, 0x00]), "00");
        assert_eq!(serial_hex(&[0x01]), "01");
        assert_eq!(serial_hex(&[]), "");
    }

    #[test]
    fn no_time_stamp_names_the_date_for_every_verifier() {
        let n = validity_note(&canva_october(), TimeStampStatus::NoTimeStamp, None).unwrap();
        assert!(n.contains("carries no time-stamp"));
        assert!(n.contains("after 2026-10-15T23:53:55Z"));
        assert!(n.contains("conformant verifier"));
    }

    #[test]
    fn untrusted_time_stamp_is_relative_to_the_list() {
        let n = validity_note(
            &canva_october(),
            TimeStampStatus::Untrusted,
            Some("2026-10-01T08:00:00+00:00"),
        )
        .unwrap();
        assert!(n.contains("under this list"));
        assert!(n.contains("as of 2026-10-01T08:00:00+00:00"));
        // No token time, no alternative sentence.
        let n = validity_note(&canva_october(), TimeStampStatus::Untrusted, None).unwrap();
        assert!(!n.contains("as of"));
    }

    #[test]
    fn trusted_time_stamp_needs_no_note() {
        assert_eq!(
            validity_note(&canva_october(), TimeStampStatus::Trusted, Some("x")),
            None
        );
    }

    /// principles.md: the date belongs to the verifier; no text says a file expires.
    #[test]
    fn no_wording_says_the_file_expires() {
        use TimeStampStatus as T;
        let c = canva_october();
        for ts in [
            T::Trusted,
            T::Untrusted,
            T::Failed,
            T::ValidatedOnly,
            T::NoTimeStamp,
        ] {
            let note = validity_note(&c, ts, Some("2026-10-01T00:00:00Z")).unwrap_or_default();
            let phrase = expired_phrase(Some(&c), Some(ts));
            for text in [note.as_str(), phrase.as_str()] {
                assert!(!text.contains("expires"), "{ts:?}: {text}");
                assert!(!text.contains("file expire"), "{ts:?}: {text}");
            }
        }
    }

    #[test]
    fn expired_phrase_names_window_and_cause() {
        let c = canva_october();
        assert_eq!(
            expired_phrase(Some(&c), Some(TimeStampStatus::NoTimeStamp)),
            "has expired (its validity ended 2026-10-15T23:53:55Z) and the signature \
             carries no time-stamp"
        );
        assert_eq!(
            expired_phrase(None, Some(TimeStampStatus::Untrusted)),
            "has expired and its time-stamp's authority is not on the loaded TSA list"
        );
        // Status unreadable: no cause is claimed.
        assert_eq!(expired_phrase(None, None), "has expired");
    }

    /// Leaf + root, generated with `openssl ca`; the leaf has a fixed 8-day window
    /// and a serial with the high bit set, so its DER encoding carries a sign byte.
    /// `openssl x509 -serial -dates` on the leaf prints
    /// serial=9D529F6E94B0FF2F728E5A20F5E91098,
    /// notBefore=Oct  7 23:53:55 2026 GMT, notAfter=Oct 15 23:53:55 2026 GMT.
    #[cfg(feature = "c2pa")]
    const TEST_CHAIN: &str = "-----BEGIN CERTIFICATE-----
MIIB4jCCAYigAwIBAgIRAJ1Sn26UsP8vco5aIPXpEJgwCgYIKoZIzj0EAwIwNTEb
MBkGA1UEAwwSSGFsZnRvbmUgVGVzdCBSb290MRYwFAYDVQQKDA1IYWxmdG9uZSBU
ZXN0MB4XDTI2MTAwNzIzNTM1NVoXDTI2MTAxNTIzNTM1NVowNzEdMBsGA1UEAwwU
SGFsZnRvbmUgVGVzdCBTaWduZXIxFjAUBgNVBAoMDUhhbGZ0b25lIFRlc3QwWTAT
BgcqhkjOPQIBBggqhkjOPQMBBwNCAAQFkIM4/ItshZ0k2IUeEaVDL0y/30jVYUOV
/2lg8w45Bd0qFQBcN7SM3eBrYklKsymdzV0hlKXRPYhUC9187Kg7o3cwdTAMBgNV
HRMBAf8EAjAAMA4GA1UdDwEB/wQEAwIHgDAVBgNVHSUEDjAMBgorBgEEAYPoXgIB
MB0GA1UdDgQWBBQu9V9Q5hdFxoyh/9KRHIr8YwwaNDAfBgNVHSMEGDAWgBQ1NJPs
hxMilFxTDwpeurZ6bXcKYzAKBggqhkjOPQQDAgNIADBFAiEA+Smzc++92mLsYboF
lIJbOcAMUpdyPBtbtOtqHR9thQcCIG7FF+P191nqu2o6JjDFMUL9xcWHu/+WRehq
GlUdEf4/
-----END CERTIFICATE-----
-----BEGIN CERTIFICATE-----
MIIBnDCCAUGgAwIBAgIBATAKBggqhkjOPQQDAjA1MRswGQYDVQQDDBJIYWxmdG9u
ZSBUZXN0IFJvb3QxFjAUBgNVBAoMDUhhbGZ0b25lIFRlc3QwHhcNMjYwMTAxMDAw
MDAwWhcNMzYwMTAxMDAwMDAwWjA1MRswGQYDVQQDDBJIYWxmdG9uZSBUZXN0IFJv
b3QxFjAUBgNVBAoMDUhhbGZ0b25lIFRlc3QwWTATBgcqhkjOPQIBBggqhkjOPQMB
BwNCAAThkkQedKhv0ZBm4Z/iw1RtgpIG0rni96DuDJeTHe5oViaSpbHdm8IBYG+b
TWwR0MQtBEQpw0E2ha0Ze2UlEaHEo0IwQDAPBgNVHRMBAf8EBTADAQH/MA4GA1Ud
DwEB/wQEAwICBDAdBgNVHQ4EFgQUNTST7IcTIpRcUw8KXrq2em13CmMwCgYIKoZI
zj0EAwIDSQAwRgIhAPjzEgciWc2hqKvmDFvD7xAdx0QGngvqkLa2spmROxWrAiEA
45yvN2KAUZ2gQ26hsFN34iQWQMGidx9wBCR5eU6MQ8Q=
-----END CERTIFICATE-----
";

    #[cfg(feature = "c2pa")]
    #[test]
    fn leaf_is_read_like_openssl() {
        let c = leaf_from_pem(TEST_CHAIN).unwrap();
        assert_eq!(c.serial, "9D529F6E94B0FF2F728E5A20F5E91098");
        assert_eq!(c.serial_decimal, "209117797344874262572627734460321304728");
        assert_eq!(c.not_before, "2026-10-07T23:53:55Z");
        assert_eq!(c.not_after, "2026-10-15T23:53:55Z");
    }

    #[cfg(feature = "c2pa")]
    #[test]
    fn empty_or_garbage_chain_is_an_error() {
        assert!(leaf_from_pem("").is_err());
        assert!(leaf_from_pem("not a certificate").is_err());
        assert!(
            leaf_from_pem("-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n")
                .is_err()
        );
    }
}
