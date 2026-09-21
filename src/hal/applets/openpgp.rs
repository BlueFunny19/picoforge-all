//! OpenPGP applet client — OpenPGP Card 3.4, ykman/GnuPG-compatible over CCID.
//!
//! Covers the management surface a config GUI needs: card status (cardholder,
//! PIN retries, per-slot key presence/algorithm/touch), PIN management (change
//! user/admin PIN, reset code, unblock), touch policy, on-device key generation,
//! cardholder editing, and factory reset. Key import from PEM and the raw PSO
//! sign/decrypt paths (gpg-driven) are out of scope.
//!
//! Management writes need PW3 (admin) verified on the SAME open session — SELECT
//! resets verification — so those ops open, VERIFY PW3, then act on one session.

#![allow(dead_code)]

use crate::error::PFError;
use crate::hal::apdu::{Apdu, CLA_ISO, tlv};
use crate::hal::transport::ccid::CcidSession;

pub const OPENPGP_AID: &[u8] = &[0xD2, 0x76, 0x00, 0x01, 0x24, 0x01];

// Instructions.
const INS_VERIFY: u8 = 0x20;
const INS_CHANGE_REF: u8 = 0x24;
const INS_RESET_RETRY: u8 = 0x2C;
const INS_ACTIVATE: u8 = 0x44;
const INS_GENERATE: u8 = 0x47;
const INS_GET_DATA: u8 = 0xCA;
const INS_PUT_DATA: u8 = 0xDA;
const INS_TERMINATE: u8 = 0xE6;
const INS_GET_VERSION: u8 = 0xF1;

// PIN references.
pub const PW1: u8 = 0x81;
pub const PW3: u8 = 0x83;

// CRT key-slot selectors (GENERATE) and their algo-attribute DO tags.
const CRT_SIG: u8 = 0xB6;
const CRT_DEC: u8 = 0xB8;
const CRT_AUT: u8 = 0xA4;

// Algorithm ids (first byte of an algo-attribute DO).
const ALGO_RSA: u8 = 0x01;
const ALGO_ECDH: u8 = 0x12;
const ALGO_ECDSA: u8 = 0x13;
const ALGO_EDDSA: u8 = 0x16;

pub const DEFAULT_PW1: &str = "123456";
pub const DEFAULT_PW3: &str = "12345678";

// Curve OID bytes (the part after the algo id).
const OID_P256: &[u8] = &[0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];
const OID_P384: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x22];
const OID_P521: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x23];
const OID_K256: &[u8] = &[0x2B, 0x81, 0x04, 0x00, 0x0A];
const OID_BP256: &[u8] = &[0x2B, 0x24, 0x03, 0x03, 0x02, 0x08, 0x01, 0x01, 0x07];
const OID_BP384: &[u8] = &[0x2B, 0x24, 0x03, 0x03, 0x02, 0x08, 0x01, 0x01, 0x0B];
const OID_ED25519: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0xDA, 0x47, 0x0F, 0x01];
const OID_X25519: &[u8] = &[0x2B, 0x06, 0x01, 0x04, 0x01, 0x97, 0x55, 0x01, 0x05, 0x01];

/// The three key slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgpSlot {
    Sig,
    Dec,
    Aut,
}

impl PgpSlot {
    pub fn label(self) -> &'static str {
        match self {
            Self::Sig => "Signature",
            Self::Dec => "Encryption",
            Self::Aut => "Authentication",
        }
    }
    fn crt(self) -> u8 {
        match self {
            Self::Sig => CRT_SIG,
            Self::Dec => CRT_DEC,
            Self::Aut => CRT_AUT,
        }
    }
    /// Algorithm-attribute DO tag (C1/C2/C3) and UIF touch DO tag (D6/D7/D8).
    fn attr_tag(self) -> u16 {
        match self {
            Self::Sig => 0xC1,
            Self::Dec => 0xC2,
            Self::Aut => 0xC3,
        }
    }
    fn uif_tag(self) -> u16 {
        match self {
            Self::Sig => 0xD6,
            Self::Dec => 0xD7,
            Self::Aut => 0xD8,
        }
    }
}

/// Per-slot key metadata parsed from GET DATA.
#[derive(Debug, Clone)]
pub struct PgpKey {
    pub slot: PgpSlot,
    pub present: bool,
    pub algo: String,
    pub fingerprint: String,
    pub touch: bool,
}

