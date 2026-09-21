use crate::ui::components::{card::Card, information, page_view::PageView, tag::Tag};
use crate::ui::screens::about::view_model::AboutViewModel;
use gpui::*;
use gpui_component::{ActiveTheme, Icon, StyledExt, button::Button, h_flex, v_flex};

impl Render for AboutViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        PageView::build(
            "About",
            "Information about the application and its development.",
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .w_full()
                        .max_w(px(1200.0))
                        .child(
                            Card::new().child(
                                v_flex()
                                    .items_center()
                                    .gap_4()
                                    .py_8()
                                    .text_center()
                                    .child(
                                        img("appIcons/in.suyogtandel.picoforge.svg")
                                            .w(px(256.0))
                                            .h(px(256.0)),
                                    )
                                    .child(
                                        h_flex().items_center().justify_center().gap_2()
                                            .child(div().text_2xl().font_bold().text_color(theme.foreground).child("PicoForge"))
                                            .child(img("appIcons/all-patch.svg").w(px(72.)).h(px(46.))),
                                    )
                                    .child(Tag::new(concat!("v", env!("CARGO_PKG_VERSION"))))
                                    .child(div().font_bold().text_color(rgb(0xE34C2D)).child("UNOFFICIAL FORK"))
                                    .child(
                                        div()
                                            .text_color(theme.muted_foreground)
                                            .max_w(px(450.0))
                                            .child(
                                                "An open source commissioning tool for RS-Key and pico-fido security keys. Developed with Rust and GPUI.",
                                            ),
                                    )
                                    .child(
                                        div().w(px(450.)).max_w_full().pt_4().border_t_1().border_color(theme.border)
                                            .child(information::grid()
                                                .child(information::field("Code By", "PicoForge Contributers & BlueFunny", theme))
                                                .child(information::field("Copyright", "©2026 Suyog Tandel", theme)))
                                    )
                                    .child(
                                        h_flex()
                                            .gap_4()
                                            .pt_6()
                                            .child(
                                                Button::new("github_btn")
                                                    .outline()
                                                    .bg(rgb(0x222225))
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .child(
                                                                Icon::default()
                                                                    .path("icons/github.svg")
                                                                    .size_4(),
                                                            )
                                                            .child("GitHub"),
                                                    )
                                                    .on_click(|_, _, cx| {
                                                        cx.open_url("https://github.com/BlueFunny19/picoforge-all")
                                                    }),
                                            )
                                            .child(
                                                Button::new("wiki_btn")
                                                    .outline()
                                                    .bg(rgb(0x222225))
                                                    .child(
                                                        h_flex()
                                                            .gap_2()
                                                            .child(
                                                                Icon::default()
                                                                    .path("icons/book-open.svg")
                                                                    .size_4(),
                                                            )
                                                            .child("Wiki"),
                                                    )
                                                    .on_click(|_, _, cx| {
                                                        cx.open_url(
                                                            "https://github.com/BlueFunny19/picoforge-all/wiki",
                                                        )
                                                    }),
                                            ),
                                    ),
                            ),
                        ),
                ),
            theme,
        )
    }
}
