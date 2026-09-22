//! Small form helpers shared by the applet screens' dialogs.

use gpui::*;
use gpui_component::select::{SelectItem, SelectState};

/// A labelled dropdown option carrying a small integer key (a wire value where
/// one exists — algorithm byte, period, digits — or a 0/1 flag otherwise).
#[derive(Clone, PartialEq)]
pub struct LabeledU8 {
    label: SharedString,
    key: u8,
}

impl SelectItem for LabeledU8 {
    type Value = u8;
    fn title(&self) -> SharedString {
        crate::i18n::text(self.label.as_ref()).into()
    }
    fn value(&self) -> &Self::Value {
        &self.key
    }
}

/// Build a `Select` state from `(label, key)` options with a default row.
pub fn select_state(
    window: &mut Window,
    cx: &mut App,
    options: &[(&str, u8)],
    default_row: usize,
) -> Entity<SelectState<Vec<LabeledU8>>> {
    let opts: Vec<LabeledU8> = options
        .iter()
        .map(|(label, key)| LabeledU8 {
            label: crate::i18n::source_label(label).into(),
            key: *key,
        })
        .collect();
    cx.new(|cx| {
        SelectState::new(
            opts,
            Some(gpui_component::IndexPath::default().row(default_row)),
            window,
            cx,
        )
    })
}

/// Read a select's chosen key by mapping its row back through `options`.
pub fn selected_key(
    sel: &Entity<SelectState<Vec<LabeledU8>>>,
    options: &[(&str, u8)],
    cx: &App,
) -> u8 {
    let row = sel.read(cx).selected_index(cx).map(|p| p.row).unwrap_or(0);
    options.get(row).map(|(_, k)| *k).unwrap_or(options[0].1)
}

/// A full-width information card whose height follows its wrapped text.
pub fn info_card(message: impl Into<SharedString>) -> impl IntoElement {
    gpui_component::h_flex()
        .w_full()
        .min_w_0()
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(0x254675))
        .bg(rgb(0x101e33))
        .text_sm()
        .text_color(rgb(0x93c5fd))
        .child(
            div().flex_shrink_0().child(
                gpui_component::Icon::default()
                    .path("icons/info.svg")
                    .size_4(),
            ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(crate::i18n::text(message.into().as_ref())),
        )
}

/// Dialog-local errors keep invalid values in place, alongside their inputs.
#[derive(Clone, Default)]
pub struct FormErrors(std::rc::Rc<std::cell::RefCell<std::collections::BTreeMap<usize, String>>>);
impl FormErrors {
    pub fn watch(
        &self,
        index: usize,
        input: &Entity<gpui_component::input::InputState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let errors = self.clone();
        let mut previous = input.read(cx).text().to_string();
        window
            .observe(input, cx, move |input, window, cx| {
                let text = input.read(cx).text().to_string();
                if text != previous {
                    previous = text;
                    errors.0.borrow_mut().remove(&index);
                }
                window.refresh();
            })
            .detach();
    }
    pub fn message(&self, index: usize) -> Option<String> {
        self.0.borrow().get(&index).cloned()
    }
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }
    pub fn set(&self, index: usize, message: impl Into<String>) {
        self.0.borrow_mut().insert(index, message.into());
    }
    pub fn valid(&self, window: &mut Window) -> bool {
        window.refresh();
        self.0.borrow().is_empty()
    }
    pub fn required(&self, index: usize, label: &str, value: &str) {
        if value.is_empty() {
            self.set(
                index,
                crate::i18n::format("{0} is required.", &[crate::i18n::text(label)]),
            );
        }
    }
    pub fn field(
        &self,
        index: usize,
        label: &str,
        input: &Entity<gpui_component::input::InputState>,
        required: bool,
    ) -> impl IntoElement {
        use gpui_component::{h_flex, input::Input, v_flex};
        let mut heading = h_flex()
            .w_full()
            .gap_1()
            .child(div().child(crate::i18n::text(label)));
        if required {
            heading = heading.child(div().text_color(rgb(0xef4444)).child("*"));
        }
        let error = self.0.borrow().get(&index).cloned();
        let mut field = v_flex().w_full().flex_shrink_0().gap_2().child(heading);
        let mut editor = Input::new(input);
        if error.is_some() {
            editor = editor.border_color(rgb(0xef4444));
        }
        field = field.child(editor);
        if let Some(error) = error {
            field = field.child(div().text_sm().text_color(rgb(0xef4444)).child(error));
        }
        field
    }
}

/// A known factory value is filled only when metadata confirms it is active.
/// Older cards offer an explicit one-click choice; no VERIFY probing is used.
#[derive(Clone, Copy)]
pub struct DefaultSecret {
    pub value: &'static str,
    pub active: Option<bool>,
}
impl DefaultSecret {
    pub fn initial_value(self) -> &'static str {
        if self.active == Some(true) {
            self.value
        } else {
            ""
        }
    }
    pub fn input(
        self,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<gpui_component::input::InputState> {
        cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx)
                .masked(true)
                .default_value(self.initial_value())
        })
    }
    pub fn automatic(self) -> bool {
        self.active == Some(true)
    }

    pub fn field(
        self,
        errors: &FormErrors,
        index: usize,
        label: &str,
        input: &Entity<gpui_component::input::InputState>,
        required: bool,
    ) -> Option<AnyElement> {
        if self.automatic() {
            return None;
        }
        use gpui_component::{button::Button, v_flex};
        let mut field = v_flex()
            .w_full()
            .gap_2()
            .child(errors.field(index, label, input, required));
        if self.active.is_none() {
            let input = input.clone();
            field = field.child(
                Button::new("use-factory-default")
                    .label(crate::i18n::tr("Use factory default"))
                    .on_click(move |_, w, cx| {
                        input.update(cx, |input, cx| input.set_value(self.value, w, cx))
                    }),
            );
        }
        Some(field.into_any_element())
    }
}

#[cfg(test)]
mod default_secret_tests {
    use super::*;
    #[core::prelude::v1::test]
    fn prefill_requires_positive_device_metadata() {
        for (active, expected) in [(Some(true), "123456"), (Some(false), ""), (None, "")] {
            assert_eq!(
                DefaultSecret {
                    value: "123456",
                    active
                }
                .initial_value(),
                expected
            );
        }
    }
}
