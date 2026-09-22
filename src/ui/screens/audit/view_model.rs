//! View model for the Audit screen. export and verify the device's
//! tamper-evident security journal.

use crate::i18n::LocalizedPlaceholder;
use crate::ui::DialogSubmit;
use crate::ui::app::AppModels;
use crate::ui::components::applet_gate::AppletGate;
use crate::ui::components::dialog;
use crate::ui::components::dialog::StatusContent;
use crate::ui::components::form::{FormErrors, LabeledU8, info_card};
use crate::ui::models::device::{DeviceEvent, DeviceRepo, FirmwareType, audit};
use gpui::*;
use gpui_component::WindowExt;
use gpui_component::button::ButtonVariants;
use gpui_component::input::InputState;

pub(super) const EVENT_FILTERS: &[(&str, u8)] = &[
    ("All events", 0),
    ("Authentication", 1),
    ("PIN and security", 2),
    ("Device", 3),
    ("Backup", 4),
    ("Verification", 5),
];
pub struct AuditViewModel {
    pub(super) device: Entity<DeviceRepo>,
    pub(super) event_scroll: UniformListScrollHandle,
    pub(super) event_filter: Entity<gpui_component::select::SelectState<Vec<LabeledU8>>>,
    pub(super) event_search: Entity<InputState>,
    pub(super) journal: Option<audit::AuditJournal>,
    pub(super) verification: Option<audit::AuditVerification>,
    /// Whether journalling is currently on. `None` until the status is read (the
    /// query is ungated, so it loads automatically). Journalling is opt-in.
    pub(super) enabled: Option<bool>,
    pub(super) loading: bool,
    pub(super) status_loading: bool,
    pub(super) status_error: Option<String>,
    _status_task: Option<Task<()>>,
    status_epoch: u64,
    status_serial: Option<String>,
    _task: Option<Task<()>>,
}

