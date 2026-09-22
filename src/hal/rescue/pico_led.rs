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
pub fn block(raw: &[u8], wanted: u8) -> Option<&[u8]> {
    let mut pos = 0;
    while pos + 2 <= raw.len() {
        let tag = raw[pos];
        let len = raw[pos + 1] as usize;
        pos += 2;
        let v = raw.get(pos..pos + len)?;
        if tag == wanted {
            return Some(v);
        }
        pos += len;
    }
    None
}
pub fn parse(raw: &[u8]) -> Option<LedStatusConfig> {
    let b = block(raw, 0x10)?;
    if b.len() != 10 || b[0] != 1 || b[1] > 1 || (0..4).any(|i| b[2 + i * 2] > 7) {
        return None;
    }
    let notifications = match block(raw, 0x11) {
        Some(n) if n.len() == 7 && n[0] == 1 && (0..3).all(|i| n[1 + i * 2] <= 7) => {
            Some(std::array::from_fn(|i| (n[1 + i * 2], n[2 + i * 2])))
        }
        Some(_) => return None,
        None => None,
    };
    let steady_modes = match block(raw, 0x12) {
        Some([1, flags]) if flags & 0x80 == 0 => {
            Some(std::array::from_fn(|i| flags & (1 << i) != 0))
        }
        Some(_) => return None,
        None => None,
    };
    Some(LedStatusConfig {
        steady_modes,
        steady: b[1] != 0,
        notifications,
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
    update(&mut raw, &config)?;
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
/// Update only advertised LED TLVs; preserve all unrelated PHY bytes.
fn update(raw: &mut [u8], config: &LedStatusConfig) -> Result<(), PFError> {
    let current = parse(raw).ok_or_else(|| {
        PFError::Device("This firmware does not support status-light editing.".into())
    })?;
    if current.steady_modes.is_some() != config.steady_modes.is_some()
        || current.notifications.is_some() != config.notifications.is_some()
        || config.statuses.iter().any(|&(color, _)| color > 7)
        || config
            .notifications
            .as_ref()
            .is_some_and(|n| n.iter().any(|&(color, _)| color > 7))
    {
        return Err(PFError::Device(
            "Unsupported status-light configuration.".into(),
        ));
    }
    let mut pos = 0;
    while pos + 2 <= raw.len() {
        let len = raw[pos + 1] as usize;
        if pos + 2 + len > raw.len() {
            return Err(PFError::Device("Truncated PHY configuration.".into()));
        }
        match raw[pos] {
            0x10 => {
                raw[pos + 3] = u8::from(config.steady);
                for (i, &(color, brightness)) in config.statuses.iter().enumerate() {
                    raw[pos + 4 + i * 2] = color;
                    raw[pos + 5 + i * 2] = brightness;
                }
            }
            0x12 => {
                if let Some(modes) = config.steady_modes {
                    raw[pos + 3] = modes
                        .iter()
                        .enumerate()
                        .fold(0, |flags, (i, steady)| flags | (u8::from(*steady) << i));
                }
            }
            0x11 => {
                if let Some(notifications) = config.notifications {
                    for (i, (color, brightness)) in notifications.into_iter().enumerate() {
                        raw[pos + 3 + i * 2] = color;
                        raw[pos + 4 + i * 2] = brightness;
                    }
                }
            }
            _ => {}
        }
        pos += 2 + len;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn per_status_modes_roundtrip_and_compatibility() {
        let mut raw = vec![
            0x10, 10, 1, 0, 6, 255, 6, 255, 4, 255, 3, 255, 0x12, 2, 1, 0, 0x72, 1, 42,
        ];
        let mut config = parse(&raw).unwrap();
        let modes = [true, false, true, false, true, false, true];
        config.steady_modes = Some(modes);
        update(&mut raw, &config).unwrap();
        assert_eq!(parse(&raw).unwrap().steady_modes, Some(modes));
        assert_eq!(raw[15], 0x55);
        assert_eq!(&raw[16..], &[0x72, 1, 42]);
        assert!(update(&mut raw[..12], &config).is_err());
        raw[15] = 0x80;
        assert!(parse(&raw).is_none());
        raw[15] = 0;
        raw[14] = 2;
        assert!(parse(&raw).is_none());
    }
    #[test]
    fn versioned_values_and_old_firmware() {
        let b = [0x10, 10, 1, 0, 2, 255, 2, 255, 4, 255, 3, 255];
        assert_eq!(parse(&b).unwrap().statuses[2], (4, 255));
        assert!(parse(&b).unwrap().notifications.is_none());
        assert!(parse(&[5, 1, 1]).is_none());
        let mut malformed = b;
        malformed[4] = 8;
        assert!(parse(&malformed).is_none());
        assert!(parse(&b[..8]).is_none());
    }
    #[test]
    fn notification_roundtrip_preserves_base_and_unknown_fields() {
        let mut raw = vec![
            0x10, 10, 1, 0, 6, 255, 6, 255, 4, 255, 3, 255, 0x11, 7, 1, 2, 255, 1, 255, 1, 255,
            0x72, 2, 42, 43,
        ];
        let mut config = parse(&raw).unwrap();
        config.notifications.as_mut().unwrap()[1] = (5, 128);
        update(&mut raw, &config).unwrap();
        assert_eq!(parse(&raw), Some(config.clone()));
        assert_eq!(&raw[21..], &[0x72, 2, 42, 43]);
        let mut old = raw[..12].to_vec();
        assert!(update(&mut old, &config).is_err());
        config.notifications = None;
        update(&mut old, &config).unwrap();
        assert_eq!(parse(&old), Some(config));
        raw[15] = 8;
        assert!(parse(&raw).is_none());
    }
}
