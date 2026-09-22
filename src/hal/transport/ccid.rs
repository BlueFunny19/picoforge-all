//! Persistent CCID (PC/SC) session for the smart-card applets.
//!
//! Unlike [`super::pcsc::PcscTransport`] (one fresh connection per rescue op),
//! a `CcidSession` **holds the card open** across calls. That is mandatory for
//! the applets: `SELECT` resets the card's security status, so a VERIFY and the
//! operation it authorises must run on the same open card. It also grows the rx
//! buffer past 256 bytes and implements `61xx`/`6Cxx` response assembly and
//! `CLA|0x10` command-chaining, none of which the rescue transport has.

// `send_chained` is consumed by the PIV/OpenPGP import stages, not by OATH.
#![allow(dead_code)]

use crate::error::PFError;
use crate::hal::apdu::{
    Apdu, CLA_CHAIN, CLA_ISO, INS_GET_RESPONSE, INS_SELECT, INS_SEND_REMAINING, StatusWord,
};
use std::sync::MutexGuard;

/// Largest single PC/SC response chunk we read before assembling via GET RESPONSE.
const RX_BUF: usize = 4096;
/// Largest command-data fragment in a chained write (ISO short Lc max).
const CHAIN_CHUNK: usize = 255;

/// An open PC/SC card bound to one applet (selected by AID).
pub struct CcidSession {
    card: pcsc::Card,
    _session: MutexGuard<'static, ()>,
    /// The applet's `SELECT` response (FCI / version block), parsed per applet.
    pub select_resp: Vec<u8>,
}

impl CcidSession {
    /// Connect to the first reader and `SELECT` the given applet AID.
    ///
    /// The card is kept open for the session's lifetime so a subsequent VERIFY
    /// stays in effect for the following operation.
    pub fn open(aid: &[u8]) -> Result<Self, PFError> {
        let (card, guard) = super::pcsc::connect_selected()?;
        let mut session = Self {
            card,
            _session: guard,
            select_resp: Vec::new(),
        };
        let select = Apdu::read(CLA_ISO, INS_SELECT, 0x04, 0x00, aid);
        session.select_resp = session.transceive_full(&select).map_err(|e| {
            PFError::Device(format!(
                "Applet not available — enable it and the CCID interface in Configuration ({e})"
            ))
        })?;
        Ok(session)
    }

    /// Send one APDU, returning `(response_data, status_word)`.
    pub fn transceive(&self, apdu: &Apdu) -> Result<(Vec<u8>, StatusWord), PFError> {
        let tx = apdu.encode();
        let mut rx = [0u8; RX_BUF];
        let started = std::time::Instant::now();
        log::debug!(
            "Smart-card command INS={:02X} P1={:02X} P2={:02X}, {} bytes",
            apdu.ins,
            apdu.p1,
            apdu.p2,
            tx.len()
        );
        let resp =
            super::pcsc::driver_call(|| self.card.transmit(&tx, &mut rx)).map_err(|error| {
                log::error!(
                    "Smart-card INS={:02X} failed after {:.1}s: {error}",
                    apdu.ins,
                    started.elapsed().as_secs_f32()
                );
                error
            })?;
        log::debug!(
            "Smart-card INS={:02X} completed after {:.1}s ({} bytes)",
            apdu.ins,
            started.elapsed().as_secs_f32(),
            resp.len()
        );
        if resp.len() < 2 {
            return Err(PFError::Device("Truncated APDU response".into()));
        }
        let (data, sw) = resp.split_at(resp.len() - 2);
        Ok((
            data.to_vec(),
            StatusWord(u16::from_be_bytes([sw[0], sw[1]])),
        ))
    }

    /// Send an APDU and assemble the full response across `61xx` continuations,
    /// retrying once on `6Cxx`. Errors on any non-`9000` final SW. Continues a
    /// `61xx` page with ISO GET RESPONSE (`0xC0`) — the right form for every
    /// applet except OATH (see [`transceive_oath`](Self::transceive_oath)).
    pub fn transceive_full(&self, apdu: &Apdu) -> Result<Vec<u8>, PFError> {
        self.transceive_paged(apdu, INS_GET_RESPONSE)
    }

    /// Like [`transceive_full`](Self::transceive_full) but continues a `61xx`
    /// page with YKOATH SEND REMAINING (`0xA5`) instead of GET RESPONSE. The
    /// OATH applet paginates LIST / CALCULATE ALL this way and rejects `0xC0`
    /// with `6D00`, discarding the pending page — so its two paged reads must
    /// use this. (Matches a real YubiKey, which pages OATH identically.)
    pub fn transceive_oath(&self, apdu: &Apdu) -> Result<Vec<u8>, PFError> {
        self.transceive_paged(apdu, INS_SEND_REMAINING)
    }

