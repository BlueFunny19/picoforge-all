//! Modal dialog components for PIN prompts, confirmations, and status display.

use super::form::FormErrors;
use gpui::*;
use gpui_component::{
    Disableable, WindowExt,
    button::{Button, ButtonVariant, ButtonVariants},
    h_flex,
    input::{InputEvent, InputState},
    v_flex,
};

type PinPromptCallback = std::rc::Rc<dyn Fn(String, WeakEntity<PinPromptContent>, &mut App)>;
type ConfirmCallback = std::rc::Rc<dyn Fn(WeakEntity<ConfirmContent>, &mut Window, &mut App)>;
type ChangePinCallback =
    std::rc::Rc<dyn Fn(String, String, WeakEntity<ChangePinContent>, &mut App)>;
type SetPinCallback = std::rc::Rc<dyn Fn(String, WeakEntity<SetPinContent>, &mut App)>;

#[derive(Clone)]
enum DialogPhase {
    Input,
    Loading,
    /// Indicates the dialog is blocked on an asynchronous background task,
    /// presenting a specific dynamic status message to guide the user (e.g. "Waiting for touch...").
    LoadingWithMessage(String),
    Success(String),
    Error(String),
}

#[derive(Default)]
struct OperationClock {
    started: Option<std::time::Instant>,
    task: Option<Task<()>>,
}
impl OperationClock {
    fn start<T: 'static>(&mut self, cx: &mut Context<T>) {
        if self.task.is_some() {
            return;
        }
        self.started = Some(std::time::Instant::now());
        self.task = Some(cx.spawn(async |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }
    fn stop(&mut self) {
        self.task = None;
    }
    fn label(&self) -> impl IntoElement {
        let seconds = self.started.map(|t| t.elapsed().as_secs()).unwrap_or(0);
        div().text_sm().text_color(rgb(0xa1a1aa)).child(format!(
            "Elapsed: {}m {:02}s",
            seconds / 60,
            seconds % 60
        ))
    }
}
fn success_message(msg: &str) -> AnyElement {
    v_flex()
        .gap_4()
        .child(
            div()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(rgb(0x18181b))
                .text_sm()
                .text_color(rgb(0x22c55e))
                .child(msg.to_string()),
        )
        .child(
            h_flex().justify_end().child(
                Button::new("done")
                    .label("Done")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            ),
        )
        .into_any_element()
}

/// Dialog content for collecting the FIDO PIN from the user.
pub struct PinPromptContent {
    errors: FormErrors,
    phase: DialogPhase,
    clock: OperationClock,
    description: SharedString,
    warning: Option<SharedString>,
    confirm_label: SharedString,
    pin_input: Entity<InputState>,
    on_confirm: PinPromptCallback,
    _subscription: Subscription,
}

impl PinPromptContent {
    /// Transition the dialog to a loading state with the given message.
    pub fn set_loading_msg(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        self.clock.start(cx);
        self.phase = DialogPhase::LoadingWithMessage(msg.into());
        cx.notify();
    }

    fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.clock.start(cx);
        self.phase = DialogPhase::Loading;
        cx.notify();
    }

    /// Transition the dialog to a success state.
    pub fn set_success(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Success(msg);
        cx.notify();
    }

    /// Transition the dialog to an error state with the given message.
    pub fn set_error(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Error(msg);
        cx.notify();
    }

    fn trigger_confirm(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_) | DialogPhase::Success(_)
        ) {
            return;
        }
        let pin = self.pin_input.read(cx).text().to_string();
        self.errors.clear();
        self.errors.required(0, "PIN", &pin);
        if !self.errors.is_empty() {
            cx.notify();
            return;
        }
        let handle = cx.entity().downgrade();
        self.set_loading(cx);
        (self.on_confirm)(pin, handle, cx);
    }
}

