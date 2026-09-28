# /// script
# requires-python = ">=3.14"
# dependencies = ["cbor2>=5.6", "cryptography>=43"]
# ///
"""Extract stapled OCSP responses from a C2PA file and show what c2pa 0.91 checks.

A signer can staple an OCSP response into the claim signature, a COSE_Sign1 whose
unprotected header carries  rVals: { ocspVals: [ <DER OCSPResponse>, ... ] }.
c2pa 0.90 reported signingCredential.ocsp.notRevoked for these offline; 0.91 silently
drops a stapled response that fails any of four checks (DIFFERENTIAL.md D-011 addendum):

  1 parse       the DER must parse, and its signature must verify with the responder key
  1b certId     serial + issuer name/key hashes must match the signer's x5chain (new in 0.91)
  2 time        the signing time must fall inside [thisUpdate, nextUpdate]
  3 EKU         the responder certificate must carry OCSP Signing (1.3.6.1.5.5.7.3.9)
                and be valid at the signing time
  4 trust       the responder must chain to the trust anchors (or the signer's CA)

This prints the fields behind each check so you can see which one fails.

    uv run scripts/ocsp-extract.py FILE [FILE ...] [--signed-at 2026-09-01T12:00:00Z] [--out DIR]

Signing time comes from `ht inspect --json` (details.signed_at) unless --signed-at is given.
Container parsing covers JPEG (APP11 JUMBF, reassembled across segments) and PNG (caBX);
other formats fall back to scanning the whole file.
"""

from __future__ import annotations

import argparse
import io
import json
import struct
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import cbor2
import hashlib
from cryptography import x509
from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec, padding, rsa
from cryptography.x509 import ocsp
from cryptography.x509.oid import ExtendedKeyUsageOID

TRUST = Path("crates/halftone-c2pa/trust")


# ---------------------------------------------------------------- containers

def jpeg_jumbf(b: bytes) -> bytes:
    """Reassemble APP11 JUMBF (JPEG XT): segments grouped by box instance En,
    ordered by sequence Z; later segments repeat the 8-byte box header."""
    parts: dict[int, dict[int, bytes]] = {}
    i = 2
    while i + 4 <= len(b) and b[i] == 0xFF:
        marker = b[i + 1]
        if marker in (0xD9, 0xDA):  # EOI / start of scan: no more metadata
            break
        seg_len = struct.unpack(">H", b[i + 2:i + 4])[0]
        seg = b[i + 4:i + 2 + seg_len]
        if marker == 0xEB and seg[:2] == b"JP":
            en, z = struct.unpack(">HI", seg[2:8])
            payload = seg[8:] if z == 1 else seg[16:]
            parts.setdefault(en, {})[z] = payload
        i += 2 + seg_len
    return b"".join(b"".join(p[z] for z in sorted(p)) for _, p in sorted(parts.items()))


def png_jumbf(b: bytes) -> bytes:
    out, i = [], 8
    while i + 8 <= len(b):
        n, typ = struct.unpack(">I4s", b[i:i + 8])
        if typ == b"caBX":
            out.append(b[i + 8:i + 8 + n])
        i += 12 + n
    return b"".join(out)


def manifest_bytes(path: Path) -> tuple[bytes, str]:
    b = path.read_bytes()
    if b[:2] == b"\xff\xd8":
        j = jpeg_jumbf(b)
        if j:
            return j, "jpeg app11"
    if b[:8] == b"\x89PNG\r\n\x1a\n":
        j = png_jumbf(b)
        if j:
            return j, "png caBX"
    return b, "whole file (no container parser)"


def x5chain_before(data: bytes, pos: int) -> list[x509.Certificate]:
    """The signer chain of the COSE_Sign1 holding this ocspVals: the nearest preceding
    x5chain (COSE label 33, CBOR 0x18 0x21) that decodes to DER certificates."""
    j = data.rfind(b"\x18\x21", 0, pos)
    while j >= 0:
        try:
            v = cbor2.CBORDecoder(io.BytesIO(data[j + 2:])).decode()
            certs = [v] if isinstance(v, bytes) else v
            return [x509.load_der_x509_certificate(c) for c in certs]
        except Exception:  # noqa: BLE001 — not an x5chain at this offset
            j = data.rfind(b"\x18\x21", 0, j)
    return []


