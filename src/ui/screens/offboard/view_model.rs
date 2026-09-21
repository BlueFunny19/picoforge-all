//! Firmware management UI; the old full-device Offboard action is no longer exposed.
use crate::hal::firmware::{self, Request};
use crate::ui::app::AppModels;
use crate::ui::models::device::DeviceRepo;
use gpui::*;
use gpui_component::{
    WindowExt,
    button::{Button, ButtonVariants},
    input::{Input, InputState},
    v_flex,
};
use std::path::PathBuf;

pub struct OffboardViewModel {
    pub(super) device: Entity<DeviceRepo>,
    pub(super) inputs: Vec<Entity<InputState>>,
    pub(super) loading: bool,
    pub(super) log: String,
    pub(super) error: Option<String>,
    pub(super) pending: Option<Request>,
    pub(super) boot_tested: bool,
    task: Option<Task<()>>,
}
pub enum OffboardEvent {
    Notification(String),
}
impl EventEmitter<OffboardEvent> for OffboardViewModel {}
pub(super) const FIELDS: [&str; 7] = [
    "Python",
    "picotool",
    "Device serial",
    "Firmware UF2",
    "Signing key PEM",
    "Output file",
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
            std::env::var("PICOFORGE_PYTHON").unwrap_or_else(|_| "python".into()),
            std::env::var("PICOTOOL").unwrap_or_default(),
            serial,
            String::new(),
            String::new(),
            String::new(),
            "0".into(),
        ];
        let inputs = values
            .into_iter()
            .enumerate()
            .map(|(i, value)| {
                cx.new(|cx| {
                    let mut input = InputState::new(window, cx).placeholder(if i == 1 {
                        "PICOTOOL / PATH, or choose executable"
                    } else {
                        FIELDS[i]
                    });
                    input.set_value(value, window, cx);
                    input
                })
            })
            .collect();
        Self {
            device: models.device.clone(),
            inputs,
            loading: false,
            log: String::new(),
            error: None,
            pending: None,
            boot_tested: false,
            task: None,
        }
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
            picotool: text(1),
            serial: text(2).to_uppercase(),
            firmware: text(3),
            key: text(4),
            output: text(5),
            slot: text(6).parse().unwrap_or(u8::MAX),
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
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.first() {
                    let _ = cx.update(|window, cx| {
                        field.update(cx, |input, cx| {
                            input.set_value(path.to_string_lossy().to_string(), window, cx)
                        })
                    });
                }
            }
        })
        .detach();
    }
    pub(super) fn select_output(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let home = directories::UserDirs::new()
            .map(|d| d.home_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let receiver = cx.prompt_for_new_path(
            &home,
            Some(if index == 4 {
                "firmware-signing.pem"
            } else {
                "firmware.signed.uf2"
            }),
        );
        let field = self.inputs[index].clone();
        cx.spawn_in(window, async move |_, cx| {
            if let Ok(Ok(Some(path))) = receiver.await {
                let _ = cx.update(|window, cx| {
                    field.update(cx, |input, cx| {
                        input.set_value(path.to_string_lossy().to_string(), window, cx)
                    })
                });
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
        let request = self.request(action, cx);
        if matches!(action, "flash" | "prepare") {
            self.confirm(request, window, cx);
        } else {
            self.run(request, cx);
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
                let _ = weak.update(cx, |this, cx| this.run(request, cx));
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
    fn run(&mut self, request: Request, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.pending = None;
        self.log = "Working… If the device flashes yellow, press and release its button. Keep it connected until the operation finishes.".into();
        let python = self.inputs[0].read(cx).text().to_string();
        let next = request.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { firmware::run(&python, request) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(result) => {
                        this.log = result.log;
                        if result.ok {
                            if let Some(review) = result.review {
                                let mut request = next;
                                request.review = Some(review);
                                this.pending = Some(request);
                                this.boot_tested = false;
                            } else if this.log.is_empty() {
                                this.log = "Operation completed.".into();
                            }
                        } else {
                            this.error = result.error;
                        }
                    }
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }
}
