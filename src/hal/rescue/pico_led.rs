//! Pico All PHY 0x10: versioned status-light configuration.
use crate::{
    error::PFError,
    hal::{transport::pcsc::PcscTransport, types::LedStatusConfig},
};
pub fn data(transport: &PcscTransport) -> Result<Vec<u8>, PFError> {
    let mut rx = [0; 256];
    let r = transport.transmit(&[0x80, 0x1e, 1, 0, 0], &mut rx)?;
    if !r.ends_with(&[0x90, 0]) {
        return Err(PFError::Device("LED configuration unavailable".into()));
    }
    Ok(r[..r.len() - 2].to_vec())
}
pub fn block(raw: &[u8]) -> Option<&[u8]> {
    let mut pos = 0;
    while pos + 2 <= raw.len() {
        let tag = raw[pos];
        let len = raw[pos + 1] as usize;
        pos += 2;
        let v = raw.get(pos..pos + len)?;
        if tag == 0x10 {
            return Some(v);
        }
        pos += len;
    }
    None
}
pub fn parse(raw: &[u8]) -> Option<LedStatusConfig> {
    let b = block(raw)?;
    if b.len() != 10 || b[0] != 1 || b[1] > 1 || (0..4).any(|i| b[2 + i * 2] > 7) {
        return None;
    }
    Some(LedStatusConfig {
        steady: b[1] != 0,
        statuses: std::array::from_fn(|i| (b[2 + i * 2], b[3 + i * 2])),
    })
}
pub fn read() -> Result<LedStatusConfig, PFError> {
    let transport = PcscTransport::open()?;
    parse(&data(&transport)?).ok_or_else(|| {
        PFError::Device("Update Pico All firmware to edit status-light colours.".into())
    })
}
pub fn write(config: LedStatusConfig) -> Result<String, PFError> {
    let transport = PcscTransport::open()?;
    let mut raw = data(&transport)?;
    if parse(&raw).is_none() {
        return Err(PFError::Device(
            "This firmware does not support status-light editing.".into(),
        ));
    }
    let mut pos = 0;
    while pos + 2 <= raw.len() {
        let len = raw[pos + 1] as usize;
        if raw[pos] == 0x10 {
            raw[pos + 3] = u8::from(config.steady);
            for (i, (color, brightness)) in config.statuses.into_iter().enumerate() {
                if color > 7 {
                    return Err(PFError::Device("Invalid LED colour.".into()));
                }
                raw[pos + 4 + i * 2] = color;
                raw[pos + 5 + i * 2] = brightness;
            }
            break;
        }
        pos += 2 + len;
    }
    let mut apdu = vec![0x80, 0x1c, 1, 0, raw.len() as u8];
    apdu.extend(&raw);
    let mut rx = [0; 256];
    let response = transport.transmit(&apdu, &mut rx)?;
    if !response.ends_with(&[0x90, 0]) {
        return Err(PFError::Device(
            "LED changes were not confirmed. Retry and press the board button.".into(),
        ));
    }
    if parse(&data(&transport)?) != Some(config) {
        return Err(PFError::Device(
            "LED configuration readback differs.".into(),
        ));
    }
    Ok("Status-light colours saved and read back.".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versioned_values_and_old_firmware() {
        let b = [0x10, 10, 1, 0, 2, 255, 2, 255, 4, 255, 3, 255];
        assert_eq!(parse(&b).unwrap().statuses[2], (4, 255));
        assert!(parse(&[5, 1, 1]).is_none());
        let mut malformed = b;
        malformed[4] = 8;
        assert!(parse(&malformed).is_none());
        assert!(parse(&b[..8]).is_none());
    }
}
