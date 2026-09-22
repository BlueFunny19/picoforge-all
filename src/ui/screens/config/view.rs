use crate::ui::components::{card::Card, page_view::PageView};
use crate::ui::models::device::{
    DeviceMethod, FirmwareType, LedColor, USB_CAP_FIDO2, USB_CAP_OATH, USB_CAP_OPENPGP,
    USB_CAP_OTP, USB_CAP_PIV, USB_CAP_U2F,
};
use crate::ui::screens::config::view_model::ConfigViewModel;
use gpui::*;
use gpui_component::{button::*, input::*, select::*, switch::*, *};

/// Per-status LED brightness is a full u8 on the device (0-255, 0 = off). The
/// +/- steppers move in coarse steps (15 divides 255 evenly) so the whole range
/// stays reachable without hundreds of clicks.
const LED_BRIGHTNESS_MAX: u8 = 255;
const LED_BRIGHTNESS_STEP: u8 = 15;

impl ConfigViewModel {
    fn render_identity_card(
        &self,
        theme: &Theme,
        is_fido: bool,
        hardware_config_disabled: bool,
    ) -> impl IntoElement {
        let content = v_flex()
            .gap_4()
            .child(
                v_flex().gap_2().child("Vendor Preset").child(
                    Select::new(&self.vendor_select)
                        .bg(rgb(0x222225))
                        .w_full()
                        .disabled(hardware_config_disabled),
                ),
            )
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(
                        v_flex().gap_2().child("Vendor ID (HEX)").child(
                            Input::new(&self.vid_input)
                                .font_family("Mono")
                                .bg(rgb(0x222225))
                                .disabled(hardware_config_disabled || !self.is_custom_vendor),
                        ),
                    )
                    .child(
                        v_flex().gap_2().child("Product ID (HEX)").child(
                            Input::new(&self.pid_input)
                                .font_family("Mono")
                                .bg(rgb(0x222225))
                                .disabled(hardware_config_disabled || !self.is_custom_vendor),
                        ),
                    ),
            )
            .child(div().h_px().bg(theme.border))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(
                        v_flex().gap_2().child("Product Name").child(
                            Input::new(&self.product_name_input)
                                .bg(rgb(0x222225))
                                .disabled(is_fido),
                        ),
                    )
                    .child(
                        v_flex().gap_2().child("Manufacturer").child(
                            Input::new(&self.manufacturer_input)
                                .bg(rgb(0x222225))
                                .disabled(is_fido),
                        ),
                    ),
            );

        Card::new()
            .title("Identity")
            .description("USB Identification settings")
            .icon(Icon::default().path("icons/tag.svg"))
            .child(content)
    }

    fn render_led_card(
        &mut self,
        cx: &mut Context<Self>,
        is_fido: bool,
        is_rskey: bool,
        hardware_config_disabled: bool,
    ) -> impl IntoElement {
        // GPIO pin + driver are the LED hardware topology — always shown.
        let mut content = v_flex().gap_4().child(
            div()
                .grid()
                .grid_cols(2)
                .gap_4()
                .child(
                    v_flex().gap_2().child("LED GPIO Pin").child(
                        Input::new(&self.led_gpio_input)
                            .bg(rgb(0x222225))
                            .disabled(hardware_config_disabled),
                    ),
                )
                .child(
                    v_flex().gap_2().child("LED Driver").child(
                        Select::new(&self.led_driver_select)
                            .w_full()
                            .bg(rgb(0x222225))
                            .disabled(is_fido),
                    ),
                ),
        );

        // Colour order is an RS-Key extension (phy tag 0x0D); pico-fido ignores
        // it, so only surface it for RS-Key. Fixes red/green swap on GRB panels.
        if is_rskey
            || self
                .device
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|s| s.firmware_type == FirmwareType::PicoAll)
        {
            content = content.child(
                v_flex().gap_2().child("LED Colour Order").child(
                    Select::new(&self.led_order_select)
                        .w_full()
                        .bg(rgb(0x222225))
                        .disabled(hardware_config_disabled),
                ),
            );
        }

        Card::new()
            .title("LED Settings")
            .description("Adjust visual feedback behavior")
            .icon(Icon::default().path("icons/microchip.svg"))
            .child(content)
    }

    fn render_touch_card(&self, _theme: &Theme, is_fido: bool) -> impl IntoElement {
        let content = v_flex().gap_4().child(
            v_flex().gap_2().child("Touch Timeout (seconds)").child(
                Input::new(&self.touch_timeout_input)
                    .bg(rgb(0x222225))
                    .disabled(is_fido),
            ),
        );

        Card::new()
            .title("Touch & Timing")
            .description("Configure interaction timeouts")
            .icon(Icon::default().path("icons/settings.svg"))
            .child(content)
    }

    fn render_options_card(
        &mut self,
        cx: &mut Context<Self>,
        hardware_config_disabled: bool,
    ) -> impl IntoElement {
        let power_cycle_listener = cx.listener(|this, checked, _, cx| {
            this.power_cycle = *checked;
            cx.notify();
        });

        let theme = cx.theme();

        let content = v_flex().gap_4().child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    v_flex().gap_0p5().child("Power Cycle on Reset").child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child("Restart device on reset"),
                    ),
                )
                .child(
                    Switch::new("power-cycle")
                        .checked(self.power_cycle)
                        .disabled(hardware_config_disabled)
                        .on_click(power_cycle_listener),
                ),
        );

        Card::new()
            .title("Device Options")
            .description("Toggle advanced features")
            .icon(Icon::default().path("icons/settings.svg"))
            .child(content)
    }

    fn render_rskey_led_card(
        &mut self,
        cx: &mut Context<Self>,
        disabled: bool,
    ) -> impl IntoElement {
        let available = self.device.read(cx).led_status.is_some();
        let pico_all = self
            .device
            .read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.firmware_type == FirmwareType::PicoAll);
        let modes_available = self
            .device
            .read(cx)
            .led_status
            .as_ref()
            .is_some_and(|l| l.steady_modes.is_some());
        let mut rows = div().grid().grid_cols(2).gap_4();
        let notifications_available = self
            .device
            .read(cx)
            .led_status
            .as_ref()
            .is_some_and(|l| l.notifications.is_some());
        let states: &[&str] = if pico_all {
            &[
                "Ready",
                "Processing",
                "Button confirmation",
                "Firmware update",
                "Success",
                "Timeout",
                "Error",
            ]
        } else {
            &["Idle", "Processing", "Touch", "Boot"]
        };
        for (i, &name) in states.iter().enumerate() {
            // Timeout shares Error's configuration; keep the wire indices stable.
            if pico_all && i == 5 {
                continue;
            }
            let available = available && (i < 4 || notifications_available);
            let color = self.led_status_colors[i];
            let brightness = self.led_status_brightness[i];
            let palette = [
                0x3f3f46, 0xef4444, 0x22c55e, 0x3b82f6, 0xfacc15, 0xd946ef, 0x22d3ee, 0xffffff,
            ];
            let swatch = palette[(color as usize).min(7)];
            let color_name = if available {
                LedColor::from_u8(color)
                    .map(|c| c.label())
                    .unwrap_or("Unknown")
            } else {
                "Unavailable"
            };
            rows = rows.child(
                v_flex()
                    .min_w_0()
                    .gap_3()
                    .p_4()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .child(
                        h_flex()
                            .justify_between()
                            .gap_3()
                            .child(div().font_semibold().child(name))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if !modes_available {
                                                "Unavailable"
                                            } else if self.led_status_modes[i] {
                                                "Steady"
                                            } else {
                                                "Breathing"
                                            }),
                                    )
                                    .child(
                                        Switch::new(SharedString::from(format!("status-mode-{i}")))
                                            .checked(self.led_status_modes[i])
                                            .disabled(disabled || !available || !modes_available)
                                            .on_click(cx.listener(move |this, checked, _, cx| {
                                                this.led_status_modes[i] = *checked;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .grid()
                            .grid_cols(2)
                            .gap_3()
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Color"),
                                    )
                                    .child(
                                        Button::new(SharedString::from(format!("led-color-{i}")))
                                            .outline()
                                            .disabled(disabled || !available)
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(div().size_3().rounded_full().bg(rgb(
                                                        if available { swatch } else { 0x3f3f46 },
                                                    )))
                                                    .child(color_name),
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.led_status_colors[i] =
                                                    (this.led_status_colors[i] + 1) % 8;
                                                cx.notify();
                                            })),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Brightness"),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "led-less-{i}"
                                                )))
                                                .outline()
                                                .label("−")
                                                .disabled(disabled || !available || brightness == 0)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.led_status_brightness[i] = this
                                                        .led_status_brightness[i]
                                                        .saturating_sub(LED_BRIGHTNESS_STEP);
                                                    cx.notify();
                                                })),
                                            )
                                            .child(div().w_12().text_center().child(if available {
                                                format!(
                                                    "{}%",
                                                    (brightness as u32 * 100 + 127) / 255
                                                )
                                            } else {
                                                "—".into()
                                            }))
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "led-more-{i}"
                                                )))
                                                .outline()
                                                .label("+")
                                                .disabled(
                                                    disabled
                                                        || !available
                                                        || brightness == LED_BRIGHTNESS_MAX,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.led_status_brightness[i] = this
                                                        .led_status_brightness[i]
                                                        .saturating_add(LED_BRIGHTNESS_STEP);
                                                    cx.notify();
                                                })),
                                            ),
                                    ),
                            ),
                    ),
            );
        }
        let mut card = Card::new()
            .title("Status light")
            .icon(Icon::default().path("icons/palette.svg"))
            .description("Switch off for breathing, on for a steady light")
            .child(rows);
        if !modes_available {
            card = card.child(crate::ui::components::form::info_card(
                "Update Pico All firmware to configure breathing or steady mode for each status.",
            ));
        }
        if pico_all && available && !notifications_available {
            card = card.child(crate::ui::components::notice::warning(
                "Notification colors unavailable",
                "Update the device firmware to edit Success and Error colors.",
                false,
            ));
        }
        if !available {
            card = card.child(crate::ui::components::notice::warning(
                "Status colors unavailable",
                if pico_all {
                    "Update the device firmware to read and save status colors."
                } else {
                    "Refresh the device to retry reading status colors."
                },
                false,
            ));
        }
        card
    }

    fn render_rskey_apps_card(
        &mut self,
        cx: &mut Context<Self>,
        is_fido: bool,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let mut rows = v_flex().gap_4();

        let apps = [
            ("FIDO2", USB_CAP_FIDO2),
            ("OATH", USB_CAP_OATH),
            ("PIV", USB_CAP_PIV),
            ("OpenPGP", USB_CAP_OPENPGP),
            ("U2F", USB_CAP_U2F),
            ("OTP", USB_CAP_OTP),
        ];

        for (name, cap) in apps {
            let is_supported = (self.usb_apps_supported & cap) != 0;
            let is_enabled = (self.usb_apps_enabled & cap) != 0;

            let toggle_listener = cx.listener(move |this, checked, _, cx| {
                if *checked {
                    this.usb_apps_enabled |= cap;
                } else {
                    this.usb_apps_enabled &= !cap;
                }
                cx.notify();
            });

            rows =
                rows.child(
                    h_flex()
                        .items_center()
                        .justify_between()
                        .child(v_flex().gap_0p5().child(name).child(
                            div().text_sm().text_color(theme.muted_foreground).child(
                                if is_supported {
                                    "Supported"
                                } else {
                                    "Not Supported by Firmware"
                                },
                            ),
                        ))
                        .child(
                            Switch::new(gpui::SharedString::from(format!("app-toggle-{}", cap)))
                                .checked(is_enabled)
                                .disabled(is_fido || !is_supported)
                                .on_click(toggle_listener),
                        ),
                );
        }

        Card::new()
            .title("USB Applications")
            .description("Enable or disable specific USB features")
            .icon(Icon::default().path("icons/microchip.svg"))
            .child(rows)
    }

    fn render_rskey_usb_itf_card(
        &mut self,
        cx: &mut Context<Self>,
        is_fido: bool,
    ) -> impl IntoElement {
        let theme = cx.theme();
        // Only the interfaces the firmware actually instantiates (USB_ITF_SUPPORTED
        // = CCID | HID | KB). WCID (WebUSB) and LWIP are pico-fido concepts RS-Key
        // never builds, so toggling them would be a no-op — don't offer them.
        let mut rows = v_flex()
            .gap_4()
            .child(crate::ui::components::notice::warning(
                "USB interfaces",
                "Turning off HID disables passkeys. CCID stays on for device management.",
                false,
            ));

        let interfaces = [
            (
                "CCID (Smart Card)",
                0x01u8,
                "Required for the rescue applet and all smart-card apps",
            ),
            (
                "HID (FIDO)",
                0x04u8,
                "FIDO/CTAP transport — off disables all FIDO2 and U2F",
            ),
            (
                "KB (Keyboard)",
                0x08u8,
                "OTP keyboard — Yubico OTP and static-password typing",
            ),
        ];

        let current_mask = self.enabled_usb_itf.unwrap_or(0x1F);

        for (name, bit, desc) in interfaces {
            let is_enabled = (current_mask & bit) != 0;
            let is_ccid = bit == 0x01;

            let toggle_listener = cx.listener(move |this, checked, _, cx| {
                let mut mask = this.enabled_usb_itf.unwrap_or(0x1F);
                if *checked {
                    mask |= bit;
                } else {
                    mask &= !bit;
                }

                if bit == 0x01 {
                    mask |= 0x01;
                }

                this.enabled_usb_itf = Some(mask);
                cx.notify();
            });

            rows = rows.child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex().gap_0p5().child(name).child(
                            div()
                                .text_sm()
                                .text_color(theme.muted_foreground)
                                .w_full()
                                .max_w(px(600.0))
                                .child(desc),
                        ),
                    )
                    .child(
                        Switch::new(gpui::SharedString::from(format!("usb-itf-toggle-{}", bit)))
                            .checked(is_enabled || is_ccid)
                            .disabled(is_fido || is_ccid)
                            .on_click(toggle_listener),
                    ),
            );
        }

        Card::new()
            .title("Hardware Endpoints")
            .description("Toggle low-level USB interfaces")
            .icon(Icon::default().path("icons/cpu.svg"))
            .child(rows)
    }
}

