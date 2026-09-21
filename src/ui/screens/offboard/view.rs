use super::view_model::{FIELDS, OffboardViewModel};
use crate::ui::components::{button::standard, card::Card, information};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, Icon,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    switch::Switch,
    v_flex,
};

impl OffboardViewModel {
    fn field(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let mut row = h_flex().w_full().min_w_0().gap_2().child(
            Input::new(&self.inputs[index])
                .disabled(self.loading)
                .flex_1(),
        );
        if index != 1 && index != 4 {
            row = row.child(
                standard(SharedString::from(format!("browse-{index}")), cx)
                    .label("Browse…")
                    .disabled(self.loading)
                    .on_click(cx.listener(move |this, _, w, cx| this.select_file(index, w, cx))),
            );
        }
        v_flex()
            .gap_2()
            .child(FIELDS[index])
            .child(row)
            .into_any_element()
    }
    fn button(&self, id: &'static str, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        standard(id, cx)
            .label(title)
            .disabled(self.loading)
            .on_click(cx.listener(move |this, _, w, cx| this.start(id, w, cx)))
            .into_any_element()
    }
    fn restart_card(&self, cx: &mut Context<Self>) -> Card {
        Card::new()
            .title("Restart device")
            .icon(Icon::default().path("icons/refresh-cw.svg"))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(self.button("reboot", "Normal mode", cx))
                    .child(self.button("bootsel", "Update mode", cx)),
            )
    }
    fn result_card(&self, height: Option<Pixels>, cx: &mut Context<Self>) -> Div {
        let mut result = v_flex().min_w_0().gap_0p5();
        for line in self.log.lines() {
            // Keep large public-key/metadata lines within the content width.
            let chars: Vec<_> = line.chars().collect();
            for part in chars.chunks(58) {
                result = result.child(
                    div()
                        .text_sm()
                        .font_family("monospace")
                        .child(part.iter().collect::<String>()),
                );
            }
        }
        let mut card = v_flex()
            .w_full()
            .min_w_0()
            .min_h_0()
            .gap_6()
            .bg(rgb(0x18181b))
            .border_1()
            .border_color(cx.theme().border)
            .rounded_xl()
            .p_6()
            .when_some(height, |this, height| this.h(height))
            .when(height.is_none(), |this| this.h_full())
            .child(
                h_flex()
                    .justify_between()
                    .flex_shrink_0()
                    .child(div().font_weight(FontWeight::BOLD).child("Console"))
                    .child(
                        Button::new("clear-console")
                            .ghost()
                            .label("Clear")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.log.clear();
                                this.error = None;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .id("firmware-log")
                    .track_scroll(&self.log_scroll)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .min_w_0()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(0x101012))
                    .child(result),
            );
        if let Some(request) = &self.pending {
            let needs_boot = matches!(request.action.as_str(), "harden" | "enable");
            if needs_boot {
                card = card.child(h_flex().gap_3()
                    .child(Switch::new("boot-tested").checked(self.boot_tested)
                        .on_click(cx.listener(|this, checked, _, cx| { this.boot_tested = *checked; cx.notify(); })))
                    .child("I power-cycled and tested the signed firmware after the previous stage."));
            }
            card = card.child(
                Button::new("apply-stage")
                    .danger()
                    .label("Confirm reviewed stage")
                    .disabled(self.loading || (needs_boot && !self.boot_tested))
                    .on_click(cx.listener(|this, _, w, cx| this.confirm_pending(w, cx))),
            );
        }
        card
    }
    pub fn security_controls(&self, locked: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex().gap_6().w_full().child(
            Card::new()
                .title("Provisioning target")
                .description("Every stage is bound to this serial and signed firmware")
                .child(self.field(1, cx))
                .child(self.field(2, cx))
                .child(self.field(4, cx)),
        );
        let mut stages = v_flex().gap_3();
        for (id, title, description) in [
            (
                "status",
                "Read OTP status",
                "Requests update mode to read the actual fuse state.",
            ),
            (
                "load-key",
                "1 · Register signing key",
                "Permanently trust the public key in the signed image using the selected key slot.",
            ),
            (
                "harden",
                "2 · Harden device",
                "Permanently disable debug and enable glitch detection. Power-cycle and test afterwards.",
            ),
            (
                "prepare",
                "3 · Prepare storage",
                "Erase all application credentials and PINs before enabling Secure Boot. Starts from normal mode.",
            ),
            (
                "enable",
                "4 · Enable Secure Boot",
                "Require signed firmware permanently. Empty storage and installed-image verification are required.",
            ),
            (
                "prove",
                "5 · Verify protected boot",
                "After a power cycle, check the normal-mode OTP root and save the boot verification.",
            ),
            (
                "lock",
                "6 · Lock boot configuration",
                "Permanently revoke other key slots and prevent changes to boot configuration.",
            ),
        ] {
            let disabled = self.loading || (locked && !matches!(id, "status" | "prove"));
            stages = stages.child(
                v_flex()
                    .gap_2()
                    .p_4()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .child(title)
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    )
                    .child(
                        standard(id, cx)
                            .label(if matches!(id, "status" | "prove") {
                                "Read / verify"
                            } else if id == "prepare" {
                                "Review erase…"
                            } else {
                                "Review stage…"
                            })
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, w, cx| this.start(id, w, cx))),
                    ),
            );
        }
        body = body.child(
            Card::new()
                .title("Security setup")
                .description("Complete stages in order; review each change before applying")
                .child(stages),
        );
        if self.loading || !self.log.is_empty() || self.error.is_some() {
            body = body.child(self.result_card(Some(px(520.)), cx));
        }
        body.into_any_element()
    }
}
impl Render for OffboardViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = if self.read_attempted {
            self.read_status.clone()
        } else {
            self.device.read(cx).status.clone()
        };
        let selected = self.inputs[1].read(cx).text().to_string();
        let target = h_flex()
            .gap_2()
            .child(Input::new(&self.inputs[1]).flex_1().disabled(self.loading))
            .child(self.button("info", "Read device", cx));
        let mut details = information::grid();
        if let Some(s) = status.filter(|s| s.info.serial.eq_ignore_ascii_case(selected.trim())) {
            for (label, value) in [
                ("Serial number", s.info.serial),
                ("Firmware", s.firmware_type.to_string()),
                ("Version", s.info.firmware_version),
                (
                    "Secure Boot",
                    if s.secure_boot { "Enabled" } else { "Disabled" }.into(),
                ),
                (
                    "Manufacturer",
                    s.info.manufacturer.unwrap_or_else(|| "Unavailable".into()),
                ),
                ("Product", s.config.product_name),
            ] {
                details = details.child(information::field(label, value, cx.theme()));
            }
        } else {
            details = details.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Device information unavailable"),
            );
        }
        let device = Card::new()
            .title("Device firmware")
            .icon(Icon::default().path("icons/microchip.svg"))
            .header_right(
                standard("firmware-refresh", cx)
                    .icon(Icon::default().path("icons/refresh-cw.svg"))
                    .tooltip("Refresh device information")
                    .disabled(self.loading)
                    .on_click(cx.listener(|this, _, w, cx| this.start("info", w, cx))),
            )
            .child(details)
            .child(target);
        let sign_disabled = self.loading || !self.image.as_ref().is_some_and(|i| !i.signed);
        let flash_disabled = self.image.is_none()
            || self.loading
            || self.assessment.as_ref().is_some_and(|a| !a.allowed);
        let files = Card::new()
            .title("Firmware image")
            .icon(Icon::default().path("icons/file.svg"))
            .child(self.field(2, cx))
            .child(self.field(3, cx))
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap_2()
                    .child(self.button("inspect", "Inspect", cx))
                    .child(
                        standard("sign", cx)
                            .label("Sign")
                            .disabled(sign_disabled)
                            .on_click(cx.listener(|this, _, w, cx| this.start("sign", w, cx))),
                    )
                    .child(
                        standard("flash", cx)
                            .label("FLASH")
                            .disabled(flash_disabled)
                            .on_click(cx.listener(|this, _, w, cx| this.start("flash", w, cx))),
                    ),
            );
        let left = v_flex()
            .id("firmware-controls")
            .h_full()
            .min_h_0()
            .flex_1()
            .overflow_y_scroll()
            .w_full()
            .min_w_0()
            .gap_6()
            .child(device)
            .child(files)
            .child(self.restart_card(cx));
        let body = h_flex()
            .w_full()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .gap_6()
            .child(left)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.result_card(None, cx)),
            );
        v_flex().size_full().min_h_0().items_center().child(
            v_flex()
                .size_full()
                .min_h_0()
                .max_w(px(1200.))
                .px_10()
                .py_5()
                .gap_8()
                .child(
                    v_flex()
                        .flex_shrink_0()
                        .child(
                            div()
                                .text_3xl()
                                .font_weight(FontWeight::EXTRA_BOLD)
                                .child("Firmware"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("Manage device firmware."),
                        ),
                )
                .child(body),
        )
    }
}
