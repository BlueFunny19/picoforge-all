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
                    "Serial number",
                    status.info.serial.clone(),
                    cx.theme(),
                ))
                .child(information::field(
                    "Secure Boot",
                    if enabled == Some(true) {
                        "Enabled"
                    } else if enabled == Some(false) {
                        "Disabled"
                    } else {
                        "Unavailable"
                    },
                    cx.theme(),
                ))
                .child(information::field(
                    "Boot configuration",
                    if locked {
                        "Permanently locked"
                    } else {
                        "Unlocked"
                    },
                    cx.theme(),
                ));
        } else {
            details = details.child(div().text_color(cx.theme().muted_foreground).child(
                if self.loading {
                    "Reading device…"
                } else {
                    "Device information unavailable"
                },
            ));
        }
        if let Some(root) = &self.root {
            let state = match root.state {
                0 => "Off",
                1 => "Ready",
                -1 => "Requires empty storage",
                -2 => "Unavailable",
                -3 => "Foreign page",
                -4 => "No free page",
                -5 => "Corrupt",
                -6 => "Unprotected",
                _ => "Unknown",
            };
            details = details
                .child(information::field("Application root", state, cx.theme()))
                .child(information::field(
                    "OTP page",
                    root.page.to_string(),
                    cx.theme(),
                ))
                .child(information::field(
                    "Debug interface",
                    if root.critical & 4 != 0 {
                        "Disabled"
                    } else {
                        "Enabled"
                    },
                    cx.theme(),
                ))
                .child(information::field(
                    "Hardware flags",
                    format!("{:08X}", root.critical),
                    cx.theme(),
                ));
        }
        body = body.child(
            Card::new()
                .title("Hardware security")
                .icon(Icon::default().path("icons/shield-check.svg"))
                .header_right(
                    standard("security-refresh", cx)
                        .icon(Icon::default().path("icons/refresh-cw.svg"))
                        .tooltip("Refresh hardware security")
                        .disabled(self.loading)
                        .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                )
                .child(details),
        );
        let mut settings = v_flex().gap_5().child(notice::warning(
            "Permanent hardware changes",
            "Secure Boot and Secure Lock cannot be undone. Losing the trusted signing key can prevent future firmware updates.",
            true));
        for (id, name, description, checked) in [
            (
                "secure-boot",
                "Enable Secure Boot",
                "Verify firmware signatures at startup. Once enabled, this cannot be disabled.",
                enabled.unwrap_or(false),
            ),
            (
                "secure-lock",
                "Secure Lock",
                "Make boot configuration read-only and prevent signing-key rotation.",
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
            settings = settings
                .child("Connect the device in normal mode to read its current protection state.");
        }
        body = body.child(
            Card::new()
                .title("Lock settings")
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
            "Security",
            "Protect firmware startup and lock the device to its trusted signing key.",
            body,
            cx.theme(),
        )
    }
}
