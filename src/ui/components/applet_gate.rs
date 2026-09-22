//! Shared empty-state gating for the CCID applet screens (Accounts, Slots,
//! PIV, OpenPGP).
//!
//! Three orthogonal questions decide whether a screen can show its content, in
//! priority order: is the CCID interface on, is the applet enabled on the
//! device, does this firmware expose it. Each screen computes an [`AppletGate`]
//! and, unless [`AppletGate::Ready`], renders the message below.

/// Why an applet screen cannot show its content — or `Ready` to proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppletGate {
    /// The applet is reachable; render the real UI.
    Ready,
    /// The CCID / smart-card USB interface is turned off.
    CcidOff,
    /// The applet is disabled in USB Applications (carries its display name).
    Disabled(&'static str),
    /// This firmware does not expose the applet.
    Unsupported,
    /// Firmware has this feature, but this client does not implement it yet.
    ClientUnsupported(&'static str),
}

pub fn empty_state(heading: &str, body: String, theme: &gpui_component::Theme) -> gpui::AnyElement {
    use gpui::*;
    use gpui_component::{StyledExt, v_flex};
    v_flex()
        .items_center()
        .justify_center()
        .h_64()
        .gap_2()
        .border_1()
        .border_color(theme.border)
        .rounded_xl()
        .child(div().font_semibold().child(heading.to_string()))
        .child(
            div()
                .text_sm()
                .max_w(px(380.))
                .text_color(theme.muted_foreground)
                .child(body),
        )
        .into_any_element()
}

impl AppletGate {
    /// Heading + body copy for the empty state, or `None` when [`Self::Ready`].
    ///
    /// Copy is firmware-neutral by design — it never names "pico-fido".
    pub fn message(&self) -> Option<(&'static str, String)> {
        match self {
            Self::Ready => None,
            Self::CcidOff => Some((
                crate::i18n::tr("Smart-card interface off"),
                crate::i18n::tr("Enable the CCID interface in Compose → Hardware Endpoints, then reconnect the device.")
                    .into(),
            )),
            Self::Disabled(name) => Some((
                crate::i18n::tr("Applet disabled"),
                crate::i18n::format("{0} is turned off. Enable it in Compose → USB Applications.", &[format!("{}", name)]),
            )),
            Self::ClientUnsupported(name) => Some((
                crate::i18n::tr("Not yet supported"),
                crate::i18n::format("{0} is not yet supported for Pico All in this build.", &[format!("{}", name)]),
            )),
            Self::Unsupported => Some((
                crate::i18n::tr("Not available"),
                crate::i18n::tr("This firmware does not expose this applet.").into(),
            )),
        }
    }
}