impl Render for PinPromptContent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let DialogPhase::Success(msg) = &self.phase {
            return success_message(msg);
        }
        if matches!(
            self.phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
        ) {
            return v_flex()
                .gap_4()
                .items_center()
                .child(match &self.phase {
                    DialogPhase::LoadingWithMessage(msg) => msg.clone(),
                    _ => "Working…".into(),
                })
                .child(self.clock.label())
                .child(Button::new("working").label("Working…").loading(true))
                .into_any_element();
        }
        let mut form = v_flex().gap_4().child(self.description.clone());
        if let Some(warning) = &self.warning {
            form = form.child(
                div()
                    .text_sm()
                    .text_color(rgb(0xef4444))
                    .child(warning.clone()),
            );
        }
        if let DialogPhase::Error(message) = &self.phase {
            form = form.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x18181b))
                    .text_sm()
                    .text_color(rgb(0xef4444))
                    .child(render_error_message(message.clone())),
            );
        }
        form.child(self.errors.field(0, "PIN", &self.pin_input, true))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    )
                    .child(
                        Button::new("confirm")
                            .primary()
                            .label(self.confirm_label.clone())
                            .on_click(cx.listener(|this, _, _, cx| this.trigger_confirm(cx))),
                    ),
            )
            .into_any_element()
    }
}

/// Open a PIN prompt dialog and return the submitted PIN.
#[allow(clippy::too_many_arguments)]
pub fn open_pin_prompt(
    title: &str,
    description: &str,
    placeholder: &str,
    warning: Option<&str>,
    confirm_label: &str,
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(String, WeakEntity<PinPromptContent>, &mut App) + 'static,
) {
    let title_str = SharedString::from(title.to_string());
    let description = SharedString::from(description.to_string());
    let placeholder_str = SharedString::from(placeholder.to_string());
    let warning = warning.map(|w| SharedString::from(w.to_string()));
    let confirm_label = SharedString::from(confirm_label.to_string());

    let pin_input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder_str)
            .masked(true)
    });

    let dialog_title = title_str.clone();
    let pin_for_sub = pin_input.clone();

    let content = cx.new(|cx| {
        let sub = cx.subscribe(&pin_for_sub, |this: &mut PinPromptContent, _, event, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.trigger_confirm(cx);
            }
        });

        PinPromptContent {
            errors: FormErrors::default(),
            phase: DialogPhase::Input,
            clock: OperationClock::default(),
            description,
            warning,
            confirm_label,
            pin_input: pin_for_sub,
            on_confirm: std::rc::Rc::new(on_confirm),
            _subscription: sub,
        }
    });

    let errors = content.read(cx).errors.clone();
    errors.watch(0, &content.read(cx).pin_input.clone(), window, cx);
    // The title and keyboard policy live outside the content entity.
    window
        .observe(&content, cx, |_, window, _| window.refresh())
        .detach();
    window.open_dialog(cx, move |dialog, _, cx| {
        let phase = &content.read(cx).phase;
        let busy = matches!(
            phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
        );
        dialog
            .title(if matches!(phase, DialogPhase::Error(_)) {
                SharedString::from("Error")
            } else if matches!(phase, DialogPhase::Success(_)) {
                SharedString::from("Success")
            } else {
                dialog_title.clone()
            })
            .keyboard(!busy)
            .child(content.clone())
            .overlay_closable(false)
            .close_button(false)
    });
}

/// Dialog content for confirming a destructive or irreversible action.
pub struct ConfirmContent {
    phase: DialogPhase,
    clock: OperationClock,
    message: String,
    ok_label: SharedString,
    ok_variant: ButtonVariant,
    on_ok: ConfirmCallback,
}

impl ConfirmContent {
    fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.clock.start(cx);
        self.phase = DialogPhase::Loading;
        cx.notify();
    }

    /// Transition the dialog to a success state.
    pub fn set_success(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Success(msg);
        cx.notify();
    }

    /// Transition the dialog to an error state with the given message.
    pub fn set_error(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Error(msg);
        cx.notify();
    }
}

