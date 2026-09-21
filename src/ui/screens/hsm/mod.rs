//! SmartCard-HSM management screen.
use crate::hal::applets::hsm;
use crate::hal::types::FirmwareType;
use crate::ui::app::AppModels;
use crate::ui::components::{
    card::Card,
    dialog,
    form::{select_state, selected_key},
    page_view::PageView,
};
use crate::ui::models::device::{DeviceEvent, DeviceRepo};
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputState};
use gpui_component::select::Select;
use gpui_component::{ActiveTheme, Disableable, Icon, WindowExt, h_flex, v_flex};

#[derive(Clone, Copy)]
enum Action {
    Generate,
    Delete,
    Crypto,
    Read,
    Write,
    DeleteObject,
    Pin,
    SoPin,
    Unblock,
    Wrap,
    Unwrap,
    Dkek,
    Initialize,
}
impl Action {
    fn title(self) -> &'static str {
        match self {
            Self::Generate => "Generate key",
            Self::Delete => "Delete key",
            Self::Crypto => "Use key",
            Self::Read => "Read object",
            Self::Write => "Write object",
            Self::DeleteObject => "Delete object",
            Self::Pin => "Change user PIN",
            Self::SoPin => "Change SO PIN",
            Self::Unblock => "Unblock user PIN",
            Self::Wrap => "Export wrapped key",
            Self::Unwrap => "Import wrapped key",
            Self::Dkek => "DKEK shares",
            Self::Initialize => "Initialize HSM",
        }
    }
    fn fields(self) -> Vec<(&'static str, bool)> {
        match self {
            Self::Generate | Self::Delete | Self::Wrap => {
                vec![("User PIN", true), ("Key ID (hex, 01–FF)", false)]
            }
            Self::Crypto | Self::Unwrap => vec![
                ("User PIN", true),
                ("Key ID (hex, 01–FF)", false),
                ("Input bytes (hex)", true),
            ],
            Self::DeleteObject => vec![("User PIN", true), ("Object ID (hex)", false)],
            Self::Read => vec![
                ("User PIN (optional for public objects)", true),
                ("Object ID (hex, e.g. CE01)", false),
            ],
            Self::Write => vec![
                ("User PIN", true),
                ("Object ID (hex, e.g. CE01)", false),
                ("Object bytes (hex)", true),
            ],
            Self::Pin | Self::SoPin => vec![
                ("Current PIN", true),
                ("New PIN", true),
                ("Repeat new PIN", true),
            ],
            Self::Unblock => vec![
                ("SO PIN", true),
                ("New user PIN", true),
                ("Repeat new PIN", true),
            ],
            Self::Dkek => vec![
                ("User PIN (required for import)", true),
                ("DKEK share (64 hex digits; empty reads status)", true),
            ],
            Self::Initialize => vec![
                ("New user PIN", true),
                ("New SO PIN", true),
                ("DKEK shares (0–16)", false),
                ("Type ERASE HSM to confirm", false),
            ],
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Initialize => {
                "Deletes all HSM keys and objects and sets new PINs. Other applications and hardware locks are preserved."
            }
            Self::Delete => "Permanently deletes this HSM key. Check the key ID before continuing.",
            Self::Generate => {
                "Creates a key in an unused slot. Asymmetric keys return a public CVC certificate."
            }
            Self::DeleteObject => "Permanently deletes this certificate, metadata or data object.",
            Self::Write => "Replaces the selected certificate, metadata or data object.",
            Self::Wrap => {
                "Exports a DKEK-encrypted key backup; configured DKEK shares and physical confirmation may be required."
            }
            Self::Unwrap => "Restores a DKEK-encrypted key into an unused slot.",
            Self::Crypto => {
                "Uses an existing key. Input and output are bytes encoded as hex; the private key stays on the device."
            }
            Self::Dkek => {
                "Reads domain 0 status or imports one DKEK share configured during HSM initialization."
            }
            _ => "Uses the PIN and objects belonging to the SmartCard-HSM application.",
        }
    }
}

