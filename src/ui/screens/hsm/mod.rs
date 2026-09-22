//! SmartCard-HSM management screen.
mod object_editor;
use crate::hal::applets::hsm;
use crate::hal::types::FirmwareType;
use crate::ui::app::AppModels;
use crate::ui::components::{
    applet_gate::{AppletGate, empty_state},
    button::standard,
    card::Card,
    dialog,
    form::{FormErrors, info_card, select_state, selected_key},
    information,
    page_view::PageView,
};
use crate::ui::models::device::{DeviceEvent, DeviceRepo};
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::select::Select;
use gpui_component::{ActiveTheme, Disableable, Icon, WindowExt, h_flex, v_flex};

#[derive(Clone, Copy)]
enum Action {
    Setup,
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
            Self::Setup => "Set up HSM",
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
            Self::Initialize => "Reset HSM",
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
            Self::Setup => vec![
                ("New user PIN (6–16 characters)", true),
                ("New SO PIN (6–16 characters)", true),
                ("DKEK shares (0 disables key backup)", false),
            ],
            Self::Initialize => Vec::new(),
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Setup => {
                "Choose a user PIN for everyday key operations and a separate security officer PIN for PIN recovery. Leave DKEK shares at 0 unless you need encrypted key backups. Keep both PINs safely."
            }
            Self::Initialize => {
                "Deletes all HSM keys and objects and sets new PINs. Other applications and hardware locks are preserved."
            }
            Self::Delete => "Permanently deletes this HSM key. Check the key ID before continuing.",
            Self::Generate => {
                "Choose an algorithm and enter your HSM user PIN. PicoForge assigns a free key ID automatically. The private key stays on the device."
            }
            Self::DeleteObject => "Permanently deletes this certificate, metadata or data object.",
            Self::Write => "Replaces the selected certificate, metadata or data object.",
            Self::Wrap => {
                "Exports a DKEK-encrypted key backup; configured DKEK shares and physical confirmation may be required."
            }
            Self::Unwrap => {
                "Enter your HSM user PIN and the wrapped backup bytes. PicoForge assigns a free key ID. Import the matching DKEK shares first."
            }
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
    loaded: bool,
    error: Option<String>,
    result: String,
    key_search: Entity<InputState>,
    object_search: Entity<InputState>,
    _task: Option<Task<()>>,
}
impl HsmViewModel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let key_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search keys"));
        let object_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search objects"));
        for input in [&key_search, &object_search] {
            cx.subscribe(input, |_, _, _: &InputEvent, cx| cx.notify())
                .detach();
        }
        let device = models.device.clone();
        cx.subscribe(&device, |this: &mut Self, _, _: &DeviceEvent, cx| {
            if this.device.read(cx).device_changed {
                this.info = None;
                this.loaded = false;
                this.result.clear();
            }
            if !this.loaded {
                this.load(cx);
            }
        })
        .detach();
        let mut this = Self {
            device,
            info: None,
            loading: false,
            loaded: false,
            error: None,
            result: String::new(),
            key_search,
            object_search,
            _task: None,
        };
        this.load(cx);
        this
    }
    fn gate(&self, cx: &App) -> AppletGate {
        let device = self.device.read(cx);
        if !device
            .status
            .as_ref()
            .is_some_and(|s| s.firmware_type == FirmwareType::PicoAll)
        {
            AppletGate::Unsupported
        } else if !device.ccid_on() {
            AppletGate::CcidOff
        } else {
            AppletGate::Ready
        }
    }
    fn available(&self, cx: &App) -> bool {
        self.gate(cx) == AppletGate::Ready
    }
    fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading || !self.available(cx) {
            return;
        }
        self.loading = true;
        self.info = None;
        self.error = None;
        cx.notify();
        self._task = Some(cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async { hsm::read_info() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                this.loaded = true;
                match res {
                    Ok(info) => this.info = Some(info),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
    }
    fn open_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        self.open_action_for(action, None, window, cx);
    }
    fn open_action_for(
        &mut self,
        action: Action,
        id: Option<u16>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .info
            .as_ref()
            .is_some_and(|i| i.initialized == Some(false))
            && !matches!(action, Action::Setup | Action::Initialize | Action::Read)
        {
            self.open_action(Action::Setup, window, cx);
            return;
        }
        if matches!(action, Action::Write) {
            self.open_object_editor(id, window, cx);
            return;
        }
        if matches!(action, Action::Initialize) {
            self.open_reset(window, cx);
            return;
        }
        let fields: Vec<_> = action
            .fields()
            .into_iter()
            .map(|(label, secret)| {
                let input = cx.new(|cx| InputState::new(window, cx).masked(secret));
                (label, input)
            })
            .collect();
        if matches!(action, Action::Setup | Action::Initialize) {
            fields[2]
                .1
                .update(cx, |input, cx| input.set_value("0", window, cx));
        }
        if let Some(id) = id {
            fields[1].1.update(cx, |input, cx| {
                input.set_value(
                    if matches!(action, Action::Crypto | Action::Delete | Action::Wrap) {
                        format!("{:02X}", id & 0xff)
                    } else {
                        format!("{id:04X}")
                    },
                    window,
                    cx,
                )
            });
        }
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
        let errors = FormErrors::default();
        for (index, (_, input)) in fields.iter().enumerate() {
            errors.watch(index, input, window, cx);
        }
        let weak = cx.entity().downgrade();
        let submit = {
            let errors = errors.clone();
            let fields = fields.clone();
            let choice = choice.clone();
            std::rc::Rc::new(move |window: &mut Window, cx: &mut App| {
                let args: Vec<_> = fields
                    .iter()
                    .map(|(_, f)| f.read(cx).text().to_string())
                    .collect();
                errors.clear();
                for (index, (label, _)) in fields.iter().enumerate() {
                    let optional = (index == 1
                        && matches!(action, Action::Generate | Action::Unwrap))
                        || (index == 0 && matches!(action, Action::Read | Action::Dkek))
                        || (index == 1 && matches!(action, Action::Dkek));
                    if !optional {
                        errors.required(index, label, &args[index]);
                    }
                }
                if matches!(action, Action::Pin | Action::SoPin | Action::Unblock)
                    && args[1] != args[2]
                {
                    errors.set(2, "New PIN entries do not match.");
                }
                if !errors.valid(window) {
                    return;
                }
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
            let mut form = v_flex().gap_3().child(action.description());
            if !matches!(action, Action::Setup | Action::Dkek) {
                form = form.child(info_card(if matches!(action, Action::SoPin | Action::Unblock) {
                    "PicoForge reset default: SO PIN 12345678. Use your own PIN if it was changed or chosen during setup."
                } else {
                    "PicoForge reset default: user PIN 123456. Use your own PIN if it was changed or chosen during setup."
                }));
            }
            if let Some(choice) = &choice {
                form = form.child("Algorithm").child(Select::new(choice).w_full());
            }
            for (index, (label, input)) in fields.iter().enumerate() {
                if index == 1 && matches!(action, Action::Generate | Action::Unwrap) {
                    continue;
                }
                if index == 1 && id.is_some() {
                    form = form.child(format!("Selected ID: {:02X}", id.unwrap()));
                } else {
                    let required = !((index == 0 && matches!(action, Action::Read | Action::Dkek)) || (index == 1 && matches!(action, Action::Dkek)));
                    form = form.child(errors.field(index, label, input, required));
                }
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
        Action::Generate => return hsm::generate_auto(pin, choice).map_err(err),
        Action::Delete => hsm::delete_key(pin, id()?).map_err(err)?,
        Action::Crypto => return hsm::crypto(pin, id()?, choice, &bytes(&args[2])?).map_err(err),
        Action::Read => return hsm::read_object(pin, fid()?).map_err(err),
        Action::DeleteObject => hsm::delete_object(pin, fid()?).map_err(err)?,
        Action::Write => {
            let data = bytes(&args[2])?;
            if args[1].is_empty() {
                hsm::write_object_auto(pin, choice, &data).map_err(err)?;
            } else {
                hsm::write_object(pin, fid()?, &data).map_err(err)?;
            }
        }
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
        Action::Unwrap => hsm::unwrap_auto(pin, &bytes(&args[2])?).map_err(err)?,
        Action::Dkek => return hsm::dkek_share(pin, &bytes(&args[1])?).map_err(err),
        Action::Setup => {
            let shares = args[2]
                .trim()
                .parse()
                .map_err(|_| "Enter a DKEK share count from 0 to 16")?;
            hsm::setup(pin, args[1].as_bytes(), shares).map_err(err)?;
        }
        Action::Initialize => hsm::reset_defaults().map_err(err)?,
    }
    Ok(Vec::new())
}
impl HsmViewModel {
    fn stored_list(&self, keys: bool, cx: &mut Context<Self>) -> Card {
        let ids: Vec<_> = self
            .info
            .as_ref()
            .map(|i| {
                i.files
                    .iter()
                    .copied()
                    .filter(|id| !matches!(*id, 0xC400 | 0xCC00) && ((*id >> 8 == 0xCC) == keys))
                    .collect()
            })
            .unwrap_or_default();
        let action = if keys {
            Action::Generate
        } else {
            Action::Write
        };
        let search = if keys {
            &self.key_search
        } else {
            &self.object_search
        };
        let query = search.read(cx).text().to_string().to_lowercase();
        let total = ids.len();
        let ids: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                format!(
                    "{} {id:04X} {:02X} {}",
                    if keys { "Key" } else { "Object" },
                    id & 0xff,
                    object_kind(*id)
                )
                .to_lowercase()
                .contains(&query)
            })
            .collect();
        let weak = cx.entity().downgrade();
        let rows = if ids.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if !query.is_empty() {
                    "No matches"
                } else if keys {
                    "No stored keys"
                } else {
                    "No stored objects"
                })
                .into_any_element()
        } else {
            uniform_list(
                if keys {
                    "hsm-key-list"
                } else {
                    "hsm-object-list"
                },
                ids.len(),
                move |range, _, cx| {
                    weak.update(cx, |this, cx| {
                        range.map(|i| this.stored_row(ids[i], keys, cx)).collect()
                    })
                    .unwrap_or_default()
                },
            )
            .h(px(264.))
            .w_full()
            .into_any_element()
        };
        Card::new()
            .title(if keys { "Keys" } else { "Objects" })
            .description(format!("{total} stored"))
            .icon(Icon::default().path(if keys {
                "icons/key.svg"
            } else {
                "icons/file.svg"
            }))
            .header_right(
                standard(
                    if keys {
                        "hsm-add-key"
                    } else {
                        "hsm-add-object"
                    },
                    cx,
                )
                .label(action.title())
                .disabled(self.loading || self.info.is_none())
                .on_click(cx.listener(move |this, _, w, cx| this.open_action(action, w, cx))),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(Input::new(search).cleanable(true))
                    .child(rows),
            )
    }
    fn stored_row(&self, id: u16, keys: bool, cx: &mut Context<Self>) -> AnyElement {
        let kind = object_kind(id);
        let mut actions = h_flex().gap_2().flex_shrink_0();
        for (action, icon) in if keys {
            vec![
                (Action::Crypto, "icons/key.svg"),
                (Action::Wrap, "icons/save.svg"),
                (Action::Delete, "icons/trash-2.svg"),
            ]
        } else {
            vec![
                (Action::Read, "icons/file.svg"),
                (Action::Write, "icons/replace.svg"),
                (Action::DeleteObject, "icons/trash-2.svg"),
            ]
        } {
            actions =
                actions.child(
                    standard(
                        SharedString::from(format!("hsm-{id}-{}", action.title())),
                        cx,
                    )
                    .icon(Icon::default().path(icon))
                    .tooltip(action.title())
                    .disabled(self.loading)
                    .on_click(cx.listener(move |this, _, w, cx| {
                        this.open_action_for(action, Some(id), w, cx)
                    })),
                );
        }
        let row =
            h_flex()
                .h(px(80.))
                .mb_2()
                .w_full()
                .justify_between()
                .gap_4()
                .p_4()
                .border_1()
                .border_color(cx.theme().border)
                .rounded_lg()
                .child(
                    h_flex()
                        .gap_3()
                        .child(div().p_2().rounded_lg().bg(rgb(0x252528)).child(
                            Icon::default().path(if keys {
                                "icons/key.svg"
                            } else {
                                "icons/file.svg"
                            }),
                        ))
                        .child(
                            v_flex()
                                .gap_1()
                                .child(if keys {
                                    format!("Key {:02X}", id & 0xff)
                                } else {
                                    format!("Object {id:04X}")
                                })
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(kind),
                                ),
                        ),
                )
                .child(actions);
        row.into_any_element()
    }
    fn action_row(
        &self,
        title: &'static str,
        description: &'static str,
        actions: &[Action],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut buttons = h_flex().gap_2().flex_wrap().min_w_0();
        for &action in actions {
            let button = standard(SharedString::from(action.title()), cx)
                .label(action.title())
                .disabled(self.loading || self.info.is_none())
                .on_click(cx.listener(move |this, _, w, cx| this.open_action(action, w, cx)));
            buttons = buttons.child(if matches!(action, Action::Initialize) {
                button.danger()
            } else {
                button
            });
        }
        h_flex()
            .w_full()
            .min_w_0()
            .justify_between()
            .items_center()
            .gap_4()
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
        if let Some((heading, message)) = self.gate(cx).message() {
            body = body.child(empty_state(heading, message, cx.theme()));
        } else {
            let mut details = if self.info.is_some() {
                information::grid()
            } else {
                div()
            };
            if let Some(info) = &self.info {
                for (label, value) in [
                    ("Firmware", info.version.clone()),
                    ("Free memory", format!("{} bytes", info.free_memory)),
                    ("User PIN tries", info.pin.to_string()),
                    ("Security officer PIN tries", info.so_pin.to_string()),
                    (
                        "Identity key description",
                        if info.files.contains(&0xC400) {
                            "C400"
                        } else {
                            "Not installed"
                        }
                        .into(),
                    ),
                    (
                        "Identity key",
                        if info.files.contains(&0xCC00) {
                            "CC00"
                        } else {
                            "Not installed"
                        }
                        .into(),
                    ),
                ] {
                    let field = information::field(label, value, cx.theme());
                    details = if label.ends_with("PIN tries") {
                        details.child(field.id(SharedString::from(label)).tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new(
                                "Remaining / total tries. (default) means the factory retry limit, not a default PIN. A dash means this firmware does not report the limit."
                            ).build(window, cx)
                        }))
                    } else {
                        details.child(field)
                    };
                }
            } else {
                details = details.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.loading {
                            "Reading card information…"
                        } else {
                            "Card information unavailable. Refresh to retry."
                        }),
                );
            }
            body = body.child(
                Card::new()
                    .title("Card information")
                    .description("HSM card status")
                    .icon(Icon::default().path("icons/microchip.svg"))
                    .header_right(
                        standard("hsm-refresh", cx)
                            .icon(Icon::default().path("icons/refresh-cw.svg"))
                            .disabled(self.loading)
                            .tooltip(
                                self.error
                                    .clone()
                                    .unwrap_or_else(|| "Refresh card information".into()),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .child(details),
            );
            if self
                .info
                .as_ref()
                .is_some_and(|i| i.initialized == Some(false))
            {
                body = body.child(Card::new().title("Set up HSM")
                    .description("Set your PINs before creating or importing keys")
                    .child(div().text_sm().child("1. Set a user PIN and a security officer PIN. 2. Generate a key; its ID is assigned automatically. 3. Use the key from its row in the Keys list. HSM has no default PIN before setup."))
                    .child(standard("hsm-setup", cx).label("Set up HSM").disabled(self.loading)
                        .on_click(cx.listener(|this, _, w, cx| this.open_action(Action::Setup, w, cx)))));
            }
            body = body
                .child(self.stored_list(true, cx))
                .child(self.stored_list(false, cx))
                .child(
                    Card::new()
                        .title("PIN")
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
                        .title("Reset")
                        .description("Erase HSM contents and configure new PINs")
                        .icon(Icon::default().path("icons/trash-2.svg"))
                        .child(self.action_row(
                            "Factory reset HSM",
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
                            .header_right(standard("copy-hsm-result", cx).label("Copy").on_click(
                                cx.listener(|this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        this.result.clone(),
                                    ))
                                }),
                            ))
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

fn object_kind(id: u16) -> &'static str {
    match id >> 8 {
        0xCC => "Private / secret key",
        0xC4 => "Key description",
        0xCE => "End-entity certificate",
        0xCA => "CA certificate",
        0xC8 => "Certificate description",
        0xC9 => "Data description",
        0xCF => "Readable data",
        0xCD => "Protected data",
        _ => "Object",
    }
}
