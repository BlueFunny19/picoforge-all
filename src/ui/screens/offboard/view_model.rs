//! Firmware management UI; the old full-device Offboard action is no longer exposed.
use crate::hal::firmware::{self, Request};
use crate::ui::app::AppModels;
use crate::ui::models::device::DeviceRepo;
use gpui::*;
use gpui_component::{
    WindowExt,
    button::{Button, ButtonVariants},
    input::{Input, InputEvent, InputState},
    v_flex,
};

pub struct OffboardViewModel {
    pub(super) device: Entity<DeviceRepo>,
    pub(super) inputs: Vec<Entity<InputState>>,
    pub(super) loading: bool,
    pub(super) log: super::console::Console,
    pub(super) log_scroll: ScrollHandle,
    pub(super) error: Option<String>,
    pub(super) pending: Option<Request>,
    pub(super) boot_tested: bool,
    task: Option<Task<()>>,
    pub(super) read_status: Option<crate::hal::types::FullDeviceStatus>,
    pub(super) read_attempted: bool,
    pub(super) image: Option<firmware::ImageInfo>,
    pub(super) assessment: Option<firmware::Assessment>,
    pub(super) inspected_path: String,
}
pub enum OffboardEvent {
    Notification(String),
}
impl EventEmitter<OffboardEvent> for OffboardViewModel {}
pub(super) const FIELDS: [&str; 5] = [
    "picotool",
    "Device serial",
    "Firmware UF2",
    "Signing key PEM",
    "Boot key slot (0–3)",
];
impl OffboardViewModel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let serial = models
            .device
            .read(cx)
            .status
            .as_ref()
            .map(|s| s.info.serial.clone())
            .unwrap_or_default();
        let values = [
            std::env::var("PICOTOOL").unwrap_or_default(),
            serial,
            String::new(),
            String::new(),
            "0".into(),
        ];
        let inputs: Vec<Entity<InputState>> = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx).placeholder(if i == 0 {
                        "PICOTOOL / PATH, or choose executable"
                    } else {
                        FIELDS[i]
                    });
                    input.set_value(value, window, cx);
                    input
                })
            })
            .collect();
        for index in [1, 2] {
            cx.subscribe(&inputs[index], |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.image = None;
                    this.assessment = None;
                    this.inspected_path.clear();
                    cx.notify();
                }
            })
            .detach();
        }
        cx.subscribe_in(
            &models.device,
            window,
            |this, _, _: &crate::ui::models::device::DeviceEvent, w, cx| {
                if let Some(serial) = this
                    .device
                    .read(cx)
                    .status
                    .as_ref()
                    .map(|s| s.info.serial.clone())
                {
                    if this.inputs[1].read(cx).text().to_string().is_empty() {
                        this.inputs[1].update(cx, |i, cx| i.set_value(serial, w, cx));
                    }
                    this.read_status = None;
                    this.read_attempted = false;
                    cx.notify();
                }
            },
        )
        .detach();
        Self {
            read_status: None,
            read_attempted: false,
            image: None,
            assessment: None,
            inspected_path: String::new(),
            device: models.device.clone(),
            inputs,
            loading: false,
            log: super::console::Console::default(),
            log_scroll: ScrollHandle::new(),
            error: None,
            pending: None,
            boot_tested: false,
            task: None,
        }
    }
    fn read_device(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let serial = self.inputs[1]
            .read(cx)
            .text()
            .to_string()
            .trim()
            .to_uppercase();
        if !firmware::serial_valid(&serial) {
            self.log.push("WARN", "Enter a 16-digit device serial.");
            cx.notify();
            return;
        }
        self.loading = true;
        self.read_status = None;
        self.read_attempted = true;
        self.log.push(
            "INFO",
            format!("Reading device information\nRequested serial: {serial}"),
        );
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let state =
                        DeviceRepo::read_device_state_blocking().map_err(|e| e.to_string())?;
                    if !state.status.info.serial.eq_ignore_ascii_case(&serial) {
                        return Err(format!("Device {serial} is not connected in normal mode."));
                    }
                    Ok(state)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(state) => {
                        this.log.push("SUCCESS", format!("Device information read\nSerial: {}\nFirmware: {}\nVersion: {}\nTransport: {:?}",
                            state.status.info.serial, state.status.firmware_type,
                            state.status.info.firmware_version, state.status.method));
                        let details = serde_json::json!({
                            "device": state.status,
                            "statusLight": state.led_status,
                            "usbApplications": state.management_apps,
                        });
                        this.log.push("DATA", format!("Device response (decoded)\n{}",
                            serde_json::to_string_pretty(&details).unwrap_or_else(|_| details.to_string())));
                        let status = state.status.clone();
                        this.device
                            .update(cx, |device, cx| device.apply_fresh_state(state, cx));
                        this.read_status = Some(status);
                    }
                    Err(error) => this.log.push("ERROR", error),
                }
                this.log_scroll.scroll_to_bottom();
                cx.notify();
            });
        }));
        cx.notify();
    }
    fn request(&self, action: &str, cx: &App) -> Request {
        let text = |i: usize| {
            self.inputs[i]
                .read(cx)
                .text()
                .to_string()
                .trim()
                .to_string()
        };
        Request {
            action: action.into(),
            picotool: text(0),
            serial: text(1).to_uppercase(),
            firmware: text(2),
            key: text(3),
            output: String::new(),
            slot: text(4).parse().unwrap_or(u8::MAX),
            ..Default::default()
        }
    }
    pub(super) fn select_file(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select".into()),
        });
        let field = self.inputs[index].clone();
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.first() {
                    let _ = cx.update(|window, cx| {
                        field.update(cx, |input, cx| {
                            input.set_value(path.to_string_lossy().to_string(), window, cx)
                        });
                        if index == 2 {
                            let _ = this.update(cx, |this, cx| this.start("image", window, cx));
                        }
                    });
                }
            }
        })
        .detach();
    }
    pub(crate) fn start(
        &mut self,
        action: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        if action == "info" {
            self.read_device(window, cx);
            return;
        }
        let mut request = self.request(action, cx);
        if action == "flash" {
            if let Some(assessment) = &self.assessment {
                if self.inspected_path == request.firmware && assessment.serial == request.serial {
                    if !assessment.allowed {
                        return;
                    }
                    request.review = Some(serde_json::to_value(assessment).unwrap());
                    self.confirm_flash(request, window, cx);
                    return;
                }
            }
            request.action = "check".into();
            self.run(request, window, cx);
        } else if action == "prepare" {
            self.confirm(request, window, cx);
        } else {
            self.run(request, window, cx);
        }
    }
    fn confirm_flash(&mut self, mut request: Request, window: &mut Window, cx: &mut Context<Self>) {
        if self.assessment.as_ref().is_some_and(|a| a.mismatch) && !request.mismatch_accepted {
            let weak = cx.entity().downgrade();
            request.mismatch_accepted = true;
            window.open_dialog(cx, move |dialog, _, _| {
                let request = request.clone();
                let weak = weak.clone();
                dialog.title("Different firmware signing key")
                    .child("This firmware uses a different signing key. Secure Boot is off, so installation is possible, but the image may be untrusted or incompatible. Continue only if you trust its source.")
                    .footer(move |_, _, _, _| {
                        let request = request.clone(); let weak = weak.clone();
                        vec![
                            Button::new("cancel-mismatch").label("Cancel").on_click(|_, w, cx| w.close_dialog(cx)),
                            Button::new("accept-mismatch").danger().label("Continue to confirmation").on_click(move |_, w, cx| {
                                w.close_dialog(cx);
                                let _ = weak.update(cx, |this, cx| this.confirm(request.clone(), w, cx));
                            })
                        ]
                    })
            });
        } else {
            self.confirm(request, window, cx);
        }
    }
    pub(super) fn confirm_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(mut request) = self.pending.clone() {
            request.boot_tested = self.boot_tested;
            self.confirm(request, window, cx);
        }
    }
    fn confirm(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        let verb = if request.action == "prepare" {
            "ERASE".into()
        } else {
            request.action.to_uppercase()
        };
        let phrase = format!("{verb} {}", request.serial);
        let warning = match request.action.as_str() {
            "flash" if self.assessment.as_ref().is_some_and(|a| a.image.nuke) => {
                "This runs Nuke and permanently erases all external Flash, including firmware, keys, PINs and settings. Hardware security locks remain. Afterward, install firmware signed with the board's trusted key."
            }
            "flash" => {
                "This writes the selected UF2, verifies the readback and restarts this board. Keep it connected until completion."
            }
            "prepare" => {
                "This erases all application credentials, PINs and settings. Firmware and permanent hardware locks are retained."
            }
            _ => {
                "This permanently programs the reviewed security fuses. It cannot be undone. Keep the trusted signing key backed up."
            }
        };
        let target = format!("Device: {}\nFirmware: {}", request.serial, request.firmware);

        let input = cx.new(|cx| InputState::new(window, cx).placeholder(phrase.clone()));
        let weak = cx.entity().downgrade();
        let submit = std::rc::Rc::new({
            let input = input.clone();
            let phrase = phrase.clone();
            move |w: &mut Window, cx: &mut App| {
                let value = input.read(cx).text().to_string();
                if value != phrase {
                    let _ = weak.update(cx, |_, cx| {
                        cx.emit(OffboardEvent::Notification(
                            "Enter the exact confirmation phrase".into(),
                        ))
                    });
                    return;
                }
                let mut request = request.clone();
                request.phrase = value;
                w.close_dialog(cx);
                let _ = weak.update(cx, |this, cx| this.run(request, w, cx));
            }
        });
        window.open_dialog(cx, move |dialog, _, _| {
            let submit = submit.clone();
            let ok = submit.clone();
            dialog
                .title("Confirm device operation")
                .child(
                    v_flex()
                        .gap_3()
                        .child(warning)
                        .child(target.clone())
                        .child(format!("Type {phrase} to continue"))
                        .child(Input::new(&input)),
                )
                .on_ok(move |_, w, cx| {
                    ok(w, cx);
                    false
                })
                .footer(move |_, _, _, _| {
                    let submit = submit.clone();
                    vec![
                        Button::new("cancel-firmware")
                            .label("Cancel")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                        Button::new("confirm-firmware")
                            .danger()
                            .label("Confirm")
                            .on_click(move |_, w, cx| submit(w, cx)),
                    ]
                })
        });
    }
    fn run(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.pending = None;
        let (log_tx, log_rx) = std::sync::mpsc::channel();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let next = request.clone();
        std::thread::spawn(move || {
            let _ = result_tx.send(firmware::run(request, log_tx));
        });
        self.task = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                let mut lines: Vec<_> = log_rx.try_iter().collect();
                let result = result_rx.try_recv();
                lines.extend(log_rx.try_iter());
                let finished = !matches!(result, Err(std::sync::mpsc::TryRecvError::Empty));
                let _ = cx.update(|window, cx| {
                    let _ = this.update(cx, |this, cx| {
                        let has_output = !lines.is_empty() || finished;
                        for line in lines {
                            this.log.worker(line);
                        }
                        match result {
                            Ok(Ok(result)) => {
                                this.loading = false;
                                if let Some(path) = result.output {
                                    this.inputs[2]
                                        .update(cx, |i, cx| i.set_value(path.clone(), window, cx));
                                    this.inspected_path = path;
                                } else {
                                    this.inspected_path = next.firmware.clone();
                                }
                                if result.image.is_some() {
                                    this.image = result.image;
                                }
                                if let Some(a) = result.assessment {
                                    this.assessment = Some(a.clone());
                                    if next.action == "check" && a.allowed {
                                        let mut req = next.clone();
                                        req.action = "flash".into();
                                        req.review = Some(serde_json::to_value(a).unwrap());
                                        this.confirm_flash(req, window, cx);
                                    }
                                }
                                if let Some(review) = result.review {
                                    let mut req = next.clone();
                                    req.review = Some(review);
                                    this.pending = Some(req);
                                    this.boot_tested = false;
                                }
                            }
                            Ok(Err(error)) => {
                                this.loading = false;
                                this.log.push("ERROR", error.clone());
                                this.error = Some(error);
                            }
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                this.loading = false;
                                this.log
                                    .push("ERROR", "Firmware worker stopped unexpectedly.");
                            }
                            _ => {}
                        }
                        if has_output {
                            this.log_scroll.scroll_to_bottom();
                        }
                        cx.notify();
                    });
                });
                if finished {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(60))
                    .await;
            }
        }));
        cx.notify();
    }
}
