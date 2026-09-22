//! PIV screen rendering.

use crate::ui::components::button::PFButton;
use crate::ui::components::card::Card;
use crate::ui::components::page_view::PageView;
use crate::ui::models::device::piv;
use crate::ui::screens::piv::view_model::PivViewModel;
use gpui::*;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_component::{ActiveTheme, Disableable, Icon, StyledExt, Theme, h_flex, v_flex};

fn empty_state(heading: &str, body: String, theme: &Theme) -> AnyElement {
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

fn kv(label: &str, value: String, theme: &Theme) -> impl IntoElement {
    v_flex()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::text(label)),
        )
        .child(div().text_sm().font_medium().child(value))
}

fn origin_label(o: u8) -> &'static str {
    match o {
        piv::ORIGIN_GENERATED => crate::i18n::tr("generated"),
        piv::ORIGIN_IMPORTED => crate::i18n::tr("imported"),
        _ => "?",
    }
}

impl PivViewModel {
    fn render_slot_row(&self, s: piv::SlotStatus, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let slot = s.slot;
        let has_key = s.meta.is_some();
        let is_generated = s
            .meta
            .map(|m| m.origin == piv::ORIGIN_GENERATED)
            .unwrap_or(false);
        let d = self.loading;

        macro_rules! btn {
            ($id:expr, $label:expr, $method:ident) => {
                PFButton::new($label)
                    .id(format!("{}-{slot:02x}", $id))
                    .disabled(d)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.$method(slot, window, cx)),
                    )
                    .into_any_element()
            };
        }

        let mut btns: Vec<AnyElement> = vec![
            PFButton::new(if has_key {
                crate::i18n::tr("Regenerate")
            } else {
                crate::i18n::tr("Generate")
            })
            .id(format!("gen-{slot:02x}"))
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .disabled(d)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_generate_dialog(slot, window, cx);
            }))
            .into_any_element(),
            btn!("impk", "Import key", open_import_key),
            btn!("impc", "Import cert", open_import_cert),
        ];
        if s.has_cert {
            btns.push(btn!("exp", "Export cert", open_export_cert));
        }
        if is_generated {
            btns.push(btn!("att", "Attest", open_attest));
        }
        if has_key {
            btns.push(btn!("mv", "Move", open_move_key));
        }
        if s.has_cert {
            btns.push(
                Button::new(SharedString::from(format!("delc-{slot:02x}")))
                    .label(crate::i18n::tr("Delete cert"))
                    .danger()
                    .disabled(d)
                    .on_click(cx.listener(move |this, _, w, cx| this.open_delete_cert(slot, w, cx)))
                    .into_any_element(),
            );
        }
        if has_key {
            btns.push(
                Button::new(SharedString::from(format!("delk-{slot:02x}")))
                    .icon(Icon::default().path("icons/trash-2.svg"))
                    .label(crate::i18n::tr("Delete key"))
                    .danger()
                    .disabled(d)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_delete_key(slot, window, cx);
                    }))
                    .into_any_element(),
            );
        }

        let row = v_flex()
            .w_full()
            .h_full()
            .overflow_hidden()
            .gap_3()
            .p_4()
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .font_medium()
                            .child(crate::i18n::text(piv::slot_label(slot))),
                    )
                    .child(
                        div()
                            .w_full()
                            .grid()
                            .grid_cols(3)
                            .gap_3()
                            .child(kv(
                                crate::i18n::tr("Algorithm"),
                                s.meta
                                    .map(|m| piv::algo_label(m.algo).to_string())
                                    .unwrap_or_else(|| crate::i18n::tr("Empty").into()),
                                theme,
                            ))
                            .child(kv(
                                crate::i18n::tr("Origin"),
                                s.meta
                                    .map(|m| origin_label(m.origin).to_string())
                                    .unwrap_or_else(|| "—".into()),
                                theme,
                            ))
                            .child(kv(
                                crate::i18n::tr("Certificate"),
                                if s.has_cert {
                                    crate::i18n::tr("Installed")
                                } else {
                                    crate::i18n::tr("Empty")
                                }
                                .into(),
                                theme,
                            )),
                    ),
            )
            .child(
                h_flex()
                    .id(SharedString::from(format!("piv-actions-{slot}")))
                    .gap_2()
                    .overflow_x_scroll()
                    .children(btns),
            );
        // The virtual list measures the root: spacing must be inside its height.
        div()
            .w_full()
            .h(px(172.))
            .pb_2()
            .child(row)
            .into_any_element()
    }

    fn action_row(
        &self,
        title: &'static str,
        subtitle: &'static str,
        btn: impl IntoElement,
        theme: &Theme,
    ) -> impl IntoElement {
        h_flex()
            .items_center()
            .justify_between()
            .p_4()
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .child(
                v_flex()
                    .gap_0p5()
                    .child(div().font_medium().child(title))
                    .child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(subtitle),
                    ),
            )
            .child(btn)
    }
}