impl Render for ConfirmContent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let phase = self.phase.clone();

        match &phase {
            DialogPhase::Success(msg) => success_message(msg),

            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_) => v_flex()
                .gap_4()
                .child(self.clock.label())
                .child(self.message.clone())
                .child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(Button::new("cancel").label("Cancel").disabled(true))
                        .child(
                            Button::new("ok")
                                .with_variant(self.ok_variant)
                                .label("Loading...")
                                .loading(true),
                        ),
                )
                .into_any_element(),

            DialogPhase::Error(err_msg) => {
                let ok_label = self.ok_label.clone();
                let ok_variant = self.ok_variant;
                let on_ok = self.on_ok.clone();
                let handle = cx.entity().downgrade();

                v_flex()
                    .gap_4()
                    .child(self.message.clone())
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x18181b))
                            .text_color(rgb(0xef4444))
                            .text_sm()
                            .child(render_error_message(err_msg.clone())),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(Button::new("cancel").label("Cancel").on_click(
                                |_, window, cx| {
                                    window.close_dialog(cx);
                                },
                            ))
                            .child(
                                Button::new("ok")
                                    .with_variant(ok_variant)
                                    .label(ok_label)
                                    .on_click(move |_, window, cx| {
                                        if let Some(h) = handle.upgrade() {
                                            h.update(cx, |this, cx| this.set_loading(cx));
                                        }
                                        on_ok(handle.clone(), window, cx);
                                    }),
                            ),
                    )
                    .into_any_element()
            }

            DialogPhase::Input => {
                let ok_label = self.ok_label.clone();
                let ok_variant = self.ok_variant;
                let on_ok = self.on_ok.clone();
                let handle = cx.entity().downgrade();

                v_flex()
                    .gap_4()
                    .child(self.message.clone())
                    .child(
                        h_flex()
                            .justify_end()
                            .gap_2()
                            .child(Button::new("cancel").label("Cancel").on_click(
                                |_, window, cx| {
                                    window.close_dialog(cx);
                                },
                            ))
                            .child(
                                Button::new("ok")
                                    .with_variant(ok_variant)
                                    .label(ok_label)
                                    .on_click(move |_, window, cx| {
                                        if let Some(h) = handle.upgrade() {
                                            h.update(cx, |this, cx| this.set_loading(cx));
                                        }
                                        on_ok(handle.clone(), window, cx);
                                    }),
                            ),
                    )
                    .into_any_element()
            }
        }
    }
}

/// Open a confirmation dialog and return whether the user accepted.
pub fn open_confirm(
    title: &str,
    message: String,
    ok_label: &str,
    ok_variant: ButtonVariant,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl Fn(WeakEntity<ConfirmContent>, &mut Window, &mut App) + 'static,
) {
    let title_str = SharedString::from(title.to_string());
    let dialog_title = title_str.clone();

    let content = cx.new(|_cx| ConfirmContent {
        phase: DialogPhase::Input,
        clock: OperationClock::default(),
        message,
        ok_label: SharedString::from(ok_label.to_string()),
        ok_variant,
        on_ok: std::rc::Rc::new(on_ok),
    });

    // The title and keyboard policy live outside the content entity.
    window
        .observe(&content, cx, |_, window, _| window.refresh())
        .detach();
    window.open_dialog(cx, move |dialog, _, cx| {
        let phase = &content.read(cx).phase;
        let busy = matches!(
            phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
        );
        dialog
            .title(if matches!(phase, DialogPhase::Error(_)) {
                SharedString::from("Error")
            } else if matches!(phase, DialogPhase::Success(_)) {
                SharedString::from("Success")
            } else {
                dialog_title.clone()
            })
            .keyboard(!busy)
            .child(content.clone())
            .overlay_closable(false)
            .close_button(false)
    });
}

/// Dialog content for changing an existing FIDO PIN.
pub struct ChangePinContent {
    errors: FormErrors,
    phase: DialogPhase,
    clock: OperationClock,
    current_pin: Entity<InputState>,
    new_pin: Entity<InputState>,
    confirm_pin: Entity<InputState>,
    on_confirm: ChangePinCallback,
    _subscriptions: Vec<Subscription>,
}

