//! PC/SC management transport and binding to the selected security key.
use crate::error::PFError;
use crate::hal::{rescue::constants::*, types::FirmwareType};
use pcsc::{Context, Protocols, Scope, ShareMode};
use std::ffi::CString;
use std::sync::{Mutex, MutexGuard};

/// pcsc 2.9 panics on unmapped Windows driver status values (for example
/// ERROR_GEN_FAILURE during removal). Convert that library boundary to an
/// operation error so the view model can release its busy state.
pub(crate) fn driver_call<T>(call: impl FnOnce() -> Result<T, pcsc::Error>) -> Result<T, PFError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
        Ok(result) => result.map_err(PFError::Pcsc),
        Err(_) => Err(PFError::Device("The smart-card driver interrupted the operation. Reconnect the device, then refresh its status.".into())),
    }
}
static CARD_SESSION: Mutex<()> = Mutex::new(());

/// Serialize HID and smart-card operations without blocking the UI thread.
pub(crate) fn lock_device() -> Result<MutexGuard<'static, ()>, PFError> {
    match CARD_SESSION.try_lock() {
        Ok(guard) => Ok(guard),
        // The mutex guards channel ownership, not mutable data. A panicking
        // caller drops its card/session; the next operation may safely proceed.
        Err(std::sync::TryLockError::Poisoned(error)) => Ok(error.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => Err(PFError::Device(
            "Device busy; wait for the current operation to finish".into(),
        )),
    }
}
static SELECTED: Mutex<Option<Target>> = Mutex::new(None);

#[derive(Clone)]
struct Target {
    reader: CString,
    identity: Vec<u8>,
    firmware: FirmwareType,
}

/// Chip serial of the selected Pico All, used to bind HID to the same board.
pub fn selected_pico_all_serial() -> Option<String> {
    SELECTED
        .lock()
        .ok()?
        .as_ref()
        .filter(|t| t.firmware == FirmwareType::PicoAll)
        .map(|t| hex::encode_upper(&t.identity[4..12]))
}

/// SELECT identity, independent of a configurable USB product name.
pub(crate) fn identify(reader: &str, data: &[u8]) -> FirmwareType {
    if data.len() >= 12 && data[1] == 0 && data[2] >= 8 {
        FirmwareType::PicoAll
    } else if reader.contains("RS-Key")
        || reader.contains("RSK")
        || (data.len() >= 4 && data[2] >= 8)
    {
        FirmwareType::RSKey
    } else if data.len() >= 4 {
        FirmwareType::PicoFido
    } else {
        FirmwareType::Unknown
    }
}

fn select(card: &pcsc::Card, aid: &[u8]) -> Result<Vec<u8>, PFError> {
    let mut apdu = vec![0, 0xA4, 4, 4, aid.len() as u8];
    apdu.extend_from_slice(aid);
    let mut buf = [0; 4096];
    let rx = driver_call(|| card.transmit(&apdu, &mut buf))?;
    if !rx.ends_with(&SW_SUCCESS) {
        return Err(PFError::Device(
            "Application not available on the selected device".into(),
        ));
    }
    Ok(rx.to_vec())
}

/// An authenticated applet session must keep this guard until its operation ends.
pub(crate) fn connect_selected() -> Result<(pcsc::Card, MutexGuard<'static, ()>), PFError> {
    let guard = lock_device()?;
    let target = SELECTED.lock().unwrap().clone().ok_or_else(|| {
        PFError::Device("Refresh the device before opening an application".into())
    })?;
    let ctx = driver_call(|| Context::establish(Scope::User))?;
    let card = driver_call(|| ctx.connect(&target.reader, ShareMode::Shared, Protocols::ANY))?;
    let response = select(&card, RESCUE_AID)?;
    if response[..response.len() - 2] != target.identity {
        return Err(PFError::Device(
            "The connected device changed; refresh before continuing".into(),
        ));
    }
    Ok((card, guard))
}

/// PC/SC transport wrapping the selected card.
pub struct PcscTransport {
    pub card: pcsc::Card,
    pub firmware_type: FirmwareType,
    pub select_resp: Vec<u8>,
    _session: MutexGuard<'static, ()>,
}

impl PcscTransport {
    /// Discover a single supported card; never silently choose between boards.
    pub fn discover() -> Result<Self, PFError> {
        let guard = lock_device()?;
        *SELECTED.lock().unwrap() = None;
        let ctx = driver_call(|| Context::establish(Scope::User))?;
        let mut buf = [0; 4096];
        let mut candidates = Vec::new();
        for reader in ctx.list_readers(&mut buf)? {
            let Ok(card) = ctx.connect(reader, ShareMode::Shared, Protocols::ANY) else {
                continue;
            };
            let Ok(data) = select(&card, RESCUE_AID) else {
                continue;
            };
            let firmware = identify(&reader.to_string_lossy(), &data[..data.len() - 2]);
            if firmware != FirmwareType::Unknown {
                candidates.push((reader.to_owned(), card, data, firmware));
            }
        }
        if candidates.len() > 1 {
            return Err(PFError::Device(
                "More than one supported key is connected; leave only the key you want to manage"
                    .into(),
            ));
        }
        let (reader, card, data, firmware) = candidates.pop().ok_or(PFError::NoDevice)?;
        *SELECTED.lock().unwrap() = Some(Target {
            reader,
            identity: data[..data.len() - 2].to_vec(),
            firmware: firmware.clone(),
        });
        Ok(Self {
            card,
            firmware_type: firmware,
            select_resp: data,
            _session: guard,
        })
    }

    pub fn open() -> Result<Self, PFError> {
        Self::open_with_aid(RESCUE_AID)
    }

    pub fn open_with_aid(aid: &[u8]) -> Result<Self, PFError> {
        if SELECTED.lock().unwrap().is_none() {
            drop(Self::discover()?);
        }
        let (card, guard) = connect_selected()?;
        let firmware = SELECTED.lock().unwrap().as_ref().unwrap().firmware.clone();
        let data = select(&card, aid)?;
        Ok(Self {
            card,
            firmware_type: firmware,
            select_resp: data,
            _session: guard,
        })
    }

    pub fn transmit<'a>(&self, apdu: &[u8], rx_buf: &'a mut [u8]) -> Result<&'a [u8], PFError> {
        driver_call(|| self.card.transmit(apdu, rx_buf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_identity_is_not_rskey_or_usb_name_dependent() {
        let mut data = [0u8; 12];
        data[2] = 8;
        data[3] = 1;
        assert_eq!(identify("FIDO", &data), FirmwareType::PicoAll);
        data[1] = 1;
        assert_eq!(identify("RS-Key", &data), FirmwareType::RSKey);
        data[2] = 7;
        assert_eq!(identify("FIDO", &data), FirmwareType::PicoFido);
        assert_eq!(identify("FIDO", &[]), FirmwareType::Unknown);
    }
}

#[cfg(test)]
mod driver_tests {
    use super::*;
    #[test]
    fn unexpected_driver_code_is_an_operation_error() {
        let result: Result<(), PFError> = driver_call(|| panic!("unmapped PC/SC error"));
        assert!(result.unwrap_err().to_string().contains("Reconnect"));
        assert_eq!(driver_call(|| Ok(42)).unwrap(), 42);
    }
}