impl AuditViewModel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let device = models.device.clone();
        cx.subscribe(&device, |this: &mut Self, _, _: &DeviceEvent, cx| {
            let serial = this
                .device
                .read(cx)
                .status
                .as_ref()
                .map(|s| s.info.serial.clone());
            if this.status_serial != serial {
                this.status_serial = serial;
                this.journal = None;
                this.verification = None;
                this.enabled = None;
                this.status_epoch += 1;
                this.status_loading = false;
                this.status_error = None;
            }
            if this.enabled.is_none() && this.status_error.is_none() {
                this.refresh_status(cx);
            }
            cx.notify();
        })
        .detach();
        let status_serial = device
            .read(cx)
            .status
            .as_ref()
            .map(|s| s.info.serial.clone());
        let event_filter = crate::ui::components::collection::filter(EVENT_FILTERS, window, cx);
        let event_search =
            cx.new(|cx| InputState::new(window, cx).localized_placeholder("Search events", cx));
        cx.subscribe(
            &event_search,
            |_, _, _: &gpui_component::input::InputEvent, cx| cx.notify(),
        )
        .detach();
        let mut this = Self {
            event_scroll: UniformListScrollHandle::new(),
            event_filter,
            event_search,
            device,
            status_serial,
            journal: None,
            verification: None,
            enabled: None,
            loading: false,
            status_loading: false,
            status_error: None,
            _status_task: None,
            status_epoch: 0,
            _task: None,
        };
        this.refresh_status(cx);
        this
    }

    /// Load whether journalling is on (ungated. no PIN, no touch).
    pub(super) fn refresh_status(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.status_loading || self.gate(cx) != AppletGate::Ready {
            return;
        }
        self.status_loading = true;
        self.status_error = None;
        let epoch = self.status_epoch;
        cx.notify();
        let weak = cx.entity().downgrade();
        self._status_task = Some(cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async { DeviceRepo::audit_status_blocking() })
                .await;
            let _ = weak.update(cx, |this, cx| {
                if this.status_epoch != epoch {
                    return;
                }
                this.status_loading = false;
                match res {
                    Ok(enabled) => this.enabled = Some(enabled),
                    Err(error) => this.status_error = Some(error),
                }
                cx.notify();
            });
        }));
    }

    // ── Enable / disable journalling (PIN + touch) ──────────────────────────

    pub(super) fn open_toggle(
        &mut self,
        enable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .device
            .read(cx)
            .fido_info
            .as_ref()
            .and_then(|f| f.options.get("clientPin").copied())
            == Some(false)
        {
            let status = dialog::open_status_dialog(
                if enable {
                    crate::i18n::tr("Enabling event recording")
                } else {
                    crate::i18n::tr("Disabling event recording")
                },
                window,
                cx,
            );
            self.run_toggle(enable, None, status, cx);
            return;
        }
        let pin = Self::pin_input(window, cx);
        let view = cx.entity().downgrade();
        let submit = {
            let pin = pin.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let p = pin.read(cx).text().to_string();
                let p = (!p.is_empty()).then_some(p);
                window.close_dialog(cx);
                let status = dialog::open_status_dialog(
                    if enable {
                        crate::i18n::tr("Enabling event recording")
                    } else {
                        crate::i18n::tr("Disabling event recording")
                    },
                    window,
                    cx,
                );
                let _ = view.update(cx, |this, cx| this.run_toggle(enable, p, status, cx));
            })
        };
        let (title, body) = if enable {
            (
                crate::i18n::tr("Enable event recording"),
                crate::i18n::tr("Start recording security events."),
            )
        } else {
            (
                crate::i18n::tr("Disable event recording"),
                crate::i18n::tr("Stop recording security events."),
            )
        };
        self.open_gate_dialog(
            title,
            body,
            pin,
            None,
            submit,
            window,
            cx,
            if enable {
                crate::i18n::tr("Enable")
            } else {
                crate::i18n::tr("Disable")
            },
        );
    }

    fn run_toggle(
        &mut self,
        enable: bool,
        pin: Option<String>,
        status: WeakEntity<StatusContent>,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        self.loading = true;
        let _ = status.update(cx, |d, cx| {
            d.set_loading(crate::ui::components::copy::CONFIRM_ON_DEVICE, cx)
        });
        cx.notify();
        let weak = cx.entity().downgrade();
        self._task = Some(cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { DeviceRepo::audit_set_enabled_blocking(enable, pin) })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.loading = false;
                match res {
                    Ok(on) => {
                        this.enabled = Some(on);
                        let _ = status.update(cx, |d, cx| {
                            d.set_success(
                                crate::i18n::format(
                                    "Event recording {0}.",
                                    &[format!(
                                        "{}",
                                        if on {
                                            crate::i18n::tr("enabled")
                                        } else {
                                            crate::i18n::tr("disabled")
                                        }
                                    )],
                                ),
                                cx,
                            )
                        });
                    }
                    Err(e) => {
                        let _ = status.update(cx, |d, cx| d.set_error(e, cx));
                    }
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn gate(&self, cx: &App) -> AppletGate {
        let repo = self.device.read(cx);
        match &repo.status {
            None => AppletGate::Unsupported,
            Some(s) if !matches!(s.firmware_type, FirmwareType::RSKey | FirmwareType::PicoAll) => {
                AppletGate::Unsupported
            }
            Some(_) => AppletGate::Ready,
        }
    }

    /// Masked, optional PIN input (blank = authorise by touch).
    fn pin_input(window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .localized_placeholder("Enter your FIDO PIN", cx)
        })
    }

    // ── Read log ────────────────────────────────────────────────────────

    pub(super) fn open_read(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .device
            .read(cx)
            .fido_info
            .as_ref()
            .and_then(|f| f.options.get("clientPin").copied())
            == Some(false)
        {
            let status = dialog::open_status_dialog(crate::i18n::tr("Reading log"), window, cx);
            self.run_read(None, status, cx);
            return;
        }
        let pin = Self::pin_input(window, cx);
        let view = cx.entity().downgrade();
        let submit = {
            let pin = pin.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let p = pin.read(cx).text().to_string();
                let p = (!p.is_empty()).then_some(p);
                window.close_dialog(cx);
                let status = dialog::open_status_dialog(crate::i18n::tr("Reading log"), window, cx);
                let _ = view.update(cx, |this, cx| this.run_read(p, status, cx));
            })
        };
        self.open_gate_dialog(
            crate::i18n::tr("Read log"),
            crate::i18n::tr("Load the recorded security events."),
            pin,
            None,
            submit,
            window,
            cx,
            crate::i18n::tr("Read log"),
        );
    }

    fn run_read(
        &mut self,
        pin: Option<String>,
        status: WeakEntity<StatusContent>,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        self.loading = true;
        let _ = status.update(cx, |d, cx| {
            d.set_loading(crate::ui::components::copy::CONFIRM_ON_DEVICE, cx)
        });
        cx.notify();
        let weak = cx.entity().downgrade();
        self._task = Some(cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { DeviceRepo::audit_log_blocking(pin) })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.loading = false;
                match res {
                    Ok(journal) => {
                        let n = journal.entries.len();
                        this.journal = Some(journal);
                        this.verification = None;
                        let _ = status.update(cx, |d, cx| {
                            d.set_success(
                                crate::i18n::format("Loaded {0} events.", &[format!("{}", n)]),
                                cx,
                            )
                        });
                    }
                    Err(e) => {
                        let _ = status.update(cx, |d, cx| d.set_error(e, cx));
                    }
                }
                cx.notify();
            });
        }));
    }

    // ── Verify checkpoint ───────────────────────────────────────────────────

    pub(super) fn open_verify(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pin = Self::pin_input(window, cx);
        let expect = cx.new(|cx| {
            InputState::new(window, cx)
                .localized_placeholder("Device fingerprint or public key", cx)
        });
        let view = cx.entity().downgrade();
        let submit = {
            let pin = pin.clone();
            let expect = expect.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let p = pin.read(cx).text().to_string();
                let p = (!p.is_empty()).then_some(p);
                let e = expect.read(cx).text().to_string();
                let e = (!e.trim().is_empty()).then_some(e);
                window.close_dialog(cx);
                let status =
                    dialog::open_status_dialog(crate::i18n::tr("Verifying log"), window, cx);
                let _ = view.update(cx, |this, cx| this.run_verify(p, e, status, cx));
            })
        };
        self.open_gate_dialog(
            crate::i18n::tr("Verify log"),
            crate::i18n::tr(
                "Check the log’s signature. Add a saved device key to also check its identity.",
            ),
            pin,
            Some((crate::i18n::tr("Saved device key (optional)"), expect)),
            submit,
            window,
            cx,
            crate::i18n::tr("Verify"),
        );
    }

    fn run_verify(
        &mut self,
        pin: Option<String>,
        expect: Option<String>,
        status: WeakEntity<StatusContent>,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        self.loading = true;
        let _ = status.update(cx, |d, cx| {
            d.set_loading(crate::ui::components::copy::CONFIRM_ON_DEVICE, cx)
        });
        cx.notify();
        let weak = cx.entity().downgrade();
        self._task = Some(cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { DeviceRepo::audit_verify_blocking(pin, expect) })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.loading = false;
                match res {
                    Ok(v) => {
                        let msg = if v.authentic() {
                            crate::i18n::tr("Log verified.").to_string()
                        } else if !v.signature_ok {
                            crate::i18n::tr("The log signature is invalid.").to_string()
                        } else if !v.head_matches {
                            crate::i18n::tr(
                                "The log changed during verification. Read it again and retry.",
                            )
                            .to_string()
                        } else {
                            crate::i18n::tr("The device does not match the saved key.").to_string()
                        };
                        this.journal = Some(v.journal.clone());
                        let authentic = v.authentic();
                        this.verification = Some(v);
                        let _ = status.update(cx, |d, cx| {
                            if authentic {
                                d.set_success(msg, cx)
                            } else {
                                d.set_error(msg, cx)
                            }
                        });
                    }
                    Err(e) => {
                        let _ = status.update(cx, |d, cx| d.set_error(e, cx));
                    }
                }
                cx.notify();
            });
        }));
    }

    /// A dialog with an optional-PIN field and an optional second field.
    fn open_gate_dialog(
        &self,
        title: &'static str,
        body: &'static str,
        pin: Entity<InputState>,
        extra: Option<(&'static str, Entity<InputState>)>,
        submit: DialogSubmit,
        window: &mut Window,
        cx: &mut Context<Self>,
        action: &'static str,
    ) {
        let pin_state = self
            .device
            .read(cx)
            .fido_info
            .as_ref()
            .and_then(|f| f.options.get("clientPin").copied());
        let pin_required = pin_state == Some(true);
        let errors = FormErrors::default();
        errors.watch(0, &pin, window, cx);
        let submit = {
            let errors = errors.clone();
            let pin = pin.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                errors.clear();
                if pin_required {
                    errors.required(0, "FIDO PIN", &pin.read(cx).text().to_string());
                }
                if !errors.valid(window) {
                    return;
                }
                submit(window, cx);
            })
        };
        window.open_dialog(cx, move |dialog, _w, _| {
            let pin = pin.clone();
            let extra = extra.clone();
            let ok = submit.clone();
            let btn = submit.clone();
            let mut fields = gpui_component::v_flex().gap_3().pb_2();
            if pin_state != Some(false) {
                fields = fields.child(errors.field(
                    0,
                    if pin_required {
                        "FIDO PIN"
                    } else {
                        crate::i18n::tr("FIDO PIN (if set)")
                    },
                    &pin,
                    pin_required,
                ));
            } else {
                fields = fields.child(info_card(crate::ui::components::copy::CONFIRM_ON_DEVICE));
            }
            if let Some((label, input)) = &extra {
                fields = fields
                    .child(crate::i18n::text(label))
                    .child(gpui_component::input::Input::new(input));
            }
            dialog
                .title(crate::i18n::text(title))
                .child(
                    gpui_component::v_flex()
                        .w_full()
                        .gap_4()
                        .child(div().child(body))
                        .child(fields),
                )
                .on_ok(move |_, window, cx| {
                    ok(window, cx);
                    false
                })
                .footer(move |_, _w, _c, _| {
                    let s = btn.clone();
                    vec![
                        gpui_component::button::Button::new("cancel")
                            .label(crate::i18n::tr("Cancel"))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        gpui_component::button::Button::new("run")
                            .primary()
                            .label(action)
                            .on_click(move |_, window, cx| s(window, cx)),
                    ]
                })
        });
    }
}