/// Aggregated OpenPGP card status.
#[derive(Debug, Clone)]
pub struct PgpInfo {
    pub version: [u8; 3],
    pub serial: u32,
    pub name: String,
    pub login: String,
    pub url: String,
    /// Language preference (5F2D), ISO-639 2-char codes concatenated.
    pub lang: String,
    /// Sex (5F35): `0x31` male, `0x32` female, `0x39` not announced.
    pub sex: u8,
    pub pw1_retries: u8,
    pub rc_retries: u8,
    pub pw3_retries: u8,
    pub keys: Vec<PgpKey>,
}

/// Algorithm choices offered in the generate wizard. `(key, label)`; the DEC
/// slot maps ECDSA→ECDH internally.
pub const GENERATE_ALGOS: &[(&str, u8)] = &[
    ("RSA-2048", 0),
    ("RSA-3072", 1),
    ("RSA-4096", 2),
    ("ECC P-256", 3),
    ("ECC P-384", 4),
    ("ECC P-521", 5),
    ("secp256k1", 6),
    ("brainpoolP256r1", 7),
    ("brainpoolP384r1", 8),
    ("Ed25519 / Cv25519", 9),
];

/// Build the algorithm-attribute bytes for a slot + choice, or `None` if the
/// combination is unsupported (e.g. Ed25519 on the encryption slot).
pub fn algo_attr(slot: PgpSlot, choice: u8) -> Option<Vec<u8>> {
    let ec_id = if slot == PgpSlot::Dec {
        ALGO_ECDH
    } else {
        ALGO_ECDSA
    };
    let ec = |oid: &[u8]| {
        let mut v = vec![ec_id];
        v.extend_from_slice(oid);
        Some(v)
    };
    match choice {
        0 => Some(vec![ALGO_RSA, 0x08, 0x00, 0x00, 0x20, 0x00]),
        1 => Some(vec![ALGO_RSA, 0x0C, 0x00, 0x00, 0x20, 0x00]),
        2 => Some(vec![ALGO_RSA, 0x10, 0x00, 0x00, 0x20, 0x00]),
        3 => ec(OID_P256),
        4 => ec(OID_P384),
        5 => ec(OID_P521),
        6 => ec(OID_K256),
        7 => ec(OID_BP256),
        8 => ec(OID_BP384),
        9 => {
            // 25519: Cv25519 (ECDH) for DEC, Ed25519 (EdDSA) otherwise.
            let mut v = Vec::new();
            if slot == PgpSlot::Dec {
                v.push(ALGO_ECDH);
                v.extend_from_slice(OID_X25519);
            } else {
                v.push(ALGO_EDDSA);
                v.extend_from_slice(OID_ED25519);
            }
            Some(v)
        }
        _ => None,
    }
}

fn algo_label(attr: &[u8]) -> String {
    match attr.first().copied() {
        Some(ALGO_RSA) if attr.len() >= 3 => {
            format!("RSA-{}", u16::from_be_bytes([attr[1], attr[2]]))
        }
        Some(ALGO_ECDH) | Some(ALGO_ECDSA) | Some(ALGO_EDDSA) => curve_label(&attr[1..]),
        _ => "unknown".to_string(),
    }
}

fn curve_label(oid: &[u8]) -> String {
    let name = if oid == OID_P256 {
        "ECC P-256"
    } else if oid == OID_P384 {
        "ECC P-384"
    } else if oid == OID_P521 {
        "ECC P-521"
    } else if oid == OID_K256 {
        "secp256k1"
    } else if oid == OID_BP256 {
        "brainpoolP256r1"
    } else if oid == OID_BP384 {
        "brainpoolP384r1"
    } else if oid == OID_ED25519 {
        "Ed25519"
    } else if oid == OID_X25519 {
        "Cv25519"
    } else {
        "EC"
    };
    name.to_string()
}

// ── Session + reads ─────────────────────────────────────────────────────────

pub fn open() -> Result<CcidSession, PFError> {
    CcidSession::open(OPENPGP_AID)
}

fn get_data(session: &CcidSession, tag: u16) -> Result<Vec<u8>, PFError> {
    session.transceive_full(&Apdu::read(
        CLA_ISO,
        INS_GET_DATA,
        (tag >> 8) as u8,
        tag as u8,
        &[],
    ))
}