    fn transceive_paged(&self, apdu: &Apdu, continue_ins: u8) -> Result<Vec<u8>, PFError> {
        assemble_response(apdu, continue_ins, |cmd| self.transceive(cmd))
    }

    /// Send a command whose data exceeds 255 bytes via ISO command-chaining
    /// (`CLA|0x10` on every fragment but the last). Used for PIV/OpenPGP import.
    pub fn send_chained(&self, apdu: &Apdu) -> Result<Vec<u8>, PFError> {
        if apdu.data.len() <= CHAIN_CHUNK {
            return self.transceive_full(apdu);
        }
        let data = apdu.data.clone();
        let mut i = 0;
        while data.len() - i > CHAIN_CHUNK {
            let frag = Apdu::write(
                apdu.cla | CLA_CHAIN,
                apdu.ins,
                apdu.p1,
                apdu.p2,
                &data[i..i + CHAIN_CHUNK],
            );
            let (_, sw) = self.transceive(&frag)?;
            if !sw.is_ok() {
                return Err(sw.to_error());
            }
            i += CHAIN_CHUNK;
        }
        let last = Apdu {
            cla: apdu.cla,
            ins: apdu.ins,
            p1: apdu.p1,
            p2: apdu.p2,
            data: data[i..].to_vec(),
            le: apdu.le,
        };
        self.transceive_full(&last)
    }
}

fn assemble_response(
    apdu: &Apdu,
    continue_ins: u8,
    mut transmit: impl FnMut(&Apdu) -> Result<(Vec<u8>, StatusWord), PFError>,
) -> Result<Vec<u8>, PFError> {
    let mut cmd = apdu.clone();
    let mut out = Vec::new();
    let mut corrected = false;
    for _ in 0..512 {
        let (data, sw) = transmit(&cmd)?;
        if let Some(n) = sw.wrong_le() {
            // 6Cxx: resend the same command with the corrected Le, no data yet.
            if corrected {
                return Err(PFError::Device(
                    "Card repeatedly rejected the response length.".into(),
                ));
            }
            corrected = true;
            cmd.le = Some(if n == 0 { 256 } else { n as u16 });
            continue;
        }
        if out.len() + data.len() > 65536 {
            return Err(PFError::Device("Card response exceeds 64 KiB.".into()));
        }
        out.extend_from_slice(&data);
        if let Some(n) = sw.more_data() {
            corrected = false;
            cmd = Apdu::read(CLA_ISO, continue_ins, 0, 0, &[]);
            cmd.le = Some(if n == 0 { 256 } else { n as u16 });
            continue;
        }
        if sw.is_ok() {
            return Ok(out);
        }
        return Err(sw.to_error());
    }
    Err(PFError::Device(
        "Card response did not finish after 512 pages.".into(),
    ))
}
#[cfg(test)]
mod response_tests {
    use super::*;
    #[test]
    fn length_retry_and_response_pages_use_real_assembler() {
        let request = Apdu::read(0, 0x47, 0x81, 0, &[0xb6, 0]);
        let mut count = 0;
        let result = assemble_response(&request, INS_GET_RESPONSE, |cmd| {
            count += 1;
            Ok(match count {
                1 => (vec![], StatusWord(0x6c02)),
                2 => {
                    assert_eq!(cmd.le, Some(2));
                    (vec![1, 2], StatusWord(0x6101))
                }
                3 => {
                    assert_eq!(cmd.ins, INS_GET_RESPONSE);
                    (vec![3], StatusWord(0x9000))
                }
                _ => panic!("unexpected extra command"),
            })
        })
        .unwrap();
        assert_eq!(result, [1, 2, 3]);
    }
    #[test]
    fn nonterminating_firmware_responses_are_bounded() {
        let command = Apdu::read(0, 0x47, 0x80, 0, &[]);
        let mut calls = 0;
        assert!(
            assemble_response(&command, INS_GET_RESPONSE, |_| {
                calls += 1;
                Ok((vec![], StatusWord(0x6c00)))
            })
            .is_err()
        );
        assert_eq!(calls, 2);
        calls = 0;
        assert!(
            assemble_response(&command, INS_GET_RESPONSE, |_| {
                calls += 1;
                Ok((vec![], StatusWord(0x6100)))
            })
            .is_err()
        );
        assert_eq!(calls, 512);
    }
}
