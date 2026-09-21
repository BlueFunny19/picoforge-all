use gpui::*;
use gpui_component::{Icon, h_flex, v_flex};

pub fn warning(title: &'static str, message: &'static str, danger: bool) -> Div {
    let color = if danger { rgb(0xef4444) } else { rgb(0xf59e0b) };
    v_flex()
        .w_full()
        .gap_2()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(Hsla::from(color).opacity(0.45))
        .text_color(color)
        .child(
            h_flex()
                .gap_2()
                .child(Icon::default().path("icons/triangle-alert.svg"))
                .child(div().font_weight(FontWeight::SEMIBOLD).child(title)),
        )
        .child(div().text_sm().child(message))
}
