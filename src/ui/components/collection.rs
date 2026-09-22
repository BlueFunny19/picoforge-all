//! Shared filtering and isolated scrolling for embedded collections.
use super::form::{LabeledU8, select_state};
use gpui::*;
use gpui_component::{
    ActiveTheme, h_flex,
    input::{Input, InputState},
    scroll::{Scrollbar, ScrollbarShow},
    select::{Select, SelectEvent, SelectState},
};

pub fn filter<T: 'static>(
    options: &[(&str, u8)],
    window: &mut Window,
    cx: &mut Context<T>,
) -> Entity<SelectState<Vec<LabeledU8>>> {
    let state = select_state(window, cx, options, 0);
    cx.subscribe(&state, |_, _, _: &SelectEvent<Vec<LabeledU8>>, cx| {
        cx.notify()
    })
    .detach();
    state
}

pub fn toolbar(search: &Entity<InputState>, filter: &Entity<SelectState<Vec<LabeledU8>>>) -> Div {
    h_flex()
        .w_full()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(search).cleanable(true)),
        )
        .child(
            div()
                .w(px(190.))
                .flex_shrink_0()
                .child(Select::new(filter).w_full()),
        )
}

/// Contain wheel events even at the first/last row; the page only scrolls outside this frame.
pub fn frame(
    id: &'static str,
    rows: AnyElement,
    scroll: &UniformListScrollHandle,
    height: f32,
    cx: &App,
) -> impl IntoElement {
    div()
        .id(id)
        .relative()
        .w_full()
        .h(px(height.max(64.) + 16.))
        .flex_shrink_0()
        .border_1()
        .border_color(cx.theme().border)
        .rounded_lg()
        .overflow_hidden()
        .bg(cx.theme().background)
        .p_2()
        .pr_5()
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .child(rows)
        .child(Scrollbar::vertical(scroll).scrollbar_show(ScrollbarShow::Always))
}

/// Each word narrows the result; searches work across labels, IDs, and translated terms.
pub fn matches(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    query
        .split_whitespace()
        .all(|term| text.contains(&term.to_lowercase()))
}