def stapled_responses(data: bytes) -> list[tuple[bytes, list[x509.Certificate]]]:
    """Every ocspVals array: CBOR text key 'ocspVals' (0x68 + 8 bytes), then the array.
    c2pa-rs uses the first element only (get_ocsp_der), so that is what we take."""
    key, out, start = b"\x68ocspVals", [], 0
    while (i := data.find(key, start)) >= 0:
        start = i + len(key)
        try:
            val = cbor2.CBORDecoder(io.BytesIO(data[start:])).decode()
        except Exception as e:  # noqa: BLE001 — report and continue
            print(f"  ocspVals at offset {i}: CBOR decode failed ({e})")
            continue
        items = val if isinstance(val, list) else [val]
        if items and isinstance(items[0], bytes):
            out.append((items[0], x5chain_before(data, i)))
    return out


def key_bits(c: x509.Certificate) -> bytes:
    """subjectPublicKey BIT STRING contents, the input to an OCSP issuerKeyHash."""
    k = c.public_key()
    if isinstance(k, ec.EllipticCurvePublicKey):
        return k.public_bytes(serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint)
    if isinstance(k, rsa.RSAPublicKey):
        return k.public_bytes(serialization.Encoding.DER, serialization.PublicFormat.PKCS1)
    return k.public_bytes(serialization.Encoding.Raw, serialization.PublicFormat.Raw)


def verify_sig(r: ocsp.OCSPResponse, cert: x509.Certificate) -> bool:
    k = cert.public_key()
    try:
        if isinstance(k, ec.EllipticCurvePublicKey):
            k.verify(r.signature, r.tbs_response_bytes, ec.ECDSA(r.signature_hash_algorithm))
        elif isinstance(k, rsa.RSAPublicKey):
            k.verify(r.signature, r.tbs_response_bytes, padding.PKCS1v15(), r.signature_hash_algorithm)
        else:
            k.verify(r.signature, r.tbs_response_bytes)
        return True
    except (InvalidSignature, TypeError, ValueError):
        return False


# ---------------------------------------------------------------- report

def signed_at(path: Path, ht: str) -> datetime | None:
    try:
        j = json.loads(subprocess.run([ht, "inspect", "--json", str(path)],
                                      capture_output=True, text=True).stdout)
    except (OSError, json.JSONDecodeError):
        return None
    stack = [j]
    while stack:
        o = stack.pop()
        if isinstance(o, dict):
            if isinstance(o.get("signed_at"), str):
                return datetime.fromisoformat(o["signed_at"].replace("Z", "+00:00"))
            stack.extend(o.values())
        elif isinstance(o, list):
            stack.extend(o)
    return None


def trust_certs() -> list[x509.Certificate]:
    out = []
    for name in ("C2PA-TRUST-LIST.pem", "C2PA-TSA-TRUST-LIST.pem"):
        try:
            out += x509.load_pem_x509_certificates((TRUST / name).read_bytes())
        except (OSError, ValueError):
            pass
    return out


def trust_subjects() -> dict[str, set[str]]:
    out = {}
    for name in ("C2PA-TRUST-LIST.pem", "C2PA-TSA-TRUST-LIST.pem"):
        p = TRUST / name
        if p.exists():
            try:
                out[name] = {c.subject.rfc4514_string() for c in x509.load_pem_x509_certificates(p.read_bytes())}
            except ValueError as e:
                print(f"note: could not read {p}: {e}", file=sys.stderr)
    return out


def eku(cert: x509.Certificate) -> list[str]:
    try:
        return [o.dotted_string for o in cert.extensions.get_extension_for_class(x509.ExtendedKeyUsage).value]
    except x509.ExtensionNotFound:
        return []


