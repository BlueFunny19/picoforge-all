//! SmartCard-HSM management for the Pico All applet.
#![allow(dead_code)]
use crate::error::PFError;
use crate::hal::apdu::{Apdu, tlv};
use crate::hal::transport::ccid::CcidSession;

pub const AID: &[u8] = &[
    0xE8, 0x2B, 0x06, 0x01, 0x04, 0x01, 0x81, 0xC3, 0x1F, 0x02, 0x01,
];

#[derive(Debug, Clone)]
pub struct HsmInfo {
    pub version: String,
    pub free_memory: u32,
    pub pin: String,
    pub so_pin: String,
    pub files: Vec<u16>,
    pub initialized: Option<bool>,
}

pub const KEY_ALGORITHMS: &[(&str, u8)] = &[
    ("ECC P-256", 0),
    ("ECC P-384", 1),
    ("RSA-2048", 2),
    ("RSA-3072", 3),
    ("RSA-4096", 4),
    ("AES-128", 5),
    ("AES-192", 6),
    ("AES-256", 7),
];
pub const CRYPTO_OPERATIONS: &[(&str, u8)] = &[
    ("ECDSA / SHA-256 (message)", 0),
    ("ECDSA (digest)", 1),
    ("RSA PKCS#1 / SHA-256 (message)", 2),
    ("RSA PSS / SHA-256 (message)", 3),
    ("RSA PKCS#1 decrypt", 4),
    ("ECDH (peer public key)", 5),
    ("AES-CBC encrypt (zero IV; block-aligned input)", 6),
    ("AES-CBC decrypt (zero IV; block-aligned input)", 7),
    ("AES-CMAC", 8),
];

fn error(msg: &str) -> PFError {
    PFError::Device(msg.into())
}
pub fn open() -> Result<CcidSession, PFError> {
    CcidSession::open(AID)
}

fn pin_metadata(data: &[u8]) -> Result<String, PFError> {
    if data.len() != 4 || data[0] != 1 || data[2] == 0 || data[1] > data[2] || data[3] & !3 != 0 {
        return Err(error("Invalid HSM PIN metadata"));
    }
    let state = if data[3] & 1 == 0 {
        " (not initialized)"
    } else if data[1] == 0 {
        " (blocked)"
    } else if data[3] & 2 != 0 {
        " (default)"
    } else {
        ""
    };
    Ok(format!("{}/{}{}", data[1], data[2], state))
}