impl ChangePinContent {
    fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.clock.start(cx);
        self.phase = DialogPhase::Loading;
        cx.notify();
    }

    /// Transition the dialog to a success state.
    pub fn set_success(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Success(msg);
        cx.notify();
    }

    /// Transition the dialog to an error state with the given message.
    pub fn set_error(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Error(msg);
        cx.notify();
    }

    fn trigger_confirm(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_) | DialogPhase::Success(_)
        ) {
            return;
        }

        let current_pin_text = self.current_pin.read(cx).text().to_string();
        let new_pin_text = self.new_pin.read(cx).text().to_string();
        let confirm_pin_text = self.confirm_pin.read(cx).text().to_string();

        self.errors.clear();
        self.errors.required(0, "Current PIN", &current_pin_text);
        self.errors.required(1, "New PIN", &new_pin_text);
        self.errors.required(2, "Repeat new PIN", &confirm_pin_text);
        if !new_pin_text.is_empty() && new_pin_text.chars().count() < 4 {
            self.errors
                .set(1, "PIN must contain at least 4 characters.");
        }
        if !confirm_pin_text.is_empty() && new_pin_text != confirm_pin_text {
            self.errors.set(2, "New PIN entries do not match.");
        }
        if !self.errors.is_empty() {
            cx.notify();
            return;
        }

        let handle = cx.entity().downgrade();
        self.set_loading(cx);
        (self.on_confirm)(current_pin_text, new_pin_text, handle, cx);
    }
}

impl Render for ChangePinContent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let DialogPhase::Success(msg) = &self.phase {
            return success_message(msg);
        }
        if matches!(
            self.phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
        ) {
            return v_flex()
                .gap_4()
                .items_center()
                .child(match &self.phase {
                    DialogPhase::LoadingWithMessage(msg) => msg.clone(),
                    _ => "Working…".into(),
                })
                .child(self.clock.label())
                .child(Button::new("working").label("Working…").loading(true))
                .into_any_element();
        }
        let mut form = v_flex().gap_4().child(SharedString::from(
            "Enter your current PIN and choose a new one.",
        ));

        if let DialogPhase::Error(message) = &self.phase {
            form = form.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x18181b))
                    .text_sm()
                    .text_color(rgb(0xef4444))
                    .child(render_error_message(message.clone())),
            );
        }
        form.child(self.errors.field(0, "Current PIN", &self.current_pin, true))
            .child(self.errors.field(1, "New PIN", &self.new_pin, true))
            .child(
                self.errors
                    .field(2, "Repeat new PIN", &self.confirm_pin, true),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    )
                    .child(
                        Button::new("confirm")
                            .primary()
                            .label(SharedString::from("Save"))
                            .on_click(cx.listener(|this, _, _, cx| this.trigger_confirm(cx))),
                    ),
            )
            .into_any_element()
    }
}

/// Open a dialog to change the FIDO PIN.
pub fn open_change_pin(
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(String, String, WeakEntity<ChangePinContent>, &mut App) + 'static,
) {
    let current_pin = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Enter current PIN")
            .masked(true)
    });
    let new_pin = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Enter new PIN")
            .masked(true)
    });
    let confirm_pin = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Confirm new PIN")
            .masked(true)
    });

    let confirm_for_sub = confirm_pin.clone();

    let content = cx.new(|cx| {
        let sub = cx.subscribe(
            &confirm_for_sub,
            |this: &mut ChangePinContent, _, event, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.trigger_confirm(cx);
                }
            },
        );

        ChangePinContent {
            errors: FormErrors::default(),
            phase: DialogPhase::Input,
            clock: OperationClock::default(),
            current_pin,
            new_pin,
            confirm_pin: confirm_for_sub,
            on_confirm: std::rc::Rc::new(on_confirm),
            _subscriptions: vec![sub],
        }
    });

    let errors = content.read(cx).errors.clone();
    errors.watch(0, &content.read(cx).current_pin.clone(), window, cx);
    errors.watch(1, &content.read(cx).new_pin.clone(), window, cx);
    errors.watch(2, &content.read(cx).confirm_pin.clone(), window, cx);
    // The title and keyboard policy live outside the content entity.
    window
        .observe(&content, cx, |_, window, _| window.refresh())
        .detach();
    window.open_dialog(cx, move |dialog, _, cx| {
        let phase = &content.read(cx).phase;
        dialog
            .title(if matches!(phase, DialogPhase::Error(_)) {
                "Error"
            } else if matches!(phase, DialogPhase::Success(_)) {
                "Success"
            } else {
                "Change PIN"
            })
            .keyboard(!matches!(
                phase,
                DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
            ))
            .child(content.clone())
            .overlay_closable(false)
            .close_button(false)
    });
}

/// Dialog content for setting an initial FIDO PIN.
pub struct SetPinContent {
    errors: FormErrors,
    phase: DialogPhase,
    clock: OperationClock,
    new_pin: Entity<InputState>,
    confirm_pin: Entity<InputState>,
    on_confirm: SetPinCallback,
    _subscriptions: Vec<Subscription>,
}

