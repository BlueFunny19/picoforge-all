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
        self.label.clone()
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
            label: (*label).to_string().into(),
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

/// Informational defaults are hints, never a readback of a stored secret.
pub fn info_card(message: impl Into<SharedString>) -> impl IntoElement {
    gpui_component::h_flex()
        .items_start()
        .gap_2()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(0x254675))
        .bg(rgb(0x101e33))
        .text_sm()
        .text_color(rgb(0x93c5fd))
        .child(gpui_component::Icon::default().path("icons/info.svg"))
        .child(div().min_w_0().child(message.into()))
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
            self.set(index, format!("{label} is required."));
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
        let mut heading = h_flex().gap_1().child(label.to_string());
        if required {
            heading = heading.child(div().text_color(rgb(0xef4444)).child("*"));
        }
        let error = self.0.borrow().get(&index).cloned();
        let mut field = v_flex().gap_2().child(heading);
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