fn pin_status(s: &CcidSession, reference: u8) -> Result<(String, Option<bool>), PFError> {
    let (metadata, status) = s.transceive(&Apdu::read(0x80, 0xF7, 0, reference, &[]))?;
    if status.is_ok() {
        return Ok((pin_metadata(&metadata)?, Some(metadata[3] & 1 != 0)));
    }
    // Older firmware exposes only remaining retries; never invent the limit.
    if !matches!(status.0, 0x6D00 | 0x6A86 | 0x6B00 | 0x6E00) {
        return Err(status.to_error());
    }
    let (_, sw) = s.transceive(&Apdu::read(0, 0x20, 0, reference, &[]))?;
    Ok((
        match sw.0 {
            0x9000 => "Verified".into(),
            0x6983 => "Blocked".into(),
            0x6A88 => "Not initialized".into(),
            _ => {
                if let Some(n) = sw.retries_left() {
                    format!("{n}/—")
                } else {
                    return Err(sw.to_error());
                }
            }
        },
        if sw.0 == 0x6A88 { Some(false) } else { None },
    ))
}
pub fn parse_files(data: &[u8]) -> Result<Vec<u16>, PFError> {
    if data.len() % 2 != 0 {
        return Err(error("Truncated HSM file list"));
    }
    let mut files: Vec<_> = data
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .filter(|f| *f != 0)
        .collect();
    files.sort_unstable();
    files.dedup();
    Ok(files)
}
fn list_files(s: &CcidSession) -> Result<Vec<u16>, PFError> {
    parse_files(&s.transceive_full(&Apdu::read(0x80, 0x58, 0, 0, &[]))?)
}
pub fn read_info() -> Result<HsmInfo, PFError> {
    let s = open()?;
    // Empty INITIALIZE is the firmware's read-only memory/version query.
    let r = s.transceive_full(&Apdu::read(0x80, 0x50, 0, 0, &[]))?;
    if r.len() != 7 {
        return Err(error("Invalid HSM version response"));
    }
    let (pin, initialized) = pin_status(&s, 0x81)?;
    Ok(HsmInfo {
        version: format!("{}.{}", r[5], r[6]),
        free_memory: u32::from_be_bytes(r[..4].try_into().unwrap()),
        pin,
        initialized,
        so_pin: pin_status(&s, 0x88)?.0,
        files: list_files(&s)?,
    })
}
fn validate_pin(pin: &[u8]) -> Result<(), PFError> {
    if !(6..=16).contains(&pin.len()) {
        return Err(error("PIN must contain 6 to 16 bytes"));
    }
    Ok(())
}
fn verify(s: &CcidSession, pin: &[u8], reference: u8) -> Result<(), PFError> {
    validate_pin(pin)?;
    s.transceive_full(&Apdu::write(0, 0x20, 0, reference, pin))?;
    Ok(())
}
pub fn change_pin(old: &[u8], new: &[u8], so: bool) -> Result<(), PFError> {
    validate_pin(old)?;
    validate_pin(new)?;
    let s = open()?;
    let mut data = old.to_vec();
    data.extend_from_slice(new);
    s.transceive_full(&Apdu::write(
        0,
        0x24,
        0,
        if so { 0x88 } else { 0x81 },
        &data,
    ))?;
    Ok(())
}
pub fn unblock_pin(so: &[u8], new: &[u8]) -> Result<(), PFError> {
    validate_pin(new)?;
    let s = open()?;
    verify(&s, so, 0x88)?;
    s.transceive_full(&Apdu::write(0, 0x2C, 2, 0x81, new))?;
    Ok(())
}
fn key_id(id: u8) -> Result<(), PFError> {
    if id == 0 {
        return Err(error("Key 00 is reserved for the device identity"));
    }
    Ok(())
}
pub fn key_template(choice: u8) -> Result<Vec<u8>, PFError> {
    let mut public = Vec::new();
    if choice <= 1 {
        tlv::write(&mut public, 6, &[4, 0, 0x7F, 0, 7, 2, 2, 2, 2, 3]);
        let prime=hex::decode(if choice==0 {
            "ffffffff00000001000000000000000000000000ffffffffffffffffffffffff"
        }else{
            "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffeffffffff0000000000000000ffffffff"
        }).unwrap();
        tlv::write(&mut public, 0x81, &prime);
    } else if (2..=4).contains(&choice) {
        tlv::write(&mut public, 6, &[4, 0, 0x7F, 0, 7, 2, 2, 2, 1, 2]);
        tlv::write(
            &mut public,
            2,
            &([2048u16, 3072, 4096][(choice - 2) as usize]).to_be_bytes(),
        );
        tlv::write(&mut public, 0x82, &[1, 0, 1]);
    } else {
        return Err(error("Unsupported asymmetric key algorithm"));
    }
    let mut data = Vec::new();
    tlv::write(&mut data, 0x7F49, &public);
    Ok(data)
}
pub fn next_key_id(files: &[u16]) -> Result<u8, PFError> {
    (1..=255u8)
        .find(|id| {
            !files
                .iter()
                .any(|fid| matches!(fid >> 8, 0xCC | 0xC4 | 0xCE) && *fid as u8 == *id)
        })
        .ok_or_else(|| error("All HSM key slots are occupied. Delete an unused key first."))
}
pub fn generate_auto(pin: &[u8], choice: u8) -> Result<Vec<u8>, PFError> {
    generate_in_slot(pin, None, choice)
}
pub fn generate(pin: &[u8], id: u8, choice: u8) -> Result<Vec<u8>, PFError> {
    key_id(id)?;
    generate_in_slot(pin, Some(id), choice)
}
fn generate_in_slot(pin: &[u8], requested: Option<u8>, choice: u8) -> Result<Vec<u8>, PFError> {
    let template = if choice <= 4 {
        key_template(choice)?
    } else if choice <= 7 {
        Vec::new()
    } else {
        return Err(error("Invalid algorithm"));
    };
    let s = open()?;
    let files = list_files(&s)?;
    let id = match requested {
        Some(id) => id,
        None => next_key_id(&files)?,
    };
    // Never overwrite a key as a side effect of generation.
    if files.contains(&(0xCC00 | id as u16)) {
        return Err(error("Key ID is occupied; choose an unused ID"));
    }
    verify(&s, pin, 0x81)?;
    let cmd = if choice <= 4 {
        Apdu::read(0, 0x46, id, 0, &template)
    } else {
        Apdu::read(0, 0x48, id, 0xB0 + (choice - 5), &[])
    };
    s.send_chained(&cmd)
}
pub fn delete_key(pin: &[u8], id: u8) -> Result<(), PFError> {
    key_id(id)?;
    let s = open()?;
    verify(&s, pin, 0x81)?;
    s.transceive_full(&Apdu::write(0, 0xE4, 0, 0, &[0xCC, id]))?;
    Ok(())
}
pub fn crypto(pin: &[u8], id: u8, operation: u8, data: &[u8]) -> Result<Vec<u8>, PFError> {
    key_id(id)?;
    if data.is_empty() || data.len() > 1800 {
        return Err(error("Input must contain 1 to 1800 bytes"));
    }
    if matches!(operation, 6 | 7) && data.len() % 16 != 0 {
        return Err(error("AES-CBC input must be a multiple of 16 bytes"));
    }
    let (ins, algo) = match operation {
        0 => (0x68, 0x73),
        1 => (0x68, 0x70),
        2 => (0x68, 0x33),
        3 => (0x68, 0x43),
        4 => (0x62, 0x22),
        5 => (0x62, 0x80),
        6 => (0x78, 0x10),
        7 => (0x78, 0x11),
        8 => (0x78, 0x18),
        _ => return Err(error("Unsupported operation")),
    };
    let s = open()?;
    verify(&s, pin, 0x81)?;
    s.send_chained(&Apdu::read(0x80, ins, id, algo, data))
}
fn public_object(fid: u16) -> Result<(), PFError> {
    if !matches!(fid >> 8, 0xC4 | 0xC8 | 0xC9 | 0xCA | 0xCE | 0xCF | 0xCD) {
        return Err(error("Select a certificate, metadata or data object"));
    }
    Ok(())
}
pub fn read_object(pin: &[u8], fid: u16) -> Result<Vec<u8>, PFError> {
    public_object(fid)?;
    let s = open()?;
    if !pin.is_empty() {
        verify(&s, pin, 0x81)?;
    }
    let mut out = Vec::new();
    loop {
        if out.len() > 65535 {
            return Err(error("HSM object exceeds the supported size"));
        }
        let offset = out.len() as u16;
        let mut body = Vec::new();
        tlv::write(&mut body, 0x54, &offset.to_be_bytes());
        let cmd = Apdu::read(0, 0xB1, (fid >> 8) as u8, fid as u8, &body);
        let (chunk, sw) = s.transceive(&cmd)?;
        if !sw.is_ok() && sw.0 != 0x6282 {
            return Err(sw.to_error());
        }
        let n = chunk.len();
        out.extend_from_slice(&chunk);
        if n < 256 || sw.0 == 0x6282 {
            break;
        }
    }
    Ok(out)
}
pub fn delete_object(pin: &[u8], fid: u16) -> Result<(), PFError> {
    public_object(fid)?;
    if fid as u8 == 0 {
        return Err(error("Device identity objects are read-only"));
    }
    let s = open()?;
    verify(&s, pin, 0x81)?;
    s.transceive_full(&Apdu::write(0, 0xE4, 0, 0, &fid.to_be_bytes()))?;
    Ok(())
}
pub const MAX_OBJECT_BYTES: usize = 1800;
pub fn validate_object_data(data: &[u8]) -> Result<(), PFError> {
    if data.is_empty() || data.len() > MAX_OBJECT_BYTES {
        return Err(error("Content must contain 1 to 1,800 bytes."));
    }
    Ok(())
}
pub fn next_object_id(files: &[u16], prefix: u8) -> Result<u16, PFError> {
    public_object((prefix as u16) << 8)?;
    (1..=255u16)
        .map(|id| ((prefix as u16) << 8) | id)
        .find(|fid| {
            !files.contains(fid)
                && (!matches!(prefix, 0xC4 | 0xCE)
                    || !files
                        .iter()
                        .any(|f| matches!(f >> 8, 0xCC | 0xC4 | 0xCE) && (*f as u8 == *fid as u8)))
        })
        .ok_or_else(|| error("All object slots of this type are occupied."))
}
pub fn write_object_auto(pin: &[u8], prefix: u8, data: &[u8]) -> Result<u16, PFError> {
    write_object_in_slot(pin, None, prefix, data)
}
pub fn write_object(pin: &[u8], fid: u16, data: &[u8]) -> Result<(), PFError> {
    write_object_in_slot(pin, Some(fid), (fid >> 8) as u8, data).map(|_| ())
}
fn write_object_in_slot(
    pin: &[u8],
    requested: Option<u16>,
    prefix: u8,
    data: &[u8],
) -> Result<u16, PFError> {
    public_object((prefix as u16) << 8)?;
    validate_object_data(data)?;
    if requested.is_some_and(|id| id as u8 == 0) {
        return Err(error("Device identity objects are read-only"));
    }
    let s = open()?;
    verify(&s, pin, 0x81)?;
    // Allocation and writing share the same card transaction.
    let fid = match requested {
        Some(id) => id,
        None => next_object_id(&list_files(&s)?, prefix)?,
    };
    let mut body = Vec::new();
    tlv::write(&mut body, 0x54, &[0, 0]);
    tlv::write(&mut body, 0x53, data);
    s.send_chained(&Apdu::write(0, 0xD7, (fid >> 8) as u8, fid as u8, &body))?;
    Ok(fid)
}
pub fn wrap_key(pin: &[u8], id: u8) -> Result<Vec<u8>, PFError> {
    key_id(id)?;
    let s = open()?;
    verify(&s, pin, 0x81)?;
    s.transceive_full(&Apdu::read(0x80, 0x72, id, 0x92, &[]))
}
pub fn unwrap_auto(pin: &[u8], data: &[u8]) -> Result<(), PFError> {
    unwrap_in_slot(pin, None, data)
}
pub fn unwrap_key(pin: &[u8], id: u8, data: &[u8]) -> Result<(), PFError> {
    key_id(id)?;
    unwrap_in_slot(pin, Some(id), data)
}
fn unwrap_in_slot(pin: &[u8], requested: Option<u8>, data: &[u8]) -> Result<(), PFError> {
    if data.is_empty() || data.len() > 1800 {
        return Err(error("Wrapped key must contain 1 to 1800 bytes"));
    }
    let s = open()?;
    let files = list_files(&s)?;
    let id = match requested {
        Some(id) => id,
        None => next_key_id(&files)?,
    };
    if files.contains(&(0xCC00 | id as u16)) {
        return Err(error("Choose an unused key ID"));
    }
    verify(&s, pin, 0x81)?;
    s.send_chained(&Apdu::write(0x80, 0x74, id, 0x93, data))?;
    Ok(())
}
/// The same fixed defaults used by PicoForge's Reset HSM action.
pub fn reset_defaults() -> Result<(), PFError> {
    initialize(b"123456", b"12345678", 0)
}