impl SetPinContent {
    fn set_loading(&mut self, cx: &mut Context<Self>) {
        self.clock.start(cx);
        self.phase = DialogPhase::Loading;
        cx.notify();
    }

    /// Transition the dialog to a success state.
    pub fn set_success(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Success(msg);
        cx.notify();
    }

    /// Transition the dialog to an error state with the given message.
    pub fn set_error(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Error(msg);
        cx.notify();
    }

    fn trigger_confirm(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_) | DialogPhase::Success(_)
        ) {
            return;
        }

        let new_pin_text = self.new_pin.read(cx).text().to_string();
        let confirm_pin_text = self.confirm_pin.read(cx).text().to_string();

        self.errors.clear();
        self.errors.required(1, "New PIN", &new_pin_text);
        self.errors.required(2, "Repeat new PIN", &confirm_pin_text);
        if !new_pin_text.is_empty() && new_pin_text.chars().count() < 4 {
            self.errors
                .set(1, "PIN must contain at least 4 characters.");
        }
        if !confirm_pin_text.is_empty() && new_pin_text != confirm_pin_text {
            self.errors.set(2, "New PIN entries do not match.");
        }
        if !self.errors.is_empty() {
            cx.notify();
            return;
        }

        let handle = cx.entity().downgrade();
        self.set_loading(cx);
        (self.on_confirm)(new_pin_text, handle, cx);
    }
}

impl Render for SetPinContent {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let DialogPhase::Success(msg) = &self.phase {
            return success_message(msg);
        }
        if matches!(
            self.phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
        ) {
            return v_flex()
                .gap_4()
                .items_center()
                .child(match &self.phase {
                    DialogPhase::LoadingWithMessage(msg) => msg.clone(),
                    _ => "Working…".into(),
                })
                .child(self.clock.label())
                .child(Button::new("working").label("Working…").loading(true))
                .into_any_element();
        }
        let mut form = v_flex()
            .gap_4()
            .child(SharedString::from("Choose a PIN for your pico-key."));

        if let DialogPhase::Error(message) = &self.phase {
            form = form.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0x18181b))
                    .text_sm()
                    .text_color(rgb(0xef4444))
                    .child(render_error_message(message.clone())),
            );
        }
        form.child(self.errors.field(1, "New PIN", &self.new_pin, true))
            .child(
                self.errors
                    .field(2, "Repeat new PIN", &self.confirm_pin, true),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                    )
                    .child(
                        Button::new("confirm")
                            .primary()
                            .label(SharedString::from("Save"))
                            .on_click(cx.listener(|this, _, _, cx| this.trigger_confirm(cx))),
                    ),
            )
            .into_any_element()
    }
}

/// Open a dialog to set an initial FIDO PIN.
pub fn open_setup_pin(
    window: &mut Window,
    cx: &mut App,
    on_confirm: impl Fn(String, WeakEntity<SetPinContent>, &mut App) + 'static,
) {
    let new_pin = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Enter new PIN")
            .masked(true)
    });
    let confirm_pin = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder("Confirm new PIN")
            .masked(true)
    });

    let confirm_for_sub = confirm_pin.clone();

    let content = cx.new(|cx| {
        let sub = cx.subscribe(
            &confirm_for_sub,
            |this: &mut SetPinContent, _, event, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.trigger_confirm(cx);
                }
            },
        );

        SetPinContent {
            errors: FormErrors::default(),
            phase: DialogPhase::Input,
            clock: OperationClock::default(),
            new_pin,
            confirm_pin: confirm_for_sub,
            on_confirm: std::rc::Rc::new(on_confirm),
            _subscriptions: vec![sub],
        }
    });

    let errors = content.read(cx).errors.clone();
    errors.watch(1, &content.read(cx).new_pin.clone(), window, cx);
    errors.watch(2, &content.read(cx).confirm_pin.clone(), window, cx);
    // The title and keyboard policy live outside the content entity.
    window
        .observe(&content, cx, |_, window, _| window.refresh())
        .detach();
    window.open_dialog(cx, move |dialog, _, cx| {
        let phase = &content.read(cx).phase;
        dialog
            .title(if matches!(phase, DialogPhase::Error(_)) {
                "Error"
            } else if matches!(phase, DialogPhase::Success(_)) {
                "Success"
            } else {
                "Set Up PIN"
            })
            .keyboard(!matches!(
                phase,
                DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
            ))
            .child(content.clone())
            .overlay_closable(false)
            .close_button(false)
    });
}
/// Dialog content for showing operation progress, success, or error.
pub struct StatusContent {
    phase: DialogPhase,
    clock: OperationClock,
}

