use super::view_model::{FIELDS, OffboardViewModel};
use crate::ui::components::{button::standard, card::Card, information};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, Icon,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    select::Select,
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
                    .label(crate::i18n::tr("Browse…"))
                    .disabled(self.loading)
                    .on_click(cx.listener(move |this, _, w, cx| this.select_file(index, w, cx))),
            );
        }
        v_flex()
            .gap_2()
            .child(crate::i18n::tr(FIELDS[index]))
            .child(row)
            .into_any_element()
    }
    fn button(&self, id: &'static str, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        standard(id, cx)
            .label(crate::i18n::text(title))
            .disabled(self.loading)
            .on_click(cx.listener(move |this, _, w, cx| this.start(id, w, cx)))
            .into_any_element()
    }
    fn restart_card(&self, cx: &mut Context<Self>) -> Card {
        Card::new()
            .title(crate::i18n::tr("Restart device"))
            .icon(Icon::default().path("icons/refresh-cw.svg"))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(self.button("reboot", crate::i18n::tr("Normal mode"), cx))
                    .child(self.button("bootsel", crate::i18n::tr("Update mode"), cx)),
            )
    }
    fn result_card(&self, height: Option<Pixels>, cx: &mut Context<Self>) -> Div {
        let mut result = v_flex().w_full().min_w_0().gap_4();
        let minimum = super::console::LEVELS[self
            .log_level
            .read(cx)
            .selected_value()
            .copied()
            .unwrap_or(2) as usize];
        for entry in self.log.entries.iter().filter(|e| e.visible(minimum)) {
            let color = match entry.level.as_str() {
                "ERROR" => rgb(0xf87171),
                "WARN" => rgb(0xfbbf24),
                "INFO" => rgb(0x67e8f9),
                "DEBUG" | "TRACE" => rgb(0xa1a1aa),
                _ => rgb(0x67e8f9),
            };
            let mut record = v_flex()
                .min_w_0()
                .w_full()
                .gap_1()
                .font_family("monospace")
                .child(
                    h_flex()
                        .gap_3()
                        .text_xs()
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(entry.timestamp.clone()),
                        )
                        .child(div().text_color(color).child(entry.level.clone())),
                );
            for line in entry.message.lines() {
                record = record.child(
                    div()
                        .w_full()
                        .min_w_0()
                        .text_sm()
                        .text_color(if matches!(entry.level.as_str(), "ERROR" | "WARN") {
                            color
                        } else {
                            rgb(0xe4e4e7)
                        })
                        .child(super::console::wrap_tokens(&crate::i18n::text(line))),
                );
            }
            result = result.child(record);
        }
        if !self.log.entries.iter().any(|e| e.visible(minimum)) {
            result = result.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::i18n::tr("No messages at this log level.")),
            );
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
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(crate::i18n::tr("Console")),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_shrink_0()
                            .child(Select::new(&self.log_level).w(px(115.)))
                            .child(
                                standard("clear-console", cx)
                                    .icon(Icon::default().path("icons/trash-2.svg"))
                                    .tooltip(crate::i18n::tr("Clear"))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.log.clear();
                                        this.error = None;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .id("firmware-log-frame")
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .w_full()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(rgb(0x101012))
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .id("firmware-log")
                            .track_scroll(&self.log_scroll)
                            .size_full()
                            .overflow_y_scroll()
                            .p_3()
                            .pr_5()
                            .child(result),
                    )
                    .child(
                        gpui_component::scroll::Scrollbar::vertical(&self.log_scroll)
                            .scrollbar_show(gpui_component::scroll::ScrollbarShow::Always),
                    ),
            );
        if let Some(request) = &self.pending {
            let needs_boot = matches!(request.action.as_str(), "harden" | "enable");
            if needs_boot {
                card = card.child(h_flex().gap_3()
                    .child(Switch::new("boot-tested").checked(self.boot_tested)
                        .on_click(cx.listener(|this, checked, _, cx| { this.boot_tested = *checked; cx.notify(); })))
                    .child(crate::i18n::tr("I power-cycled and tested the signed firmware after the previous stage.")));
            }
            card = card.child(
                Button::new("apply-stage")
                    .danger()
                    .label(crate::i18n::tr("Confirm reviewed stage"))
                    .disabled(self.loading || (needs_boot && !self.boot_tested))
                    .on_click(cx.listener(|this, _, w, cx| this.confirm_pending(w, cx))),
            );
        }
        card
    }
    pub fn security_controls(&self, locked: bool, cx: &mut Context<Self>) -> AnyElement {
        let mut body = v_flex().gap_6().w_full().child(
            Card::new()
                .title(crate::i18n::tr("Provisioning target"))
                .description(crate::i18n::tr(
                    "Every stage is bound to this serial and signed firmware",
                ))
                .child(self.field(1, cx))
                .child(self.field(2, cx))
                .child(self.field(4, cx)),
        );
        let mut stages = v_flex().gap_3();
        for (id, title, description) in [
            (
                "status",
                crate::i18n::tr("Read OTP status"),
                crate::i18n::tr("Requests update mode to read the actual fuse state."),
            ),
            (
                "load-key",
                crate::i18n::tr("1 · Register signing key"),
                crate::i18n::tr(
                    "Permanently trust the public key in the signed image using the selected key slot.",
                ),
            ),
            (
                "harden",
                crate::i18n::tr("2 · Harden device"),
                crate::i18n::tr(
                    "Permanently disable debug and enable glitch detection. Power-cycle and test afterwards.",
                ),
            ),
            (
                "prepare",
                crate::i18n::tr("3 · Prepare storage"),
                crate::i18n::tr(
                    "Erase all application credentials and PINs before enabling Secure Boot. Starts from normal mode.",
                ),
            ),
            (
                "enable",
                crate::i18n::tr("4 · Enable Secure Boot"),
                crate::i18n::tr(
                    "Require signed firmware permanently. Empty storage and installed-image verification are required.",
                ),
            ),
            (
                "prove",
                crate::i18n::tr("5 · Verify protected boot"),
                crate::i18n::tr(
                    "After a power cycle, check the normal-mode OTP root and save the boot verification.",
                ),
            ),
            (
                "lock",
                crate::i18n::tr("6 · Lock boot configuration"),
                crate::i18n::tr(
                    "Permanently revoke other key slots and prevent changes to boot configuration.",
                ),
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
                                crate::i18n::tr("Read / verify")
                            } else if id == "prepare" {
                                crate::i18n::tr("Review erase…")
                            } else {
                                crate::i18n::tr("Review stage…")
                            })
                            .disabled(disabled)
                            .on_click(cx.listener(move |this, _, w, cx| this.start(id, w, cx))),
                    ),
            );
        }
        body = body.child(
            Card::new()
                .title(crate::i18n::tr("Security setup"))
                .description(crate::i18n::tr(
                    "Complete stages in order; review each change before applying",
                ))
                .child(stages),
        );
        if self.loading || !self.log.entries.is_empty() || self.error.is_some() {
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
            .child(self.button("info", crate::i18n::tr("Read device"), cx));
        let mut details = information::grid();
        if let Some(s) = status.filter(|s| s.info.serial.eq_ignore_ascii_case(selected.trim())) {
            for (label, value) in [
                (crate::i18n::tr("Serial number"), s.info.serial),
                (crate::i18n::tr("Firmware"), s.firmware_type.to_string()),
                (crate::i18n::tr("Version"), s.info.firmware_version),
                (
                    crate::i18n::tr("Secure Boot"),
                    if s.secure_boot {
                        crate::i18n::tr("Enabled")
                    } else {
                        crate::i18n::tr("Disabled")
                    }
                    .into(),
                ),
                (
                    crate::i18n::tr("Manufacturer"),
                    s.info
                        .manufacturer
                        .unwrap_or_else(|| crate::i18n::tr("Unavailable").into()),
                ),
                (crate::i18n::tr("Product"), s.config.product_name),
            ] {
                details = details.child(information::field(label, value, cx.theme()));
            }
        } else {
            details = details.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::i18n::tr("Device information unavailable")),
            );
        }
        let device = Card::new()
            .title(crate::i18n::tr("Device firmware"))
            .icon(Icon::default().path("icons/microchip.svg"))
            .header_right(
                standard("firmware-refresh", cx)
                    .icon(Icon::default().path("icons/refresh-cw.svg"))
                    .tooltip(crate::i18n::tr("Refresh device information"))
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
            .title(crate::i18n::tr("Firmware image"))
            .icon(Icon::default().path("icons/file.svg"))
            .child(self.field(2, cx))
            .child(self.field(3, cx))
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap_2()
                    .child(self.button("inspect", crate::i18n::tr("Inspect"), cx))
                    .child(
                        standard("sign", cx)
                            .label(crate::i18n::tr("Sign"))
                            .disabled(sign_disabled)
                            .on_click(cx.listener(|this, _, w, cx| this.start("sign", w, cx))),
                    )
                    .child(
                        standard("flash", cx)
                            .label(crate::i18n::tr("Flash"))
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
                    v_flex().flex_shrink_0().child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child(crate::i18n::tr("Firmware")),
                    ),
                )
                .child(body),
        )
    }
}