impl Render for ConfigViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_device = self.device.read(cx).status.is_some();

        if !has_device {
            let theme = cx.theme();
            return PageView::build(
                "Configuration",
                "Customize device settings and behavior.",
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .h_64()
                    .border_1()
                    .border_color(theme.border)
                    .rounded_xl()
                    .child(
                        div()
                            .text_color(theme.muted_foreground)
                            .child("No Device Connected"),
                    ),
                theme,
            );
        }

        let device = self.device.read(cx);
        let status = device.status.clone();
        let is_fido = status.as_ref().map(|s| s.method.clone()) == Some(DeviceMethod::Fido);
        let is_rskey = status.as_ref().map(|s| &s.firmware_type) == Some(&FirmwareType::RSKey);

        let supports_legacy_fido_config = status
            .as_ref()
            .map(ConfigViewModel::status_supports_legacy_fido_config)
            .unwrap_or(false);

        let hardware_config_disabled = is_fido && !supports_legacy_fido_config && !is_rskey;

        // RS-Key supports full config read/write over FIDO via CONFIG_READ/CONFIG_WRITE.
        // Other firmwares (pico-fido) don't: product name, LED driver, curves, etc.
        let is_fido_no_rskey = is_fido && !is_rskey;

        let led_card = self
            .render_led_card(cx, is_fido_no_rskey, is_rskey, hardware_config_disabled)
            .into_any_element();
        let options_card = self
            .render_options_card(cx, hardware_config_disabled)
            .into_any_element();

        let identity_card = self
            .render_identity_card(cx.theme(), is_fido_no_rskey, hardware_config_disabled)
            .into_any_element();
        let touch_card = self
            .render_touch_card(cx.theme(), is_fido_no_rskey)
            .into_any_element();

        let mut inner = v_flex().gap_6().w_full().child(identity_card);

        // RS-Key: put the functional config (which apps + transports are on)
        // right after Identity, before appearance/misc, so the panel reads
        // top-down by importance rather than burying it under the LED cards.
        // No curves card: the firmware ignores the phy ENABLED_CURVES tag
        // (curve support is compile-time), so exposing it would only mislead.
        if is_rskey || status.as_ref().map(|s| &s.firmware_type) == Some(&FirmwareType::PicoAll) {
            inner = inner
                .child(self.render_rskey_apps_card(cx, false))
                .child(self.render_rskey_usb_itf_card(cx, false));
        }

        inner = inner.child(led_card);

        if is_rskey
            || status
                .as_ref()
                .is_some_and(|s| s.firmware_type == FirmwareType::PicoAll)
        {
            inner = inner.child(self.render_rskey_led_card(cx, self.loading));
        }
        inner = inner.child(touch_card).child(options_card);

        inner = inner.child(
            h_flex().justify_end().pt_4().child(
                Button::new("apply-changes")
                    .icon(Icon::default().path("icons/save.svg"))
                    .child("Apply Changes")
                    .disabled(self.loading || hardware_config_disabled)
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(rgb(0xe3e3e6).into())
                            .hover(rgb(0xcfcfd1).into())
                            .active(rgb(0xe3e3e6).into())
                            .foreground(rgb(0x4b4b4e).into()),
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_changes(window, cx);
                    })),
            ),
        );

        let theme = cx.theme();
        PageView::build(
            "Configuration",
            "Customize device settings and behavior.",
            inner,
            theme,
        )
    }
}