pub fn get_version(session: &CcidSession) -> Result<[u8; 3], PFError> {
    let r = session.transceive_full(&Apdu::read(CLA_ISO, INS_GET_VERSION, 0, 0, &[]))?;
    let mut v = [0u8; 3];
    v[..r.len().min(3)].copy_from_slice(&r[..r.len().min(3)]);
    Ok(v)
}

/// Decode packed-BCD serial bytes into a decimal serial number.
fn bcd_serial(b: &[u8]) -> u32 {
    let mut n = 0u32;
    for &byte in b {
        n = n * 100 + (byte >> 4) as u32 * 10 + (byte & 0x0F) as u32;
    }
    n
}

/// Full card status (unauthenticated).
pub fn read_info(session: &CcidSession) -> Result<PgpInfo, PFError> {
    let version = get_version(session).unwrap_or([0; 3]);
    let app = get_data(session, 0x6E)?;
    let app = tlv::find(&app, 0x6E).unwrap_or(&app);

    let serial = tlv::find(app, 0x4F)
        .filter(|a| a.len() >= 14)
        .map(|a| bcd_serial(&a[10..14]))
        .unwrap_or(0);

    let disc = tlv::find(app, 0x73).unwrap_or(app);
    let pw = tlv::find(disc, 0xC4);
    let (pw1_retries, rc_retries, pw3_retries) = pw
        .filter(|p| p.len() >= 7)
        .map(|p| (p[4], p[5], p[6]))
        .unwrap_or((0, 0, 0));

    let fps = tlv::find(disc, 0xC5).unwrap_or(&[]);
    let key_info = key_status(app, disc);

    let mut keys = Vec::new();
    for (i, slot) in [PgpSlot::Sig, PgpSlot::Dec, PgpSlot::Aut]
        .into_iter()
        .enumerate()
    {
        let attr = tlv::find(disc, slot.attr_tag() as u32).unwrap_or(&[]);
        let fp = fps.get(i * 20..i * 20 + 20).unwrap_or(&[]);
        let present =
            fp.iter().any(|&b| b != 0) || key_info.get(i * 2 + 1).map(|&b| b != 0).unwrap_or(false);
        let touch = tlv::find(disc, slot.uif_tag() as u32)
            .and_then(|u| u.first())
            .map(|&b| b != 0)
            .unwrap_or(false);
        keys.push(PgpKey {
            slot,
            present,
            algo: algo_label(attr),
            fingerprint: hex::encode(fp),
            touch,
        });
    }

    // Cardholder: 65 { 5B name, 5F2D lang, 5F35 sex }, login 5E, url 5F50.
    let ch = get_data(session, 0x65).unwrap_or_default();
    let ch = tlv::find(&ch, 0x65).unwrap_or(&ch);
    let name = tlv::find(ch, 0x5B).map(str_of).unwrap_or_default();
    let lang = tlv::find(ch, 0x5F2D).map(str_of).unwrap_or_default();
    let sex = tlv::find(ch, 0x5F35)
        .and_then(|v| v.first().copied())
        .unwrap_or(0x39);
    let login = get_data(session, 0x5E)
        .map(|v| str_of(&v))
        .unwrap_or_default();
    let url = get_data(session, 0x5F50)
        .map(|v| str_of(&v))
        .unwrap_or_default();

    Ok(PgpInfo {
        version,
        serial,
        name,
        login,
        url,
        lang,
        sex,
        pw1_retries,
        rc_retries,
        pw3_retries,
        keys,
    })
}

fn str_of(v: &[u8]) -> String {
    String::from_utf8_lossy(v)
        .trim_end_matches('\0')
        .to_string()
}

// ── PIN management ──────────────────────────────────────────────────────────

pub fn verify_pin(session: &CcidSession, reference: u8, pin: &str) -> Result<(), PFError> {
    session.transceive_full(&Apdu::write(
        CLA_ISO,
        INS_VERIFY,
        0x00,
        reference,
        pin.as_bytes(),
    ))?;
    Ok(())
}

/// Change PW1 (`ref=PW1`) or PW3 (`ref=PW3`). The device splits old/new at the
/// stored PIN length, so send `old ‖ new` concatenated.
pub fn change_pin(
    session: &CcidSession,
    reference: u8,
    old: &str,
    new: &str,
) -> Result<(), PFError> {
    let mut body = old.as_bytes().to_vec();
    body.extend_from_slice(new.as_bytes());
    session.transceive_full(&Apdu::write(
        CLA_ISO,
        INS_CHANGE_REF,
        0x00,
        reference,
        &body,
    ))?;
    Ok(())
}