impl Render for PivViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const TITLE: &str = "PIV";
        const SUBTITLE: &str = "Smart-card certificates and keys (PIV).";

        if let Some((heading, body)) = self.gate(cx).message() {
            let theme = cx.theme();
            return PageView::build(TITLE, SUBTITLE, empty_state(heading, body, theme), theme)
                .into_any_element();
        }

        let info = self.info.clone();
        let slots = info.as_ref().map(|i| i.slots.clone()).unwrap_or_default();

        let query = self.slot_search.read(cx).text().to_string().to_lowercase();
        let selected = crate::ui::components::form::selected_key(
            &self.slot_filter,
            super::view_model::SLOT_FILTERS,
            cx,
        );
        let slots: Vec<_> = slots
            .into_iter()
            .filter(|s| {
                let selected_match = selected == 0
                    || (selected == 1 && s.has_cert)
                    || (selected == 2 && s.meta.is_some())
                    || (selected == 3 && s.meta.is_none() && !s.has_cert);
                selected_match
                    && crate::ui::components::collection::matches(
                        &query,
                        &format!(
                            "{} {} {}",
                            crate::i18n::text(piv::slot_label(s.slot)),
                            s.meta.map(|m| piv::algo_label(m.algo)).unwrap_or("Empty"),
                            if s.has_cert {
                                crate::i18n::tr("certificate installed")
                            } else {
                                crate::i18n::tr("no certificate")
                            }
                        ),
                    )
            })
            .collect();
        let list_height = crate::preferences::list_height(slots.len(), 172.);
        let weak = cx.entity().downgrade();
        let slot_rows = if slots.is_empty() {
            div()
                .p_4()
                .child(crate::i18n::tr("No matching slots"))
                .into_any_element()
        } else {
            uniform_list("piv-slot-list", slots.len(), move |range, _, cx| {
                weak.update(cx, |this, cx| {
                    range
                        .map(|i| this.render_slot_row(slots[i].clone(), cx))
                        .collect()
                })
                .unwrap_or_default()
            })
            .track_scroll(self.slot_scroll.clone())
            .h(px(list_height))
            .w_full()
            .into_any_element()
        };

        // Buttons.
        let theme = cx.theme();
        let refresh_btn = Button::new("piv-refresh")
            .icon(Icon::default().path("icons/refresh-cw.svg"))
            .custom(
                ButtonCustomVariant::new(cx)
                    .color(rgb(0x1b1b1d).into())
                    .hover(rgb(0x232325).into())
                    .active(rgb(0x3f3f46).into())
                    .border(theme.border),
            )
            .disabled(self.loading)
            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)));
        let change_pin_btn = PFButton::new(crate::i18n::tr("Change PIN"))
            .id("piv-change-pin")
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .on_click(cx.listener(|this, _, window, cx| this.open_change_pin(false, window, cx)));
        let change_puk_btn = PFButton::new(crate::i18n::tr("Change PUK"))
            .id("piv-change-puk")
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .on_click(cx.listener(|this, _, window, cx| this.open_change_pin(true, window, cx)));
        let unblock_btn = PFButton::new(crate::i18n::tr("Unblock PIN"))
            .id("piv-unblock")
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .on_click(cx.listener(|this, _, window, cx| this.open_unblock_pin(window, cx)));
        let retries_btn = PFButton::new(crate::i18n::tr("Set retries"))
            .id("piv-retries")
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .on_click(cx.listener(|this, _, window, cx| this.open_set_retries(window, cx)));
        let mgm_btn = PFButton::new(crate::i18n::tr("Change key"))
            .id("piv-mgm")
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .on_click(cx.listener(|this, _, window, cx| this.open_change_mgm(window, cx)));
        let reset_btn = Button::new("piv-reset")
            .label(crate::i18n::tr("Reset PIV applet"))
            .danger()
            .disabled(self.loading)
            .on_click(cx.listener(|this, _, window, cx| this.open_reset_dialog(window, cx)));

        // Card information.
        let info_card = {
            let body = match &info {
                Some(i) => {
                    let pin = i
                        .pin
                        .map(|p| {
                            format!(
                                "{}/{}{}",
                                p.left,
                                p.total,
                                if p.is_default { " (default)" } else { "" }
                            )
                        })
                        .unwrap_or_else(|| "—".into());
                    let puk = i
                        .puk
                        .map(|p| {
                            format!(
                                "{}/{}{}",
                                p.left,
                                p.total,
                                if p.is_default { " (default)" } else { "" }
                            )
                        })
                        .unwrap_or_else(|| "—".into());
                    let mgm = format!(
                        "{}{}",
                        piv::algo_label(i.mgm_algo),
                        if i.mgm_default { " (default)" } else { "" }
                    );
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_4()
                        .child(kv(
                            crate::i18n::tr("Firmware"),
                            format!("{}.{}.{}", i.version[0], i.version[1], i.version[2]),
                            theme,
                        ))
                        .child(kv(crate::i18n::tr("Serial"), i.serial.to_string(), theme))
                        .child(kv(crate::i18n::tr("PIN tries"), pin, theme))
                        .child(kv(crate::i18n::tr("PUK tries"), puk, theme))
                        .child(kv(crate::i18n::tr("Management key"), mgm, theme))
                        .into_any_element()
                }
                None => div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(if self.loading {
                        crate::i18n::tr("Reading card…")
                    } else {
                        crate::i18n::tr("Card information unavailable. Refresh to retry.")
                    })
                    .into_any_element(),
            };
            Card::new()
                .title(crate::i18n::tr("Card information"))
                .icon(Icon::default().path("icons/cpu.svg"))
                .header_right(refresh_btn)
                .child(body)
        };

        let slots_card = Card::new()
            .title(crate::i18n::tr("Key slots"))
            .description(crate::i18n::tr("Primary and retired certificate slots"))
            .icon(Icon::default().path("icons/key.svg"))
            .child(
                v_flex()
                    .gap_3()
                    .child(gpui_component::input::Input::new(&self.slot_search).cleanable(true))
                    .child(crate::ui::components::collection::frame(
                        "piv-slots-frame",
                        slot_rows,
                        &self.slot_scroll,
                        list_height,
                        cx,
                    )),
            );

        let pin_card = Card::new()
            .title(crate::i18n::tr("PIN & PUK"))
            .icon(Icon::default().path("icons/lock.svg"))
            .child(
                v_flex()
                    .gap_2()
                    .child(self.action_row(
                        "PIN",
                        crate::i18n::tr("Change the 6–8 digit PIV PIN"),
                        change_pin_btn,
                        theme,
                    ))
                    .child(self.action_row(
                        "PUK",
                        crate::i18n::tr("Change the PIN Unblock Key"),
                        change_puk_btn,
                        theme,
                    ))
                    .child(self.action_row(
                        crate::i18n::tr("Unblock"),
                        crate::i18n::tr("Reset a blocked PIN using the PUK"),
                        unblock_btn,
                        theme,
                    ))
                    .child(self.action_row(
                        crate::i18n::tr("Retry limits"),
                        crate::i18n::tr("Set PIN/PUK retries (resets both to defaults)"),
                        retries_btn,
                        theme,
                    )),
            );

        let mgm_card = Card::new()
            .title(crate::i18n::tr("Management key"))
            .description(crate::i18n::tr(
                "The key that authorises key and certificate changes",
            ))
            .icon(Icon::default().path("icons/key-round.svg"))
            .child(self.action_row(
                crate::i18n::tr("Management key"),
                crate::i18n::tr("Change the PIV management key"),
                mgm_btn,
                theme,
            ));

        let reset_card = Card::new()
            .title(crate::i18n::tr("Reset"))
            .description(crate::i18n::tr("Erase all PIV keys and certificates"))
            .icon(Icon::default().path("icons/trash.svg"))
            .child(self.action_row(
                crate::i18n::tr("Factory reset PIV"),
                crate::i18n::tr("Blocks PIN+PUK then wipes everything. Cannot be undone."),
                reset_btn,
                theme,
            ));

        let content = v_flex()
            .gap_6()
            .child(info_card)
            .child(slots_card)
            .child(pin_card)
            .child(mgm_card)
            .child(reset_card);

        PageView::build(TITLE, SUBTITLE, content, theme).into_any_element()
    }
}