impl StatusContent {
    /// Transitions the dialog into a loading state while displaying a custom, dynamic status message.
    /// Useful for multi-step background operations where user context needs to be updated.
    pub fn set_loading(&mut self, msg: impl Into<String>, cx: &mut Context<Self>) {
        self.clock.start(cx);
        self.phase = DialogPhase::LoadingWithMessage(msg.into());
        cx.notify();
    }

    pub fn set_success(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Success(msg);
        cx.notify();
    }

    pub fn set_error(&mut self, msg: String, cx: &mut Context<Self>) {
        self.clock.stop();
        self.phase = DialogPhase::Error(msg);
        cx.notify();
    }
}

impl Render for StatusContent {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let phase = self.phase.clone();

        match &phase {
            DialogPhase::Success(msg) => success_message(msg),

            DialogPhase::Error(err_msg) => {
                v_flex()
                    .gap_4()
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x18181b))
                            .text_color(rgb(0xef4444))
                            .text_sm()
                            .child(render_error_message(err_msg.clone())),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .child(Button::new("close").label("Close").on_click(
                                |_, window, cx| {
                                    window.close_dialog(cx);
                                },
                            )),
                    )
                    .into_any_element()
            }

            _ => v_flex()
                .gap_4()
                .items_center()
                .child(match &phase {
                    DialogPhase::LoadingWithMessage(msg) => msg.clone(),
                    _ => "Applying configuration…".into(),
                })
                .child(self.clock.label())
                .child(
                    Button::new("loading")
                        .primary()
                        .label("Working…")
                        .loading(true),
                )
                .into_any_element(),
        }
    }
}

/// Open a status dialog showing progress, success, or error state.
pub fn open_status_dialog(
    title: &str,
    window: &mut Window,
    cx: &mut App,
) -> WeakEntity<StatusContent> {
    let title_str = SharedString::from(title.to_string());
    let dialog_title = title_str.clone();

    let content = cx.new(|cx| {
        let mut clock = OperationClock::default();
        clock.start(cx);
        StatusContent {
            phase: DialogPhase::Loading,
            clock,
        }
    });

    let handle = content.downgrade();

    // The title and keyboard policy live outside the content entity.
    window
        .observe(&content, cx, |_, window, _| window.refresh())
        .detach();
    window.open_dialog(cx, move |dialog, _, cx| {
        let phase = &content.read(cx).phase;
        let busy = matches!(
            phase,
            DialogPhase::Loading | DialogPhase::LoadingWithMessage(_)
        );
        dialog
            .title(if matches!(phase, DialogPhase::Error(_)) {
                SharedString::from("Error")
            } else if matches!(phase, DialogPhase::Success(_)) {
                SharedString::from("Success")
            } else {
                dialog_title.clone()
            })
            .keyboard(!busy)
            .child(content.clone())
            .overlay_closable(false)
            .close_button(false)
    });

    handle
}

fn render_error_message(msg: String) -> impl IntoElement {
    let troubleshooting_phrase = "troubleshooting guide";
    let url = "https://github.com/librekeys/picoforge/wiki/Troubleshooting#1-my-key-is-not-detected-by-picoforge-or-picoforge-displays-a-device-status-of-online---fido-and-there-are-some-settings-that-i-cannot-configure";

    if msg.contains(troubleshooting_phrase) {
        v_flex()
            .child("The device firmware does not support being configured in fido only communication mode.")
            .child(
                h_flex()
                    .gap_1()
                    .child("Have a look at the")
                    .child(
                        div()
                            .text_color(rgb(0x3b82f6))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.open_url(url);
                            })
                            .child(troubleshooting_phrase.to_string()),
                    )
                    .child("to fix this"),
            )
    } else {
        div().child(msg)
    }
}