pub fn initialize(pin: &[u8], so: &[u8], shares: u8) -> Result<(), PFError> {
    initialize_card(pin, so, shares, false)
}
pub fn setup(pin: &[u8], so: &[u8], shares: u8) -> Result<(), PFError> {
    initialize_card(pin, so, shares, true)
}
fn initialize_card(pin: &[u8], so: &[u8], shares: u8, first_use: bool) -> Result<(), PFError> {
    validate_pin(pin)?;
    validate_pin(so)?;
    if shares > 16 {
        return Err(error("Choose 0 to 16 DKEK shares"));
    }
    let mut data = Vec::new();
    tlv::write(&mut data, 0x80, &[0, 1]); // allow SO PIN recovery
    tlv::write(&mut data, 0x81, pin);
    tlv::write(&mut data, 0x82, so);
    tlv::write(&mut data, 0x91, &[3]);
    if shares > 0 {
        tlv::write(&mut data, 0x92, &[shares]);
    }
    let s = open()?;
    if first_use
        && (pin_status(&s, 0x81)?.1 != Some(false)
            || list_files(&s)?
                .iter()
                .any(|f| !matches!(*f, 0xC400 | 0xCC00)))
    {
        return Err(error(
            "HSM already contains user data or is initialized. Refresh its status before continuing.",
        ));
    }
    s.send_chained(&Apdu::write(0x80, 0x50, 0, 0, &data))?;
    Ok(())
}
pub fn dkek_share(pin: &[u8], share: &[u8]) -> Result<Vec<u8>, PFError> {
    if !share.is_empty() && share.len() != 32 {
        return Err(error("A DKEK share must contain exactly 32 bytes"));
    }
    let s = open()?;
    if !share.is_empty() {
        verify(&s, pin, 0x81)?;
    }
    s.transceive_full(&Apdu::read(0x80, 0x52, 0, 0, share))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pin_metadata_uses_real_limits_and_state() {
        assert_eq!(pin_metadata(&[1, 3, 3, 3]).unwrap(), "3/3 (default)");
        assert_eq!(pin_metadata(&[1, 14, 15, 3]).unwrap(), "14/15 (default)");
        assert_eq!(pin_metadata(&[1, 4, 5, 1]).unwrap(), "4/5");
        assert_eq!(pin_metadata(&[1, 0, 5, 1]).unwrap(), "0/5 (blocked)");
        assert_eq!(
            pin_metadata(&[1, 3, 3, 2]).unwrap(),
            "3/3 (not initialized)"
        );
        for bad in [
            &[1, 4, 3, 3][..],
            &[1, 0, 0, 1],
            &[2, 3, 3, 3],
            &[1, 3, 3, 7],
            &[1, 3],
        ] {
            assert!(pin_metadata(bad).is_err());
        }
    }
    #[test]
    fn list_ignores_padding_and_deduplicates() {
        assert_eq!(
            parse_files(&[0xCC, 1, 0xC4, 1, 0, 0, 0xCC, 1]).unwrap(),
            [0xC401, 0xCC01]
        );
        assert!(parse_files(&[0xCC]).is_err());
    }
    #[test]
    fn rsa_templates_use_bit_counts_and_public_exponent() {
        let t = key_template(4).unwrap();
        let p = tlv::find(&t, 0x7F49).unwrap();
        assert_eq!(tlv::find(p, 2), Some(&[0x10, 0][..]));
        assert_eq!(tlv::find(p, 0x82), Some(&[1, 0, 1][..]));
        assert!(key_template(9).is_err());
    }
    #[test]
    fn curve_templates_match_prime_lengths() {
        for (choice, len) in [(0, 32), (1, 48)] {
            let t = key_template(choice).unwrap();
            assert_eq!(
                tlv::find(tlv::find(&t, 0x7F49).unwrap(), 0x81)
                    .unwrap()
                    .len(),
                len
            );
        }
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::*;
    #[test]
    fn automatic_id_skips_identity_and_all_key_related_objects() {
        assert_eq!(next_key_id(&[0xCC00, 0xC400]).unwrap(), 1);
        assert_eq!(next_key_id(&[0xCC01, 0xC402, 0xCE03, 0xCC05]).unwrap(), 4);
        assert_eq!(next_key_id(&[0xCA01]).unwrap(), 1);
    }
    #[test]
    fn automatic_id_does_not_overwrite_a_full_card() {
        let files: Vec<_> = (1..=255u16).map(|id| 0xCC00 | id).collect();
        assert!(next_key_id(&files).is_err());
    }
}

#[cfg(test)]
mod object_editor_tests {
    use super::*;
    #[test]
    fn allocation_preserves_existing_objects_and_identity() {
        assert_eq!(
            next_object_id(&[0xCD01, 0xCD03, 0xCF02], 0xCD).unwrap(),
            0xCD02
        );
        assert_eq!(
            next_object_id(&[0xCC01, 0xC402, 0xCE03], 0xCE).unwrap(),
            0xCE04
        );
        assert!(next_object_id(&[], 0xCC).is_err());
        let full: Vec<_> = (1..=255).map(|n| 0xCD00 | n).collect();
        assert!(next_object_id(&full, 0xCD).is_err());
    }
    #[test]
    fn content_limit_counts_bytes() {
        assert!(validate_object_data(&[]).is_err());
        assert!(validate_object_data(&vec![1; MAX_OBJECT_BYTES]).is_ok());
        assert!(validate_object_data(&vec![1; MAX_OBJECT_BYTES + 1]).is_err());
        assert!(validate_object_data("中".repeat(600).as_bytes()).is_ok());
        assert!(validate_object_data("中".repeat(601).as_bytes()).is_err());
    }
}