def report(der: bytes, chain: list[x509.Certificate], when: datetime | None,
           anchors: dict[str, set[str]], anchor_certs: list[x509.Certificate]) -> None:
    ok = lambda b: "ok  " if b else "FAIL"  # noqa: E731
    try:
        r = ocsp.load_der_ocsp_response(der)
    except ValueError as e:
        print(f"  1 parse  FAIL {e}")
        return
    if r.response_status != ocsp.OCSPResponseStatus.SUCCESSFUL:
        print(f"  1 parse  FAIL response status {r.response_status.name}")
        return
    print(f"  1 parse  ok   {len(der)} bytes, cert status {r.certificate_status.name}, "
          f"serial {r.serial_number:x}")
    if r.certificates:
        print(f"           signature {'verifies' if verify_sig(r, r.certificates[0]) else 'DOES NOT verify'} "
              "with the embedded responder key")
    if not chain:
        print("  1b certId ?    no x5chain found before this response")
    else:
        leaf = chain[0]
        issuer = chain[1] if len(chain) > 1 else next((c for c in anchor_certs if c.subject == leaf.issuer), None)
        src = "x5chain" if len(chain) > 1 else ("trust anchors" if issuer else "nowhere")
        if issuer is None:
            print(f"  1b certId FAIL signer's issuer {leaf.issuer.rfc4514_string()} not in x5chain or anchors")
        else:
            h = lambda b: hashlib.new(r.hash_algorithm.name, b).digest()  # noqa: E731
            parts = {"serial": leaf.serial_number == r.serial_number,
                     "nameHash": h(issuer.subject.public_bytes()) == r.issuer_name_hash,
                     "keyHash": h(key_bits(issuer)) == r.issuer_key_hash}
            bad = [k for k, v in parts.items() if not v]
            print(f"  1b certId {ok(not bad)} {r.hash_algorithm.name}, issuer from {src}"
                  + (f"; mismatch: {', '.join(bad)}" if bad else ""))
        print(f"           signer {leaf.subject.rfc4514_string()}, "
              f"valid {leaf.not_valid_before_utc:%Y-%m-%d} → {leaf.not_valid_after_utc:%Y-%m-%d}")
    tu, nu = r.this_update_utc, r.next_update_utc
    print(f"           producedAt {r.produced_at_utc:%Y-%m-%d %H:%M}Z, "
          f"thisUpdate {tu:%Y-%m-%d %H:%M}Z, nextUpdate {nu:%Y-%m-%d %H:%M}Z" if nu else
          f"           producedAt {r.produced_at_utc:%Y-%m-%d %H:%M}Z, thisUpdate {tu:%Y-%m-%d %H:%M}Z, no nextUpdate")
    if when is None:
        print("  2 time   ?    signing time unknown (pass --signed-at)")
    else:
        # c2pa-rs accepts a signing time before thisUpdate, or inside [thisUpdate, nextUpdate]
        # (nextUpdate defaults to producedAt + 24 h when absent)
        end = nu or (r.produced_at_utc + timedelta(hours=24))
        inside = when < tu or tu <= when <= end
        where = "before thisUpdate" if when < tu else ("inside the window" if when <= end else "AFTER nextUpdate")
        print(f"  2 time   {ok(inside)} signed {when:%Y-%m-%d %H:%M}Z, {where}"
              " (only meaningful for the active manifest's response)")
    certs = r.certificates
    if not certs:
        print("  3 EKU    FAIL no responder certificate embedded (0.91 treats the response as unknown)")
        print(f"           responder {r.responder_name.rfc4514_string() if r.responder_name else r.responder_key_hash.hex()}")
        return
    rc = certs[0]
    ekus = eku(rc)
    print(f"  3 EKU    {ok(ExtendedKeyUsageOID.OCSP_SIGNING.dotted_string in ekus)} "
          f"responder EKUs {ekus or 'none'}")
    print(f"           responder {rc.subject.rfc4514_string()}")
    print(f"           valid {rc.not_valid_before_utc:%Y-%m-%d} → {rc.not_valid_after_utc:%Y-%m-%d}"
          + ("" if when is None else
             f" ({'covers' if rc.not_valid_before_utc <= when <= rc.not_valid_after_utc else 'does NOT cover'} signing time)"))
    issuer = rc.issuer.rfc4514_string()
    where = [n for n, subs in anchors.items() if issuer in subs]
    print(f"  4 trust  {'ok  ' if where else '?   '} responder issuer {issuer}")
    print("           " + (f"is an anchor in {', '.join(where)}" if where else
                            "is not an anchor itself; 0.91 then needs it in the signer's x5chain "
                            "(delegated responder) — compare with the signer's issuing CA"))
    for extra in certs[1:]:
        print(f"           also embedded: {extra.subject.rfc4514_string()}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("files", nargs="+", type=Path)
    ap.add_argument("--signed-at", help="UTC signing time, overrides ht")
    ap.add_argument("--ht", default="target/release/ht")
    ap.add_argument("--out", type=Path, help="write each response as <file>.<n>.der")
    a = ap.parse_args()
    anchors, anchor_certs = trust_subjects(), trust_certs()
    if not anchors:
        print(f"note: no trust lists under {TRUST}; check 4 will show '?'", file=sys.stderr)
    for f in a.files:
        data, how = manifest_bytes(f)
        ders = stapled_responses(data)
        when = (datetime.fromisoformat(a.signed_at.replace("Z", "+00:00")) if a.signed_at
                else signed_at(f, a.ht))
        if when and when.tzinfo is None:
            when = when.replace(tzinfo=timezone.utc)
        print(f"== {f}  [{how}]  {len(ders)} stapled response(s)"
              + ("" if len(ders) < 2 else "; the active manifest's is usually the last"))
        for n, (der, chain) in enumerate(ders, 1):
            print(f" -- response {n}")
            report(der, chain, when, anchors, anchor_certs)
            if a.out:
                a.out.mkdir(parents=True, exist_ok=True)
                (a.out / f"{f.name}.{n}.der").write_bytes(der)
    return 0


if __name__ == "__main__":
    sys.exit(main())
