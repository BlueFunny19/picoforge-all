//! Per-firmware applet feature profiles.
//!
//! Extends the `firmwares/` capability seam to the CCID applets. A firmware
//! answers "does my applet support feature X?" through [`AppletProfile`], and
//! the applet screens read the answer to show or disable controls. Adding a new
//! firmware means one `impl AppletProfile` here — the shared wire ops in
//! [`crate::hal::applets`], the transport, and the UI stay untouched.

use crate::hal::applets::{OathFeatures, OpenPgpFeatures, OtpFeatures, PivFeatures};
use crate::hal::firmwares::{AnyFirmware, PicoFidoFirmware, RSKeyFirmware};

/// Feature answers per applet. `None` means the firmware exposes no such applet
/// (or it is not yet characterised) → the screen renders an honest empty state
/// rather than offering controls the wire can't back.
pub trait AppletProfile {
    fn oath(&self) -> Option<OathFeatures> {
        None
    }
    fn otp(&self) -> Option<OtpFeatures> {
        None
    }
    fn piv(&self) -> Option<PivFeatures> {
        None
    }
    fn openpgp(&self) -> Option<OpenPgpFeatures> {
        None
    }
}

impl AppletProfile for RSKeyFirmware {
    fn oath(&self) -> Option<OathFeatures> {
        // From the RS-Key rsk-oath wire audit: full Yubico-Authenticator parity.
        Some(OathFeatures {
            rename: true,
            touch: true,
            sha512: true,
            password: true,
        })
    }

    fn otp(&self) -> Option<OtpFeatures> {
        // rsk-otp implements all four slot types over four slots (3/4 = extension).
        Some(OtpFeatures {
            slots: 4,
            chalresp: true,
            hotp: true,
            static_pw: true,
            yubiotp: true,
            swap: true,
        })
    }

    fn piv(&self) -> Option<PivFeatures> {
        Some(PivFeatures {
            generate: true,
            import_cert: true,
            attestation: true,
            retired_slots: true,
        })
    }

    fn openpgp(&self) -> Option<OpenPgpFeatures> {
        // rsk-openpgp implements OpenPGP Card 3.4 with RSA + the full EC set
        // (Weierstrass / Ed25519 / X25519), per-key touch, and reset code.
        Some(OpenPgpFeatures {
            generate: true,
            touch: true,
            reset_code: true,
            ecc: true,
        })
    }
}

// No applet data for pico-fido / other firmwares yet — a maintainer supplies it
// when the wire facts are known. Until then screens render "not characterised".
impl AppletProfile for PicoFidoFirmware {}

impl AnyFirmware {
    /// OATH feature profile for the inner firmware.
    pub fn oath_features(&self) -> Option<OathFeatures> {
        match self {
            Self::PicoFido(fw) => fw.oath(),
            Self::RSKey(fw) => fw.oath(),
        }
    }

    /// OTP feature profile for the inner firmware.
    pub fn otp_features(&self) -> Option<OtpFeatures> {
        match self {
            Self::PicoFido(fw) => fw.otp(),
            Self::RSKey(fw) => fw.otp(),
        }
    }

    /// PIV feature profile for the inner firmware.
    pub fn piv_features(&self) -> Option<PivFeatures> {
        match self {
            Self::PicoFido(fw) => fw.piv(),
            Self::RSKey(fw) => fw.piv(),
        }
    }

    /// OpenPGP feature profile for the inner firmware.
    pub fn openpgp_features(&self) -> Option<OpenPgpFeatures> {
        match self {
            Self::PicoFido(fw) => fw.openpgp(),
            Self::RSKey(fw) => fw.openpgp(),
        }
    }
}
