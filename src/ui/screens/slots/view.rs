//! Slots (OTP) screen rendering.

use crate::ui::components::button::PFButton;
use crate::ui::components::card::Card;
use crate::ui::components::page_view::PageView;
use crate::ui::models::device::otp;
use crate::ui::screens::slots::view_model::SlotsViewModel;
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

impl SlotsViewModel {
    fn render_slot_card(&self, slot: u8, cx: &mut Context<Self>) -> AnyElement {
        let info = self.slot_info(slot);
        let configured = info.configured();
        let theme = cx.theme();

        let program_btn = PFButton::new(if configured {
            crate::i18n::tr("Reprogram")
        } else {
            crate::i18n::tr("Program")
        })
        .id(format!("prog-{slot}"))
        .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
        .disabled(self.loading)
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_program_dialog(slot, window, cx);
        }));
        let test_btn = (info.kind == otp::SlotType::ChallengeResponse).then(|| {
            Button::new(SharedString::from(format!("test-{slot}")))
                .label(crate::i18n::tr("Test"))
                .ghost()
                .disabled(self.loading)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_test_dialog(slot, window, cx);
                }))
        });
        let delete_btn = configured.then(|| {
            Button::new(SharedString::from(format!("del-{slot}")))
                .icon(Icon::default().path("icons/trash-2.svg"))
                .ghost()
                .disabled(self.loading)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_delete_dialog(slot, window, cx);
                }))
        });

        v_flex()
            .min_w_0()
            .gap_4()
            .p_4()
            .border_1()
            .border_color(theme.border)
            .rounded_lg()
            .child(
                div()
                    .font_medium()
                    .child(crate::i18n::format("Slot {0}", &[format!("{}", slot)])),
            )
            .child(
                crate::ui::components::information::grid()
                    .child(crate::ui::components::information::field(
                        crate::i18n::tr("Type"),
                        if configured {
                            info.kind.label()
                        } else {
                            crate::i18n::tr("Empty")
                        },
                        theme,
                    ))
                    .child(crate::ui::components::information::field(
                        crate::i18n::tr("Touch confirmation"),
                        if info.touch {
                            crate::i18n::tr("Required")
                        } else {
                            crate::i18n::tr("Off")
                        },
                        theme,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(program_btn)
                    .children(test_btn)
                    .children(delete_btn),
            )
            .into_any_element()
    }
}

impl Render for SlotsViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const TITLE: &str = "Slots";
        const SUBTITLE: &str = "Configurable OTP slots (Yubico OTP protocol).";

        if let Some((heading, body)) = self.gate(cx).message() {
            let theme = cx.theme();
            return PageView::build(TITLE, SUBTITLE, empty_state(heading, body, theme), theme)
                .into_any_element();
        }

        let count = self.slot_count(cx);
        let mut cards = Vec::with_capacity(count as usize);
        for slot in 1..=count {
            cards.push(self.render_slot_card(slot, cx));
        }

        let theme = cx.theme();
        let swap_btn = PFButton::new(crate::i18n::tr("Swap 1 ↔ 2"))
            .id("swap-slots")
            .with_colors(rgb(0x222225), rgb(0x2a2a2d), rgb(0x333336))
            .disabled(self.loading)
            .on_click(cx.listener(|this, _, window, cx| this.open_swap_dialog(window, cx)));
        let refresh_btn = Button::new("refresh-slots")
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
        let toolbar = h_flex().gap_2().child(swap_btn).child(refresh_btn);

        let slots_card = Card::new()
            .title(crate::i18n::tr("Slots"))
            .description(crate::i18n::format(
                "{0} configurable slots",
                &[format!("{}", count)],
            ))
            .icon(Icon::default().path("icons/touch-app.svg"))
            .header_right(toolbar)
            .child(v_flex().gap_2().children(cards));

        let content = v_flex().gap_6().child(slots_card);
        PageView::build(TITLE, SUBTITLE, content, theme).into_any_element()
    }
}
