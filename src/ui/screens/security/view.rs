//! Hardware security with read-only enabled switches and a staged provisioning workflow.
use super::view_model::SecurityViewModel;
use crate::ui::components::{card::Card, page_view::PageView};
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, Icon, button::Button, h_flex, switch::Switch, v_flex,
};

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
        body = body.child(Card::new().title("Permanent hardware protection")
            .icon(Icon::default().path("icons/shield-check.svg"))
            .child(div().text_color(if locked { cx.theme().muted_foreground } else { cx.theme().danger })
                .child(if locked { "Hardware protection is locked. Firmware updates require the original trusted signing key." }
                    else { "Enabling protection programs permanent fuses. Keep the signing key backed up and verify each stage after a power cycle. Losing the trusted key can prevent future updates." })));
        let mut settings = v_flex().gap_5();
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
        if let Some(root) = &self.root {
            let state = match root.state {
                0 => "Off",
                1 => "Ready",
                -1 => "Requires empty storage",
                -2 => "Read error",
                -3 => "Foreign page",
                -4 => "No free root page",
                -5 => "Corrupt",
                -6 => "Unprotected",
                _ => "Unknown",
            };
            settings = settings
                .child(div().h_px().bg(cx.theme().border))
                .child(format!(
                    "Application root: {state} · OTP page {}",
                    root.page
                ))
                .child(format!(
                    "Debug: {} · Hardware flags {:08X}",
                    if root.critical & 4 != 0 {
                        "Disabled"
                    } else {
                        "Enabled"
                    },
                    root.critical
                ));
        }
        body = body.child(Card::new().title("Lock settings").description("Current device state; use the verified setup stages below to enable protection")
            .icon(Icon::default().path("icons/lock.svg"))
            .header_right(Button::new("security-refresh").outline().label("Refresh").disabled(self.loading)
                .on_click(cx.listener(|this, _, _, cx| this.load(cx))))
            .child(settings));
        if let Some(error) = &self.error {
            body = body.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        if !locked {
            let controls = self
                .tools
                .update(cx, |tools, cx| tools.security_controls(false, cx));
            body = body.child(controls);
        } else {
            body = body.child(Card::new().title("Signed firmware updates").description(
                "Use Firmware to inspect, sign and install updates with the original trusted key.",
            ));
        }
        PageView::build(
            "Secure Boot",
            "Protect firmware startup and lock the device to its trusted signing key.",
            body,
            cx.theme(),
        )
    }
}