pub struct HsmViewModel {
    device: Entity<DeviceRepo>,
    info: Option<hsm::HsmInfo>,
    loading: bool,
    error: Option<String>,
    result: String,
    _task: Option<Task<()>>,
}
impl HsmViewModel {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let device = models.device.clone();
        cx.subscribe(&device, |this: &mut Self, _, _: &DeviceEvent, cx| {
            if this.device.read(cx).device_changed {
                this.info = None;
                this.result.clear();
            }
            this.load(cx);
        })
        .detach();
        let mut this = Self {
            device,
            info: None,
            loading: false,
            error: None,
            result: String::new(),
            _task: None,
        };
        this.load(cx);
        this
    }
    fn available(&self, cx: &App) -> bool {
        self.device
            .read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.firmware_type == FirmwareType::PicoAll)
            && self.device.read(cx).ccid_on()
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading || !self.available(cx) {
            return;
        }
        self.loading = true;
        self.error = None;
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async { hsm::read_info() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match res {
                    Ok(info) => this.info = Some(info),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
    }
    fn open_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let fields: Vec<_> = action
            .fields()
            .into_iter()
            .map(|(label, secret)| {
                let input = cx.new(|cx| InputState::new(window, cx).masked(secret));
                (label, input)
            })
            .collect();
        let options = match action {
            Action::Generate => hsm::KEY_ALGORITHMS,
            Action::Crypto => hsm::CRYPTO_OPERATIONS,
            _ => &[],
        };
        let choice = if options.is_empty() {
            None
        } else {
            Some(select_state(window, cx, options, 0))
        };
        let weak = cx.entity().downgrade();
        let submit = {
            let fields = fields.clone();
            let choice = choice.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let args: Vec<_> = fields
                    .iter()
                    .map(|(_, f)| f.read(cx).text().to_string())
                    .collect();
                let selected = choice
                    .as_ref()
                    .map(|s| selected_key(s, options, cx))
                    .unwrap_or(0);
                window.close_dialog(cx);
                let status = dialog::open_status_dialog(action.title(), window, cx);
                let _ = weak.update(cx, |this, cx| this.run(action, args, selected, status, cx));
            })
        };
        window.open_dialog(cx, move |d, _, _| {
            let mut form = v_flex().gap_2().child(action.description());
            if let Some(choice) = &choice {
                form = form.child("Algorithm").child(Select::new(choice).w_full());
            }
            for (label, input) in &fields {
                form = form.child(*label).child(Input::new(input));
            }
            let ok = submit.clone();
            let button = submit.clone();
            d.title(action.title())
                .child(form)
                .on_ok(move |_, w, cx| {
                    ok(w, cx);
                    false
                })
                .footer(move |_, _, _, _| {
                    let submit = button.clone();
                    vec![
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, w, cx| w.close_dialog(cx)),
                        Button::new("apply")
                            .primary()
                            .label(action.title())
                            .on_click(move |_, w, cx| submit(w, cx)),
                    ]
                })
        });
    }
    fn run(
        &mut self,
        action: Action,
        args: Vec<String>,
        choice: u8,
        status: WeakEntity<dialog::StatusContent>,
        cx: &mut Context<Self>,
    ) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.result.clear();
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { execute(action, &args, choice) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(bytes) => {
                        this.result = hex::encode_upper(bytes);
                        let msg = if this.result.is_empty() {
                            "Operation completed"
                        } else {
                            "Operation completed; result is available on the HSM page"
                        };
                        let _ = status.update(cx, |s, cx| s.set_success(msg.into(), cx));
                        this.load(cx);
                    }
                    Err(e) => {
                        let _ = status.update(cx, |s, cx| s.set_error(e, cx));
                    }
                }
                cx.notify();
            });
        }));
    }
}
fn execute(action: Action, args: &[String], choice: u8) -> Result<Vec<u8>, String> {
    let err = |e: crate::error::PFError| e.to_string();
    let bytes = |s: &str| {
        hex::decode(s.chars().filter(|c| !c.is_whitespace()).collect::<String>())
            .map_err(|_| "Invalid hex input".to_string())
    };
    let id = || {
        u8::from_str_radix(args[1].trim(), 16)
            .map_err(|_| "Enter a key ID from 01 to FF".to_string())
    };
    let fid = || {
        u16::from_str_radix(args[1].trim(), 16)
            .map_err(|_| "Enter a four-digit hex object ID".to_string())
    };
    let pin = args[0].as_bytes();
    match action {
        Action::Generate => return hsm::generate(pin, id()?, choice).map_err(err),
        Action::Delete => hsm::delete_key(pin, id()?).map_err(err)?,
        Action::Crypto => return hsm::crypto(pin, id()?, choice, &bytes(&args[2])?).map_err(err),
        Action::Read => return hsm::read_object(pin, fid()?).map_err(err),
        Action::DeleteObject => hsm::delete_object(pin, fid()?).map_err(err)?,
        Action::Write => hsm::write_object(pin, fid()?, &bytes(&args[2])?).map_err(err)?,
        Action::Pin | Action::SoPin | Action::Unblock => {
            if args[1] != args[2] {
                return Err("New PIN entries do not match".into());
            }
            if matches!(action, Action::Unblock) {
                hsm::unblock_pin(pin, args[1].as_bytes()).map_err(err)?;
            } else {
                hsm::change_pin(pin, args[1].as_bytes(), matches!(action, Action::SoPin))
                    .map_err(err)?;
            }
        }
        Action::Wrap => return hsm::wrap_key(pin, id()?).map_err(err),
        Action::Unwrap => hsm::unwrap_key(pin, id()?, &bytes(&args[2])?).map_err(err)?,
        Action::Dkek => return hsm::dkek_share(pin, &bytes(&args[1])?).map_err(err),
        Action::Initialize => {
            if args[3] != "ERASE HSM" {
                return Err("Type ERASE HSM to confirm initialization".into());
            }
            let shares = args[2]
                .trim()
                .parse()
                .map_err(|_| "Enter a DKEK share count from 0 to 16")?;
            hsm::initialize(pin, args[1].as_bytes(), shares).map_err(err)?;
        }
    }
    Ok(Vec::new())
}
impl HsmViewModel {
    fn action_row(
        &self,
        title: &'static str,
        description: &'static str,
        actions: &[Action],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut buttons = h_flex().gap_2().flex_wrap().min_w_0();
        for &action in actions {
            let button = Button::new(SharedString::from(action.title()))
                .label(action.title())
                .outline()
                .disabled(self.loading || self.info.is_none())
                .on_click(cx.listener(move |this, _, w, cx| this.open_action(action, w, cx)));
            buttons = buttons.child(if matches!(action, Action::Initialize) {
                button.danger()
            } else {
                button
            });
        }
        v_flex()
            .w_full()
            .min_w_0()
            .gap_3()
            .p_4()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .child(
                v_flex().gap_1().child(title).child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(description),
                ),
            )
            .child(buttons)
            .into_any_element()
    }
}
impl Render for HsmViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut body = v_flex().w_full().min_w_0().gap_6();
        if !self.available(cx) {
            body =
                body.child(Card::new().title("HSM unavailable").description(
                    "Connect a Pico All device with its smart-card interface enabled.",
                ));
        } else {
            let mut details = div().grid().grid_cols(2).gap_6();
            if let Some(info) = &self.info {
                for (label, value) in [
                    ("Firmware", info.version.clone()),
                    ("Free memory", format!("{} bytes", info.free_memory)),
                    ("User PIN", info.pin.to_string()),
                    ("Security officer PIN", info.so_pin.to_string()),
                ] {
                    details = details.child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(label),
                            )
                            .child(value),
                    );
                }
            } else {
                details = details.child(if self.loading {
                    "Reading card information…"
                } else {
                    "Card information is unavailable"
                });
            }
            body = body.child(
                Card::new()
                    .title("Card information")
                    .description("SmartCard-HSM status")
                    .icon(Icon::default().path("icons/microchip.svg"))
                    .header_right(
                        Button::new("hsm-refresh")
                            .outline()
                            .icon(Icon::default().path("icons/refresh-cw.svg"))
                            .label("Refresh")
                            .disabled(self.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .child(details),
            );
            if let Some(e) = &self.error {
                body = body.child(div().text_color(cx.theme().danger).child(e.clone()));
            }
            let mut objects = v_flex().gap_2();
            if let Some(info) = &self.info {
                if info.files.is_empty() {
                    objects = objects.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("No stored objects"),
                    );
                }
                for &id in &info.files {
                    let kind = match id >> 8 {
                        0xCC => "Private / secret key",
                        0xC4 => "Public certificate",
                        0xCE => "Key description",
                        0xCA => "Data object",
                        0xC8 => "Certificate",
                        0xC9 => "Certificate description",
                        0xCF => "Data description",
                        0xCD => "Protected data",
                        _ => "Object",
                    };
                    objects = objects.child(
                        h_flex()
                            .justify_between()
                            .gap_4()
                            .p_4()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_lg()
                            .child(format!("{id:04X}"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if id & 0xff == 0 {
                                        format!("{kind} · device identity")
                                    } else {
                                        kind.to_string()
                                    }),
                            ),
                    );
                }
            }
            body = body
                .child(
                    Card::new()
                        .title("Keys & objects")
                        .description("Keys, certificates and data stored on this HSM")
                        .icon(Icon::default().path("icons/key.svg"))
                        .child(objects)
                        .child(self.action_row(
                            "Key management",
                            "Generate a key in an unused slot, use it, or remove it.",
                            &[Action::Generate, Action::Crypto, Action::Delete],
                            cx,
                        ))
                        .child(self.action_row(
                            "Certificates & data",
                            "Read, replace or delete an object by its hexadecimal ID.",
                            &[Action::Read, Action::Write, Action::DeleteObject],
                            cx,
                        )),
                )
                .child(
                    Card::new()
                        .title("PIN & recovery")
                        .description("Manage the HSM user and security officer PINs")
                        .icon(Icon::default().path("icons/lock.svg"))
                        .child(self.action_row(
                            "User PIN",
                            "Authorizes private key operations and protected objects.",
                            &[Action::Pin],
                            cx,
                        ))
                        .child(self.action_row(
                            "Security officer PIN",
                            "Authorizes user PIN recovery.",
                            &[Action::SoPin, Action::Unblock],
                            cx,
                        )),
                )
                .child(
                    Card::new()
                        .title("Key backup")
                        .description("Protect and restore keys using DKEK shares")
                        .icon(Icon::default().path("icons/save.svg"))
                        .child(self.action_row(
                            "Wrapped keys",
                            "Export an encrypted key or restore it into an unused slot.",
                            &[Action::Wrap, Action::Unwrap],
                            cx,
                        ))
                        .child(self.action_row(
                            "DKEK shares",
                            "Read the wrapping domain status or import a share.",
                            &[Action::Dkek],
                            cx,
                        )),
                )
                .child(
                    Card::new()
                        .title("Initialize HSM")
                        .description("Erase HSM contents and configure new PINs")
                        .icon(Icon::default().path("icons/trash-2.svg"))
                        .child(self.action_row(
                            "Initialize or reset",
                            "Deletes all HSM keys and objects. This cannot be undone.",
                            &[Action::Initialize],
                            cx,
                        )),
                );
            if !self.result.is_empty() {
                body =
                    body.child(
                        Card::new()
                            .title("Operation result")
                            .description("Hexadecimal output")
                            .header_right(
                                Button::new("copy-hsm-result")
                                    .outline()
                                    .label("Copy")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            this.result.clone(),
                                        ))
                                    })),
                            )
                            .child(v_flex().gap_1().children(
                                self.result.as_bytes().chunks(64).map(|line| {
                                    div()
                                        .text_sm()
                                        .font_family("monospace")
                                        .child(String::from_utf8_lossy(line).into_owned())
                                }),
                            )),
                    );
            }
        }
        PageView::build(
            "HSM",
            "Manage SmartCard-HSM keys, certificates and PINs.",
            body,
            cx.theme(),
        )
    }
}
