use super::view_model::{FIELDS, OffboardViewModel};
use crate::ui::components::{card::Card, page_view::PageView};
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
        if index != 2 && index != 6 {
            row = row.child(
                Button::new(SharedString::from(format!("browse-{index}")))
                    .outline()
                    .label(if index == 5 {
                        "Save as…"
                    } else {
                        "Browse…"
                    })
                    .disabled(self.loading)
                    .on_click(cx.listener(move |this, _, w, cx| {
                        if index == 5 {
                            this.select_output(index, w, cx);
                        } else {
                            this.select_file(index, w, cx);
                        }
                    })),
            );
        }
        if index == 4 {
            row = row.child(
                Button::new("new-key-path")
                    .outline()
                    .label("New path…")
                    .disabled(self.loading)
                    .on_click(cx.listener(|this, _, w, cx| this.select_output(4, w, cx))),
            );
        }
        v_flex()
            .gap_2()
            .child(FIELDS[index])
            .child(row)
            .into_any_element()
    }
    fn button(&self, id: &'static str, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        Button::new(id)
            .outline()
            .label(title)
            .disabled(self.loading)
            .on_click(cx.listener(move |this, _, w, cx| this.start(id, w, cx)))
            .into_any_element()
    }
    fn tool_card(&self, cx: &mut Context<Self>) -> Card {
        Card::new()
            .title("Local tools")
            .description("Python with rich, cryptography and pyscard; Raspberry Pi picotool")
            .icon(Icon::default().path("icons/settings.svg"))
            .child(self.field(0, cx))
            .child(self.field(1, cx))
    }
    fn result_card(&self, cx: &mut Context<Self>) -> Card {
        let mut result = v_flex().min_w_0().gap_2();
        if let Some(error) = &self.error {
            result = result.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        for line in self.log.lines() {
            // Keep large public-key/metadata lines within the content width.
            let chars: Vec<_> = line.chars().collect();
            for part in chars.chunks(88) {
                result = result.child(
                    div()
                        .text_sm()
                        .font_family("monospace")
                        .child(part.iter().collect::<String>()),
                );
            }
        }
        let mut card = Card::new()
            .title(if self.loading {
                "Operation in progress"
            } else {
                "Operation details"
            })
            .child(result);
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
        let mut body = v_flex().gap_6().w_full().child(self.tool_card(cx)).child(
            Card::new()
                .title("Provisioning target")
                .description("Every stage is bound to this serial and signed firmware")
                .child(self.field(2, cx))
                .child(self.field(3, cx))
                .child(self.field(6, cx)),
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
                        Button::new(id)
                            .outline()
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
            body = body.child(self.result_card(cx));
        }
        body.into_any_element()
    }
}
impl Render for OffboardViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.device.read(cx).status.clone();
        let summary = if let Some(s) = status {
            format!(
                "{} · {} · {} · Secure Boot {}",
                s.info.serial,
                s.firmware_type,
                s.info.firmware_version,
                if s.secure_boot { "enabled" } else { "disabled" }
            )
        } else {
            "No device in normal mode. Enter its serial below to manage a board in BOOTSEL mode."
                .into()
        };
        let mut body = v_flex().w_full().min_w_0().gap_6()
            .child(Card::new().title("Device firmware").description(summary).icon(Icon::default().path("icons/microchip.svg"))
                .child(self.field(2, cx))
                .child(h_flex().gap_2().flex_wrap()
                    .child(self.button("info", "Read installed image", cx))
                    .child(self.button("bootsel", "Enter update mode", cx))
                    .child(self.button("reboot", "Restart firmware", cx)))
                .child(div().text_sm().text_color(cx.theme().muted_foreground)
                    .child("Reading the installed image requests BOOTSEL and may need a button press. Restart firmware to return to normal mode.")))
            .child(self.tool_card(cx))
            .child(Card::new().title("Firmware image").description("Inspect a UF2 before signing or installing it")
                .icon(Icon::default().path("icons/file.svg")).child(self.field(3, cx))
                .child(self.button("inspect", "Inspect image", cx)))
            .child(Card::new().title("Signing").description("Create a local signing key or sign with an existing key")
                .icon(Icon::default().path("icons/key.svg"))
                .child(self.field(4, cx)).child(self.field(5, cx))
                .child(div().text_sm().text_color(cx.theme().muted_foreground)
                    .child("A locked board requires its original trusted signing key. A new key cannot replace it. Existing key and output files are never overwritten."))
                .child(h_flex().gap_2().flex_wrap()
                    .child(self.button("new-key", "Generate signing key", cx))
                    .child(self.button("sign", "Sign & verify image", cx))))
            .child(Card::new().title("Install firmware").description("Write to the selected board, verify the readback and restart")
                .icon(Icon::default().path("icons/refresh-cw.svg"))
                .child("Select the signed UF2 in Firmware image above. Keep the board connected throughout the update.")
                .child(self.button("flash", "Review firmware update…", cx)));
        if self.loading || !self.log.is_empty() || self.error.is_some() {
            body = body.child(self.result_card(cx));
        }
        PageView::build(
            "Firmware",
            "Inspect, sign and install Pico All firmware.",
            body,
            cx.theme(),
        )
    }
}