/// Unblock PW1 with the resetting code (`RC ‖ new_pw1`).
pub fn unblock_with_rc(session: &CcidSession, rc: &str, new_pw1: &str) -> Result<(), PFError> {
    let mut body = rc.as_bytes().to_vec();
    body.extend_from_slice(new_pw1.as_bytes());
    session.transceive_full(&Apdu::write(CLA_ISO, INS_RESET_RETRY, 0x00, PW1, &body))?;
    Ok(())
}

/// Unblock PW1 using a verified admin PIN (call `verify_pin(PW3)` first).
pub fn unblock_with_admin(session: &CcidSession, new_pw1: &str) -> Result<(), PFError> {
    session.transceive_full(&Apdu::write(
        CLA_ISO,
        INS_RESET_RETRY,
        0x02,
        PW1,
        new_pw1.as_bytes(),
    ))?;
    Ok(())
}

/// Set (or clear, if empty) the resetting code — PUT DATA D3 (PW3 first).
pub fn set_reset_code(session: &CcidSession, new_rc: &str) -> Result<(), PFError> {
    put_data(session, 0xD3, new_rc.as_bytes())
}

// ── PUT DATA (PW3-gated writes) ─────────────────────────────────────────────

fn put_data(session: &CcidSession, tag: u16, value: &[u8]) -> Result<(), PFError> {
    session.transceive_full(&Apdu::write(
        CLA_ISO,
        INS_PUT_DATA,
        (tag >> 8) as u8,
        tag as u8,
        value,
    ))?;
    Ok(())
}

pub fn set_cardholder(
    session: &CcidSession,
    name: &str,
    login: &str,
    url: &str,
    lang: &str,
    sex: u8,
) -> Result<(), PFError> {
    put_data(session, 0x5B, name.as_bytes())?;
    put_data(session, 0x5E, login.as_bytes())?;
    put_data(session, 0x5F50, url.as_bytes())?;
    put_data(session, 0x5F2D, lang.as_bytes())?;
    put_data(session, 0x5F35, &[sex])?;
    Ok(())
}

pub fn set_touch(session: &CcidSession, slot: PgpSlot, on: bool) -> Result<(), PFError> {
    put_data(
        session,
        slot.uif_tag(),
        &[if on { 0x01 } else { 0x00 }, 0x20],
    )
}

pub fn set_algo_attr(session: &CcidSession, slot: PgpSlot, attr: &[u8]) -> Result<(), PFError> {
    put_data(session, slot.attr_tag(), attr)
}

/// Set the slot's algorithm then GENERATE a key (returns the `7F49` public key).
pub fn generate(session: &CcidSession, slot: PgpSlot, attr: &[u8]) -> Result<Vec<u8>, PFError> {
    set_algo_attr(session, slot, attr)?;
    session.transceive_full(&Apdu::read(
        CLA_ISO,
        INS_GENERATE,
        0x80,
        0x00,
        &[slot.crt(), 0x00],
    ))
}

// ── Factory reset (block both PINs → TERMINATE → ACTIVATE) ───────────────────

pub fn reset(session: &CcidSession) -> Result<(), PFError> {
    for reference in [PW1, PW3] {
        for _ in 0..10 {
            match session.transceive(&Apdu::write(
                CLA_ISO,
                INS_VERIFY,
                0x00,
                reference,
                b"00000000",
            )) {
                Ok((_, sw)) if sw.0 == 0x6983 => break,
                Ok(_) => continue,
                Err(e) => return Err(e),
            }
        }
    }
    session.transceive_full(&Apdu::write(CLA_ISO, INS_TERMINATE, 0x00, 0x00, &[]))?;
    session.transceive_full(&Apdu::write(CLA_ISO, INS_ACTIVATE, 0x00, 0x00, &[]))?;
    Ok(())
}

