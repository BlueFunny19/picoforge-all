//! Hardware security with read-only enabled switches and a staged provisioning workflow.
use super::view_model::SecurityViewModel;
use crate::ui::components::{
    button::standard, card::Card, information, notice, page_view::PageView,
};
use gpui::*;
use gpui_component::{ActiveTheme, Disableable, Icon, h_flex, switch::Switch, v_flex};

impl Render for SecurityViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.device.read(cx).status.clone();
        let locked = status.as_ref().is_some_and(|s| s.secure_lock);
        let enabled = self
            .root
            .as_ref()
            .map(|r| r.critical & 1 != 0)
            .or_else(|| status.as_ref().map(|s| s.secure_boot));
        let mut body = v_flex().gap_6().w_full();
        let mut details = information::grid();
        if let Some(status) = &status {
            details = details
                .child(information::field(
                    crate::i18n::tr("Serial number"),
                    status.info.serial.clone(),
                    cx.theme(),
                ))
                .child(information::field(
                    crate::i18n::tr("Secure Boot"),
                    if enabled == Some(true) {
                        crate::i18n::tr("Enabled")
                    } else if enabled == Some(false) {
                        crate::i18n::tr("Disabled")
                    } else {
                        crate::i18n::tr("Unavailable")
                    },
                    cx.theme(),
                ))
                .child(information::field(
                    crate::i18n::tr("Boot configuration"),
                    if locked {
                        crate::i18n::tr("Permanently locked")
                    } else {
                        crate::i18n::tr("Unlocked")
                    },
                    cx.theme(),
                ));
        } else {
            details = details.child(div().text_color(cx.theme().muted_foreground).child(
                if self.loading {
                    crate::i18n::tr("Reading device…")
                } else {
                    crate::i18n::tr("Device information unavailable")
                },
            ));
        }
        if let Some(root) = &self.root {
            let state = match root.state {
                0 => crate::i18n::tr("Off"),
                1 => crate::i18n::tr("Ready"),
                -1 => crate::i18n::tr("Requires empty storage"),
                -2 => crate::i18n::tr("Unavailable"),
                -3 => crate::i18n::tr("Foreign page"),
                -4 => crate::i18n::tr("No free page"),
                -5 => crate::i18n::tr("Corrupt"),
                -6 => crate::i18n::tr("Unprotected"),
                _ => crate::i18n::tr("Unknown"),
            };
            details = details
                .child(information::field(
                    crate::i18n::tr("Application root"),
                    state,
                    cx.theme(),
                ))
                .child(information::field(
                    crate::i18n::tr("OTP page"),
                    root.page.to_string(),
                    cx.theme(),
                ))
                .child(information::field(
                    crate::i18n::tr("Debug interface"),
                    if root.critical & 4 != 0 {
                        crate::i18n::tr("Disabled")
                    } else {
                        crate::i18n::tr("Enabled")
                    },
                    cx.theme(),
                ))
                .child(information::field(
                    crate::i18n::tr("Hardware flags"),
                    format!("{:08X}", root.critical),
                    cx.theme(),
                ));
        }
        body = body.child(
            Card::new()
                .title(crate::i18n::tr("Hardware security"))
                .icon(Icon::default().path("icons/shield-check.svg"))
                .header_right(
                    standard("security-refresh", cx)
                        .icon(Icon::default().path("icons/refresh-cw.svg"))
                        .tooltip(crate::i18n::tr("Refresh hardware security"))
                        .disabled(self.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                )
                .child(details),
        );
        let mut settings = v_flex().gap_5().child(notice::warning(
            crate::i18n::tr("Permanent hardware changes"),
            crate::i18n::tr("Secure Boot and Secure Lock cannot be undone. Losing the trusted signing key can prevent future firmware updates."),
            true));
        for (id, name, description, checked) in [
            (
                "secure-boot",
                crate::i18n::tr("Enable Secure Boot"),
                crate::i18n::tr(
                    "Verify firmware signatures at startup. Once enabled, this cannot be disabled.",
                ),
                enabled.unwrap_or(false),
            ),
            (
                "secure-lock",
                crate::i18n::tr("Secure Lock"),
                crate::i18n::tr(
                    "Make boot configuration read-only and prevent signing-key rotation.",
                ),
                locked,
            ),
        ] {
            settings = settings.child(
                h_flex()
                    .justify_between()
                    .gap_4()
                    .child(
                        v_flex().gap_1().child(name).child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(description),
                        ),
                    )
                    // Provisioning needs multiple verified stages; the status switch never burns a fuse.
                    .child(
                        Switch::new(id)
                            .checked(checked)
                            .disabled(checked || self.loading || enabled.is_none())
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.tools.update(cx, |tools, cx| {
                                    tools.start(
                                        if id == "secure-boot" {
                                            "enable"
                                        } else {
                                            "lock"
                                        },
                                        w,
                                        cx,
                                    )
                                });
                            })),
                    ),
            );
        }
        if enabled.is_none() {
            settings = settings.child(crate::i18n::tr(
                "Connect the device in normal mode to read its current protection state.",
            ));
        }
        body = body.child(
            Card::new()
                .title(crate::i18n::tr("Lock settings"))
                .icon(Icon::default().path("icons/lock.svg"))
                .child(settings),
        );
        if let Some(error) = &self.error {
            body = body.child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(error.clone()),
            );
        }
        if !locked {
            let controls = self
                .tools
                .update(cx, |tools, cx| tools.security_controls(false, cx));
            body = body.child(controls);
        }
        PageView::build(
            crate::i18n::tr("Security"),
            crate::i18n::tr(
                "Protect firmware startup and lock the device to its trusted signing key.",
            ),
            body,
            cx.theme(),
        )
    }
}
