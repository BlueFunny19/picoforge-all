//! Consistent label/value presentation shared by device information cards.
use gpui::*;
use gpui_component::{Theme, v_flex};

pub fn field(label: impl Into<SharedString>, value: impl Into<SharedString>, theme: &Theme) -> Div {
    v_flex()
        .min_w_0()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::text(label.into().as_ref())),
        )
        .child(
            div()
                .min_w_0()
                .text_sm()
                .text_color(theme.foreground)
                .child(value.into()),
        )
}

pub fn grid() -> Div {
    div().w_full().min_w_0().grid().grid_cols(2).gap_4()
}
