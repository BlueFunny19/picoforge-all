//! View model for the Backup screen — wallet-style FIDO seed export/restore.

use crate::i18n::LocalizedPlaceholder;
use crate::ui::DialogSubmit;
use crate::ui::app::AppModels;
use crate::ui::components::applet_gate::AppletGate;
use crate::ui::components::dialog;
use crate::ui::components::dialog::StatusContent;
use crate::ui::models::device::{DeviceEvent, DeviceRepo, FirmwareType, backup};
use gpui::*;
use gpui_component::WindowExt;
use gpui_component::button::{ButtonVariant, ButtonVariants};
use gpui_component::input::InputState;

pub struct BackupViewModel {
    pub(super) device: Entity<DeviceRepo>,
    pub(super) status: Option<backup::BackupStatus>,
    /// The last exported mnemonic, held for on-screen display until cleared.
    pub(super) exported: Option<String>,
    pub(super) loading: bool,
    _task: Option<Task<()>>,
}

impl BackupViewModel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let device = models.device.clone();
        cx.subscribe(&device, |this: &mut Self, _, _: &DeviceEvent, cx| {
            if this.device.read(cx).device_changed {
                this.status = None;
                this.exported = None;
            }
            this.load(cx);
            cx.notify();
        })
        .detach();
        let mut this = Self {
            device,
            status: None,
            exported: None,
            loading: false,
            _task: None,
        };
        this.load(cx);
        this
    }

    pub(super) fn gate(&self, cx: &App) -> AppletGate {
        let repo = self.device.read(cx);
        match &repo.status {
            None => AppletGate::Unsupported,
            Some(s) if s.firmware_type == FirmwareType::PicoAll => {
                AppletGate::ClientUnsupported(crate::i18n::tr("Backup and recovery"))
            }
            Some(s) if s.firmware_type != FirmwareType::RSKey => AppletGate::Unsupported,
            Some(_) => AppletGate::Ready,
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading || self.gate(cx) != AppletGate::Ready {
            return;
        }
        self.loading = true;
        cx.notify();
        let weak = cx.entity().downgrade();
        self._task = Some(cx.spawn(async move |_, cx| {
            let res = cx
                .background_executor()
                .spawn(async { DeviceRepo::backup_status_blocking() })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.loading = false;
                if let Ok(s) = res {
                    this.status = Some(s);
                }
                cx.notify();
            });
        }));
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.load(cx);
    }

    pub(super) fn clear_exported(&mut self, cx: &mut Context<Self>) {
        self.exported = None;
        cx.notify();
    }

    fn pin_input(window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .localized_placeholder("Enter your FIDO PIN, if set", cx)
        })
    }

    // ── Export ──────────────────────────────────────────────────────────────

    pub(super) fn open_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pin = Self::pin_input(window, cx);
        let view = cx.entity().downgrade();
        let submit = {
            let pin = pin.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let p = pin.read(cx).text().to_string();
                let p = (!p.is_empty()).then_some(p);
                window.close_dialog(cx);
                let status =
                    dialog::open_status_dialog(crate::i18n::tr("Exporting Seed"), window, cx);
                let _ = view.update(cx, |this, cx| this.run_export(p, status, cx));
            })
        };
        Self::gated_dialog(
            crate::i18n::tr("Export FIDO Seed"),
            crate::i18n::tr(
                "Show the 24-word recovery phrase. Anyone with this phrase can copy your FIDO identity. Keep it private.",
            ),
            pin,
            None,
            (crate::i18n::tr("Export"), ButtonVariant::Danger),
            submit,
            window,
            cx,
        );
    }

    fn run_export(
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
                .spawn(async move { DeviceRepo::backup_export_blocking(pin) })
                .await;
            let _ = weak.update(cx, |this, cx| {
                this.loading = false;
                match res {
                    Ok(mnemonic) => {
                        this.exported = Some(mnemonic);
                        this.load(cx);
                        let _ = status.update(cx, |d, cx| {
                            d.set_success(
                                crate::i18n::tr("Seed exported — write down the phrase shown below, then seal the window.").into(),
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

    // ── Finalize (seal) ─────────────────────────────────────────────────────

    pub(super) fn open_finalize(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity().downgrade();
        dialog::open_confirm(
            crate::i18n::tr("Disable recovery exports"),
            crate::i18n::tr("Disable recovery phrase exports until the next FIDO reset? Save the phrase before continuing.").to_string(),
            crate::i18n::tr("Seal"),
            ButtonVariant::Primary,
            window,
            cx,
            move |_dh, window, cx| {
                window.close_dialog(cx);
                let status = dialog::open_status_dialog(crate::i18n::tr("Disabling recovery exports"), window, cx);
                let _ = view.update(cx, |this, cx| {
                    this.run_unit(DeviceRepo::backup_finalize_blocking, crate::i18n::tr("Recovery exports disabled."), status, cx);
                });
            },
        );
    }

    // ── Restore ─────────────────────────────────────────────────────────────

    pub(super) fn open_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pin = Self::pin_input(window, cx);
        let phrase = cx.new(|cx| {
            InputState::new(window, cx).localized_placeholder("24 words separated by spaces", cx)
        });
        let view = cx.entity().downgrade();
        let submit = {
            let pin = pin.clone();
            let phrase = phrase.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let m = phrase.read(cx).text().to_string();
                if m.trim().is_empty() {
                    return;
                }
                let p = pin.read(cx).text().to_string();
                let p = (!p.is_empty()).then_some(p);
                window.close_dialog(cx);
                let status =
                    dialog::open_status_dialog(crate::i18n::tr("Restoring Seed"), window, cx);
                let _ = view.update(cx, |this, cx| {
                    this.run_unit(
                        move || DeviceRepo::backup_restore_blocking(p, m),
                        crate::i18n::tr(
                            "Seed restored — the FIDO identity now matches the backup.",
                        ),
                        status,
                        cx,
                    );
                });
            })
        };
        Self::gated_dialog(
            crate::i18n::tr("Restore FIDO Seed"),
            crate::i18n::tr("Replace this device’s FIDO identity using a recovery phrase."),
            pin,
            Some((crate::i18n::tr("Recovery phrase"), phrase)),
            (crate::i18n::tr("Restore"), ButtonVariant::Danger),
            submit,
            window,
            cx,
        );
    }

    /// Run a blocking op returning `()`, reporting on `status` and reloading.
    fn run_unit(
        &mut self,
        op: impl FnOnce() -> Result<(), String> + Send + 'static,
        ok_msg: &'static str,
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
            let res = cx.background_executor().spawn(async move { op() }).await;
            let _ = weak.update(cx, |this, cx| {
                this.loading = false;
                match res {
                    Ok(_) => {
                        let _ = status.update(cx, |d, cx| d.set_success(ok_msg.into(), cx));
                        this.load(cx);
                    }
                    Err(e) => {
                        let _ = status.update(cx, |d, cx| d.set_error(e, cx));
                    }
                }
                cx.notify();
            });
        }));
    }

    /// A dialog with a warning body, an optional-PIN field, an optional second
    /// text field, and a coloured submit button.
    #[allow(clippy::too_many_arguments)]
    fn gated_dialog(
        title: &'static str,
        body: &'static str,
        pin: Entity<InputState>,
        extra: Option<(&'static str, Entity<InputState>)>,
        action: (&'static str, ButtonVariant),
        submit: DialogSubmit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.open_dialog(cx, move |dialog, _w, _| {
            let pin = pin.clone();
            let extra = extra.clone();
            let ok = submit.clone();
            let btn = submit.clone();
            let (action_label, action_variant) = action;
            let mut fields = gpui_component::v_flex().gap_3().pb_2();
            if let Some((label, input)) = &extra {
                fields = fields
                    .child(crate::i18n::text(label))
                    .child(gpui_component::input::Input::new(input));
            }
            fields = fields
                .child("FIDO PIN")
                .child(gpui_component::input::Input::new(&pin));
            dialog
                .title(crate::i18n::text(title))
                .child(body)
                .child(fields)
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
                        gpui_component::button::Button::new("go")
                            .with_variant(action_variant)
                            .label(action_label)
                            .on_click(move |_, window, cx| s(window, cx)),
                    ]
                })
        });
    }
}
