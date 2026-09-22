//! Plain-language labels for the device's compact audit records.
use crate::ui::models::device::audit::AuditEntry;

pub(super) fn event_title(entry: &AuditEntry) -> String {
    match entry.event {
        0x01 => crate::i18n::tr("Device started"),
        0x02 => crate::i18n::tr("Passkey created"),
        0x03 => crate::i18n::tr("Passkey used"),
        0x04 => crate::i18n::tr("FIDO reset"),
        0x05 => crate::i18n::tr("FIDO PIN set"),
        0x06 => crate::i18n::tr("FIDO PIN changed"),
        0x07 if entry.aux == 1 => crate::i18n::tr("PIN temporarily blocked"),
        0x07 => crate::i18n::tr("PIN blocked"),
        0x08 => crate::i18n::tr("Minimum PIN length changed"),
        0x09 => crate::i18n::tr("Enterprise attestation changed"),
        0x0A => crate::i18n::tr("Device locked"),
        0x0B => crate::i18n::tr("Device unlocked"),
        0x0C => crate::i18n::tr("Backup exported"),
        0x0D => crate::i18n::tr("Backup loaded"),
        0x0E => crate::i18n::tr("Recovery exports disabled"),
        0x0F => crate::i18n::tr("Security key registered"),
        0x10 => crate::i18n::tr("Security key used"),
        0x11 => crate::i18n::tr("Log verification requested"),
        0x12 => crate::i18n::tr("Organization certificate imported"),
        0x13 => crate::i18n::tr("Organization certificate removed"),
        0x14 => crate::i18n::tr("User verification setting changed"),
        0x15 => crate::i18n::tr("Device settings saved"),
        0x16 if entry.aux == 1 => crate::i18n::tr("Event recording enabled"),
        0x16 if entry.aux == 0 => crate::i18n::tr("Event recording disabled"),
        0x16 => crate::i18n::tr("Event recording changed"),
        0x17 => crate::i18n::tr("Enterprise site list changed"),
        _ => return crate::i18n::format("Unknown event ({0})", &[format!("{:02X}", entry.event)]),
    }
    .into()
}

pub(super) fn event_detail(entry: &AuditEntry) -> String {
    let mut detail = entry
        .timestamp
        .and_then(crate::preferences::timestamp)
        .unwrap_or_else(|| crate::i18n::tr("Time unknown").into());
    if entry.event == 0x08 {
        detail.push_str(&crate::i18n::format(
            " · Minimum {0} characters",
            &[format!("{}", entry.aux)],
        ));
    }
    if matches!(entry.event, 0x02 | 0x03 | 0x0F | 0x10) && entry.detail != [0; 8] {
        detail.push_str(&crate::i18n::format(
            " · Site fingerprint {0}",
            &[format!("{}", entry.detail_hex())],
        ));
    }
    detail
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explains_event_values_without_inventing_dates_or_site_names() {
        let mut entry = AuditEntry {
            seq: 7,
            uptime_ms: 314500,
            timestamp: None,
            event: 0x16,
            aux: 1,
            detail: [0; 8],
        };
        assert_eq!(event_title(&entry), "Event recording enabled");
        assert_eq!(event_detail(&entry), "Time unknown");
        entry.aux = 0;
        assert_eq!(event_title(&entry), "Event recording disabled");
        entry.event = 0xff;
        assert_eq!(event_title(&entry), "Unknown event (FF)");
    }
}