// Tag DE is a sibling of discretionary data on Pico All, but some cards nest it.
fn key_status<'a>(app: &'a [u8], disc: &'a [u8]) -> &'a [u8] {
    tlv::find(app, 0xDE)
        .or_else(|| tlv::find(disc, 0xDE))
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_key_without_fingerprint_is_reported_from_application_data() {
        let app = hex::decode("7303c40101de06000101000200").unwrap();
        let disc = tlv::find(&app, 0x73).unwrap();
        assert_eq!(key_status(&app, disc), [0, 1, 1, 0, 2, 0]);
        assert_eq!(
            key_status(&[], &hex::decode("de06000101000200").unwrap()),
            [0, 1, 1, 0, 2, 0]
        );
    }

    #[test]
    fn algo_attr_bytes() {
        assert_eq!(
            algo_attr(PgpSlot::Sig, 0).unwrap(),
            vec![0x01, 0x08, 0x00, 0x00, 0x20, 0x00]
        );
        assert_eq!(
            algo_attr(PgpSlot::Sig, 2).unwrap(),
            vec![0x01, 0x10, 0x00, 0x00, 0x20, 0x00]
        );
        // P-256: ECDSA on SIG, ECDH on DEC, same OID.
        assert_eq!(algo_attr(PgpSlot::Sig, 3).unwrap()[0], ALGO_ECDSA);
        assert_eq!(algo_attr(PgpSlot::Dec, 3).unwrap()[0], ALGO_ECDH);
        assert_eq!(&algo_attr(PgpSlot::Sig, 3).unwrap()[1..], OID_P256);
        // 25519: Ed25519 on SIG, Cv25519 on DEC.
        assert_eq!(algo_attr(PgpSlot::Sig, 9).unwrap()[0], ALGO_EDDSA);
        assert_eq!(algo_attr(PgpSlot::Dec, 9).unwrap()[0], ALGO_ECDH);
    }

    #[test]
    fn labels_from_attrs() {
        assert_eq!(
            algo_label(&[0x01, 0x10, 0x00, 0x00, 0x20, 0x00]),
            "RSA-4096"
        );
        assert_eq!(
            algo_label(&[0x13, 0x2B, 0x81, 0x04, 0x00, 0x22]),
            "ECC P-384"
        );
        assert_eq!(
            algo_label(&[0x16, 0x2B, 0x06, 0x01, 0x04, 0x01, 0xDA, 0x47, 0x0F, 0x01]),
            "Ed25519"
        );
    }

    #[test]
    fn bcd_serial_decode() {
        assert_eq!(bcd_serial(&[0x01, 0x23, 0x45, 0x67]), 1234567);
        assert_eq!(bcd_serial(&[0x00, 0x00, 0x00, 0x00]), 0);
    }

    #[test]
    fn parses_app_data_tree() {
        // Build a minimal 6E { 4F <aid>, 73 { C4 pw, C5 fps, DE keyinfo, C1 attr } }.
        let mut aid = vec![0xD2, 0x76, 0x00, 0x01, 0x24, 0x01, 0x03, 0x04, 0x00, 0x06];
        aid.extend_from_slice(&[0x00, 0x12, 0x34, 0x56]); // serial BCD = 123456
        aid.extend_from_slice(&[0x00, 0x00]);
        let mut disc = Vec::new();
        tlv::write(&mut disc, 0xC1, &[0x01, 0x08, 0x00, 0x00, 0x20, 0x00]);
        tlv::write(&mut disc, 0xC4, &[0x01, 0x7F, 0x7F, 0x7F, 0x03, 0x00, 0x02]);
        tlv::write(&mut disc, 0xC5, &[0u8; 60]); // no fingerprints
        tlv::write(&mut disc, 0xDE, &[0x01, 0x00, 0x02, 0x00, 0x03, 0x00]);
        let mut app = Vec::new();
        tlv::write(&mut app, 0x4F, &aid);
        tlv::write(&mut app, 0x73, &disc);
        let mut full = Vec::new();
        tlv::write(&mut full, 0x6E, &app);

        let a = tlv::find(&full, 0x6E).unwrap();
        assert_eq!(bcd_serial(&tlv::find(a, 0x4F).unwrap()[10..14]), 123456);
        let d = tlv::find(a, 0x73).unwrap();
        let c4 = tlv::find(d, 0xC4).unwrap();
        assert_eq!((c4[4], c4[5], c4[6]), (0x03, 0x00, 0x02)); // pw1/rc/pw3 retries
        assert_eq!(algo_label(tlv::find(d, 0xC1).unwrap()), "RSA-2048");
    }
}
