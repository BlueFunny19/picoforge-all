//! ISO 7816-4 APDU encoding and status-word decoding.
//!
//! Firmware-agnostic: every CCID applet (OATH, PIV, OpenPGP, OTP) speaks the
//! same command/response APDU framing over PC/SC. This is the shared base the
//! `hal::applets::*` modules build on, alongside the BER-TLV codec in [`tlv`].

// Staged applet foundation: some helpers (chaining class, retry decoding) are
// consumed by later applet stages rather than the OATH screen alone.
#![allow(dead_code)]

use crate::error::PFError;

pub mod tlv;

/// GET RESPONSE — pulls the next chunk after a `61xx` status (`00 C0 00 00 Le`).
pub const INS_GET_RESPONSE: u8 = 0xC0;
/// SELECT by DF name (`00 A4 04 00`).
pub const INS_SELECT: u8 = 0xA4;
/// YKOATH SEND REMAINING (`00 A5 00 00 Le`) — continues a `61xx` page. OATH
/// paginates LIST / CALCULATE ALL with this, not ISO GET RESPONSE (`0xC0`),
/// which it rejects with `6D00` (dropping the pending page).
pub const INS_SEND_REMAINING: u8 = 0xA5;
/// ISO class byte for the applets we speak (all use `0x00`).
pub const CLA_ISO: u8 = 0x00;
/// Command-chaining class bit — set on every fragment but the last.
pub const CLA_CHAIN: u8 = 0x10;

/// A command APDU. `le` requests a response length (`Some(0)` = the ISO short
/// form "send up to 256 bytes"); `None` is a pure write with no `Le`.
#[derive(Debug, Clone)]
pub struct Apdu {
    pub cla: u8,
    pub ins: u8,
    pub p1: u8,
    pub p2: u8,
    pub data: Vec<u8>,
    pub le: Option<u16>,
}

impl Apdu {
    /// A command that expects a response (`Le = 0` short form).
    pub fn read(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Self {
        Self {
            cla,
            ins,
            p1,
            p2,
            data: data.to_vec(),
            le: Some(0),
        }
    }

    /// A pure write (no `Le`), for commands that answer with only a status word.
    pub fn write(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Self {
        Self {
            cla,
            ins,
            p1,
            p2,
            data: data.to_vec(),
            le: None,
        }
    }

    /// Encode to the wire. Short-form Lc/Le when the body is ≤255 bytes,
    /// extended-form (3-byte Lc, 2-byte Le) when longer.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.cla, self.ins, self.p1, self.p2];
        let n = self.data.len();
        if n == 0 {
            if let Some(le) = self.le {
                out.push((le & 0xFF) as u8); // 0 → 256 (ISO short Le)
            }
        } else if n <= 255 && self.le.map(|le| le <= 256).unwrap_or(true) {
            out.push(n as u8);
            out.extend_from_slice(&self.data);
            if let Some(le) = self.le {
                out.push((le & 0xFF) as u8);
            }
        } else {
            // Extended: 3-byte Lc, and a 2-byte Le if requested.
            out.push(0);
            out.extend_from_slice(&(n as u16).to_be_bytes());
            out.extend_from_slice(&self.data);
            if let Some(le) = self.le {
                out.extend_from_slice(&le.to_be_bytes());
            }
        }
        out
    }
}

/// A two-byte ISO status word (SW1 SW2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusWord(pub u16);

impl StatusWord {
    pub const OK: u16 = 0x9000;

    pub fn is_ok(self) -> bool {
        self.0 == Self::OK
    }

    /// `61xx` → `xx` more bytes available via GET RESPONSE (`0` means 256).
    pub fn more_data(self) -> Option<u8> {
        ((self.0 & 0xFF00) == 0x6100).then_some((self.0 & 0xFF) as u8)
    }

    /// `6Cxx` → resend the command with `Le = xx`.
    pub fn wrong_le(self) -> Option<u8> {
        ((self.0 & 0xFF00) == 0x6C00).then_some((self.0 & 0xFF) as u8)
    }

    /// `63Cx` → `x` verification attempts remaining (wrong PIN/password).
    pub fn retries_left(self) -> Option<u8> {
        ((self.0 & 0xFFF0) == 0x63C0).then_some((self.0 & 0x0F) as u8)
    }

    /// Map a non-`9000` status to a typed, user-facing error.
    pub fn to_error(self) -> PFError {
        let msg = match self.0 {
            0x6581 => "Storage failure — not enough memory on the device".to_string(),
            0x6982 => "Security status not satisfied — unlock or verify the PIN first".to_string(),
            0x6983 => "Authentication method blocked".to_string(),
            0x6A80 => "Incorrect parameters in the command data".to_string(),
            0x6A81 => "Function not supported by this firmware".to_string(),
            0x6A82 => "Requested object not found".to_string(),
            0x6A83 => "Record not found".to_string(),
            0x6A84 => "Not enough memory on the device".to_string(),
            0x6A88 => "Reference data not found — no reset code set, or unknown object".to_string(),
            0x6985 => "Conditions of use not satisfied".to_string(),
            0x6700 => "Wrong length".to_string(),
            0x6B00 => "Wrong parameters P1-P2".to_string(),
            0x6D00 => "Instruction not supported".to_string(),
            0x6E00 => "Applet class not supported".to_string(),
            sw if (sw & 0xFFF0) == 0x63C0 => {
                format!("Verification failed — {} attempt(s) left", sw & 0x0F)
            }
            other => format!("Card returned status 0x{other:04X}"),
        };
        PFError::Device(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_case2_no_data_with_le() {
        // LIST: header + single Le byte (0 = 256).
        assert_eq!(
            Apdu::read(0x00, 0xA1, 0, 0, &[]).encode(),
            vec![0x00, 0xA1, 0, 0, 0x00]
        );
    }

    #[test]
    fn encodes_case3_write_no_le() {
        assert_eq!(
            Apdu::write(0x00, 0x02, 0, 0, &[0x71, 0x01, 0xAA]).encode(),
            vec![0x00, 0x02, 0, 0, 0x03, 0x71, 0x01, 0xAA]
        );
    }

    #[test]
    fn encodes_case4_data_and_le() {
        assert_eq!(
            Apdu::read(0x00, 0xA2, 0, 1, &[0x71, 0x01, 0xAA]).encode(),
            vec![0x00, 0xA2, 0, 1, 0x03, 0x71, 0x01, 0xAA, 0x00]
        );
    }

    #[test]
    fn encodes_extended_lc_when_body_over_255() {
        let body = vec![0x5A; 300];
        let enc = Apdu::write(0x00, 0xDB, 0x3F, 0xFF, &body).encode();
        assert_eq!(&enc[..7], &[0x00, 0xDB, 0x3F, 0xFF, 0x00, 0x01, 0x2C]); // Lc = 0x012C = 300
        assert_eq!(enc.len(), 4 + 3 + 300);
    }

    #[test]
    fn status_word_classification() {
        assert!(StatusWord(0x9000).is_ok());
        assert_eq!(StatusWord(0x6110).more_data(), Some(0x10));
        assert_eq!(StatusWord(0x6C1D).wrong_le(), Some(0x1D));
        assert_eq!(StatusWord(0x63C2).retries_left(), Some(2));
        assert_eq!(StatusWord(0x9000).more_data(), None);
    }

    #[test]
    fn oath_continuation_ins_is_send_remaining() {
        // OATH pages with SEND REMAINING (0xA5), not GET RESPONSE (0xC0).
        assert_eq!(INS_SEND_REMAINING, 0xA5);
        assert_ne!(INS_SEND_REMAINING, INS_GET_RESPONSE);
    }
}
