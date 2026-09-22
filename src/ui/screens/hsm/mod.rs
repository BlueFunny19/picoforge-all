//! SmartCard-HSM management screen.
mod object_editor;
use crate::hal::applets::hsm;
use crate::hal::types::FirmwareType;
use crate::i18n::LocalizedPlaceholder;
use crate::ui::app::AppModels;
use crate::ui::components::{
    applet_gate::{AppletGate, empty_state},
    button::standard,
    card::Card,
    dialog,
    form::{DefaultSecret, FormErrors, info_card, select_state, selected_key},
    information,
    page_view::PageView,
};
use crate::ui::models::device::{DeviceEvent, DeviceRepo};
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::input::{InputEvent, InputState};
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
            Self::Setup => crate::i18n::tr("Set up HSM"),
            Self::Generate => crate::i18n::tr("Generate key"),
            Self::Delete => crate::i18n::tr("Delete key"),
            Self::Crypto => crate::i18n::tr("Use key"),
            Self::Read => crate::i18n::tr("Read object"),
            Self::Write => crate::i18n::tr("Write object"),
            Self::DeleteObject => crate::i18n::tr("Delete object"),
            Self::Pin => crate::i18n::tr("Change user PIN"),
            Self::SoPin => crate::i18n::tr("Change SO PIN"),
            Self::Unblock => crate::i18n::tr("Unblock user PIN"),
            Self::Wrap => crate::i18n::tr("Export wrapped key"),
            Self::Unwrap => crate::i18n::tr("Import wrapped key"),
            Self::Dkek => crate::i18n::tr("DKEK shares"),
            Self::Initialize => crate::i18n::tr("Reset HSM"),
        }
    }
    fn fields(self) -> Vec<(&'static str, bool)> {
        match self {
            Self::Generate | Self::Delete | Self::Wrap => {
                vec![
                    (crate::i18n::tr("User PIN"), true),
                    (crate::i18n::tr("Key ID (hex, 01–FF)"), false),
                ]
            }
            Self::Crypto | Self::Unwrap => vec![
                (crate::i18n::tr("User PIN"), true),
                (crate::i18n::tr("Key ID (hex, 01–FF)"), false),
                (crate::i18n::tr("Input bytes (hex)"), true),
            ],
            Self::DeleteObject => vec![
                (crate::i18n::tr("User PIN"), true),
                (crate::i18n::tr("Object ID (hex)"), false),
            ],
            Self::Read => vec![
                (
                    crate::i18n::tr("User PIN (optional for public objects)"),
                    true,
                ),
                (crate::i18n::tr("Object ID (hex, e.g. CE01)"), false),
            ],
            Self::Write => vec![
                (crate::i18n::tr("User PIN"), true),
                (crate::i18n::tr("Object ID (hex, e.g. CE01)"), false),
                (crate::i18n::tr("Object bytes (hex)"), true),
            ],
            Self::Pin | Self::SoPin => vec![
                (crate::i18n::tr("Current PIN"), true),
                (crate::i18n::tr("New PIN"), true),
                (crate::i18n::tr("Repeat new PIN"), true),
            ],
            Self::Unblock => vec![
                ("SO PIN", true),
                (crate::i18n::tr("New user PIN"), true),
                (crate::i18n::tr("Repeat new PIN"), true),
            ],
            Self::Dkek => vec![
                (crate::i18n::tr("User PIN (required for import)"), true),
                (
                    crate::i18n::tr("DKEK share (64 hex digits; empty reads status)"),
                    true,
                ),
            ],
            Self::Setup => vec![
                (crate::i18n::tr("New user PIN (6–16 characters)"), true),
                (crate::i18n::tr("New SO PIN (6–16 characters)"), true),
                (
                    crate::i18n::tr("DKEK shares (0 disables key backup)"),
                    false,
                ),
            ],
            Self::Initialize => Vec::new(),
        }
    }
    fn description(self) -> &'static str {
        match self {
            Self::Setup => crate::i18n::tr(
                "Set a user PIN and an SO PIN for recovery. Leave DKEK shares at 0 to keep key backup disabled.",
            ),
            Self::Initialize => crate::i18n::tr(
                "Deletes all HSM keys and objects and sets new PINs. Other applications and hardware locks are preserved.",
            ),
            Self::Delete => crate::i18n::tr("Permanently delete this key?"),
            Self::Generate => {
                crate::i18n::tr("Create a key on this device. The private key stays on the device.")
            }
            Self::DeleteObject => {
                crate::i18n::tr("Permanently deletes this certificate, metadata or data object.")
            }
            Self::Write => {
                crate::i18n::tr("Replaces the selected certificate, metadata or data object.")
            }
            Self::Wrap => crate::i18n::tr(
                "Exports a DKEK-encrypted key backup; configured DKEK shares and physical confirmation may be required.",
            ),
            Self::Unwrap => crate::i18n::tr(
                "Restore an encrypted key backup. Import the matching DKEK shares first.",
            ),
            Self::Crypto => crate::i18n::tr(
                "Uses an existing key. Input and output are bytes encoded as hex; the private key stays on the device.",
            ),
            Self::Dkek => crate::i18n::tr(
                "Reads domain 0 status or imports one DKEK share configured during HSM initialization.",
            ),
            _ => "",
        }
    }
}

const KEY_FILTERS: &[(&str, u8)] = &[
    ("All keys", 0),
    ("With certificate", 1),
    ("Without certificate", 2),
];
const OBJECT_FILTERS: &[(&str, u8)] = &[
    ("All objects", 0),
    ("Certificates", 1),
    ("Descriptions", 2),
    ("Data", 3),
];

pub struct HsmViewModel {
    device: Entity<DeviceRepo>,
    info: Option<hsm::HsmInfo>,
    loading: bool,
    loaded: bool,
    error: Option<String>,
    key_filter:
        Entity<gpui_component::select::SelectState<Vec<crate::ui::components::form::LabeledU8>>>,
    object_filter:
        Entity<gpui_component::select::SelectState<Vec<crate::ui::components::form::LabeledU8>>>,
    key_scroll: UniformListScrollHandle,
    object_scroll: UniformListScrollHandle,
    key_search: Entity<InputState>,
    object_search: Entity<InputState>,
    _task: Option<Task<()>>,
}
impl HsmViewModel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let key_filter = crate::ui::components::collection::filter(KEY_FILTERS, window, cx);
        let object_filter = crate::ui::components::collection::filter(OBJECT_FILTERS, window, cx);
        let key_search =
            cx.new(|cx| InputState::new(window, cx).localized_placeholder("Search keys", cx));
        let object_search =
            cx.new(|cx| InputState::new(window, cx).localized_placeholder("Search objects", cx));
        for input in [&key_search, &object_search] {
            cx.subscribe(input, |_, _, _: &InputEvent, cx| cx.notify())
                .detach();
        }
        let device = models.device.clone();
        cx.subscribe(&device, |this: &mut Self, _, _: &DeviceEvent, cx| {
            if this.device.read(cx).device_changed {
                this.info = None;
                this.loaded = false;
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
            key_filter,
            object_filter,
            key_scroll: UniformListScrollHandle::new(),
            object_scroll: UniformListScrollHandle::new(),
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
    fn default_pin(&self, so: bool) -> DefaultSecret {
        DefaultSecret {
            value: if so { "12345678" } else { "123456" },
            active: self
                .info
                .as_ref()
                .and_then(|i| if so { i.so_pin_default } else { i.pin_default }),
        }
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
        let default = self.default_pin(matches!(action, Action::SoPin | Action::Unblock));
        let fields: Vec<_> = action
            .fields()
            .into_iter()
            .map(|(label, secret)| {
                let input = cx.new(|cx| InputState::new(window, cx).masked(secret));
                (label, input)
            })
            .collect();
        if !matches!(action, Action::Setup | Action::Initialize) {
            fields[0].1.update(cx, |input, cx| {
                input.set_value(default.initial_value(), window, cx)
            });
        }
        if matches!(action, Action::Setup) {
            for (index, value) in [(0, "123456"), (1, "12345678")] {
                fields[index]
                    .1
                    .update(cx, |input, cx| input.set_value(value, window, cx));
            }
        }
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
                    errors.set(2, crate::i18n::tr("New PIN entries do not match."));
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
            let mut form = v_flex().gap_3();
            if !action.description().is_empty() {
                form = form.child(action.description());
            }
            if let Some(choice) = &choice {
                form = form
                    .child(crate::i18n::tr("Algorithm"))
                    .child(Select::new(choice).w_full());
            }
            for (index, (label, input)) in fields.iter().enumerate() {
                if index == 1 && matches!(action, Action::Generate | Action::Unwrap) {
                    continue;
                }
                if index == 1 && id.is_some() {
                    form = form.child(crate::i18n::format(
                        "Selected ID: {0}",
                        &[format!("{:02X}", id.unwrap())],
                    ));
                } else {
                    let required = !((index == 0 && matches!(action, Action::Read | Action::Dkek))
                        || (index == 1 && matches!(action, Action::Dkek)));
                    if index == 0 && !matches!(action, Action::Setup) {
                        form = form.children(default.field(&errors, index, label, input, required));
                    } else {
                        form = form.child(errors.field(index, label, input, required));
                    }
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
                            .label(crate::i18n::tr("Cancel"))
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
                        let output = operation_result(action, &bytes);
                        let _ = status.update(cx, |s, cx| {
                            if let Some(output) = output {
                                s.set_result(
                                    crate::i18n::tr("Operation completed").into(),
                                    output,
                                    cx,
                                );
                            } else {
                                s.set_success(crate::i18n::tr("Operation completed").into(), cx);
                            }
                        });
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
fn operation_result(action: Action, bytes: &[u8]) -> Option<dialog::OperationResult> {
    if bytes.is_empty() {
        return None;
    }
    if matches!(action, Action::Read) {
        if let Ok(text) = std::str::from_utf8(bytes) {
            if text
                .chars()
                .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
            {
                return Some(dialog::OperationResult {
                    label: crate::i18n::format("Text ({0} bytes)", &[format!("{}", bytes.len())]),
                    display: text.into(),
                    copy: text.into(),
                });
            }
        }
    }
    let copy = hex::encode_upper(bytes);
    let display = copy
        .as_bytes()
        .chunks(48)
        .map(|line| std::str::from_utf8(line).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    Some(dialog::OperationResult {
        label: crate::i18n::format("Hexadecimal ({0} bytes)", &[format!("{}", bytes.len())]),
        display,
        copy,
    })
}

fn execute(action: Action, args: &[String], choice: u8) -> Result<Vec<u8>, String> {
    let err = |e: crate::error::PFError| e.to_string();
    let bytes = |s: &str| {
        hex::decode(s.chars().filter(|c| !c.is_whitespace()).collect::<String>())
            .map_err(|_| crate::i18n::tr("Invalid hex input").to_string())
    };
    let id = || {
        u8::from_str_radix(args[1].trim(), 16)
            .map_err(|_| crate::i18n::tr("Enter a key ID from 01 to FF").to_string())
    };
    let fid = || {
        u16::from_str_radix(args[1].trim(), 16)
            .map_err(|_| crate::i18n::tr("Enter a four-digit hex object ID").to_string())
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
                return Err(crate::i18n::tr("New PIN entries do not match").into());
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
                .map_err(|_| crate::i18n::tr("Enter a DKEK share count from 0 to 16"))?;
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
        let filter = if keys {
            &self.key_filter
        } else {
            &self.object_filter
        };
        let selected = selected_key(filter, if keys { KEY_FILTERS } else { OBJECT_FILTERS }, cx);
        let scroll = if keys {
            &self.key_scroll
        } else {
            &self.object_scroll
        };
        let files = self
            .info
            .as_ref()
            .map(|i| i.files.as_slice())
            .unwrap_or_default();
        let ids: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                let kind = *id >> 8;
                let has_cert = files.contains(&(0xCE00 | (*id & 0xff)));
                let selected_match = if keys {
                    selected == 0 || (selected == 1 && has_cert) || (selected == 2 && !has_cert)
                } else {
                    selected == 0
                        || (selected == 1 && matches!(kind, 0xCE | 0xCA))
                        || (selected == 2 && matches!(kind, 0xC4 | 0xC8 | 0xC9))
                        || (selected == 3 && !matches!(kind, 0xCE | 0xCA | 0xC4 | 0xC8 | 0xC9))
                };
                selected_match
                    && crate::ui::components::collection::matches(
                        &query,
                        &crate::i18n::format(
                            "{0} {1} {2} {3}",
                            &[
                                format!(
                                    "{}",
                                    if keys {
                                        crate::i18n::tr("Key")
                                    } else {
                                        crate::i18n::tr("Object")
                                    }
                                ),
                                format!("{:04X}", id),
                                format!("{:02X}", id & 0xff),
                                format!("{}", object_kind(*id)),
                            ],
                        ),
                    )
            })
            .collect();
        let list_height = crate::preferences::list_height(ids.len(), 88.);
        let weak = cx.entity().downgrade();
        let rows = if ids.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(if !query.is_empty() || selected != 0 {
                    crate::i18n::tr("No matches")
                } else if keys {
                    crate::i18n::tr("No stored keys")
                } else {
                    crate::i18n::tr("No stored objects")
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
            .track_scroll(scroll.clone())
            .h(px(list_height))
            .w_full()
            .into_any_element()
        };
        Card::new()
            .title(if keys {
                crate::i18n::tr("Keys")
            } else {
                crate::i18n::tr("Objects")
            })
            .description(crate::i18n::format("{0} stored", &[format!("{}", total)]))
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
                    .child(crate::ui::components::collection::toolbar(search, filter))
                    .child(crate::ui::components::collection::frame(
                        if keys {
                            "hsm-keys-frame"
                        } else {
                            "hsm-objects-frame"
                        },
                        rows,
                        scroll,
                        list_height,
                        cx,
                    )),
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
                .h_full()
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
                                    crate::i18n::format("Key {0}", &[format!("{:02X}", id & 0xff)])
                                } else {
                                    crate::i18n::format("Object {0}", &[format!("{:04X}", id)])
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
        div()
            .w_full()
            .h(px(88.))
            .pb_2()
            .child(row)
            .into_any_element()
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
                    (crate::i18n::tr("Firmware"), info.version.clone()),
                    (
                        crate::i18n::tr("Free memory"),
                        crate::i18n::format("{0} bytes", &[format!("{}", info.free_memory)]),
                    ),
                    (
                        crate::i18n::tr("User PIN tries"),
                        info.pin
                            .replace(" (default)", crate::i18n::tr(" (default)")),
                    ),
                    (
                        crate::i18n::tr("Security officer PIN tries"),
                        info.so_pin
                            .replace(" (default)", crate::i18n::tr(" (default)")),
                    ),
                    (
                        crate::i18n::tr("Identity key description"),
                        if info.files.contains(&0xC400) {
                            "C400"
                        } else {
                            crate::i18n::tr("Not installed")
                        }
                        .into(),
                    ),
                    (
                        crate::i18n::tr("Identity key"),
                        if info.files.contains(&0xCC00) {
                            "CC00"
                        } else {
                            crate::i18n::tr("Not installed")
                        }
                        .into(),
                    ),
                ] {
                    let field = information::field(label, value, cx.theme());
                    details = if label == crate::i18n::tr("User PIN tries")
                        || label == crate::i18n::tr("Security officer PIN tries")
                    {
                        details.child(field.id(SharedString::from(label)).tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new(
                                crate::i18n::tr("Remaining / total tries. (default) means the factory retry limit, not a default PIN. A dash means this firmware does not report the limit.")
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
                            crate::i18n::tr("Reading card information…")
                        } else {
                            crate::i18n::tr("Card information unavailable. Refresh to retry.")
                        }),
                );
            }
            body = body.child(
                Card::new()
                    .title(crate::i18n::tr("Card information"))
                    .icon(Icon::default().path("icons/microchip.svg"))
                    .header_right(
                        standard("hsm-refresh", cx)
                            .icon(Icon::default().path("icons/refresh-cw.svg"))
                            .disabled(self.loading)
                            .tooltip(self.error.clone().unwrap_or_else(|| {
                                crate::i18n::tr("Refresh card information").into()
                            }))
                            .on_click(cx.listener(|this, _, _, cx| this.load(cx))),
                    )
                    .child(details),
            );
            if self
                .info
                .as_ref()
                .is_some_and(|i| i.initialized == Some(false))
            {
                body = body.child(Card::new().title(crate::i18n::tr("Set up HSM"))
                    .description(crate::i18n::tr("Set your PINs before creating or importing keys"))
                    .child(div().text_sm().child(crate::i18n::tr("1. Set a user PIN and a security officer PIN. 2. Generate a key; its ID is assigned automatically. 3. Use the key from its row in the Keys list. HSM has no default PIN before setup.")))
                    .child(standard("hsm-setup", cx).label(crate::i18n::tr("Set up HSM")).disabled(self.loading)
                        .on_click(cx.listener(|this, _, w, cx| this.open_action(Action::Setup, w, cx)))));
            }
            body = body
                .child(self.stored_list(true, cx))
                .child(self.stored_list(false, cx))
                .child(
                    Card::new()
                        .title("PIN")
                        .icon(Icon::default().path("icons/lock.svg"))
                        .child(self.action_row(
                            crate::i18n::tr("User PIN"),
                            crate::i18n::tr(
                                "Authorizes private key operations and protected objects.",
                            ),
                            &[Action::Pin],
                            cx,
                        ))
                        .child(self.action_row(
                            crate::i18n::tr("Security officer PIN"),
                            crate::i18n::tr("Authorizes user PIN recovery."),
                            &[Action::SoPin, Action::Unblock],
                            cx,
                        )),
                )
                .child(
                    Card::new()
                        .title(crate::i18n::tr("Key backup"))
                        .description(crate::i18n::tr(
                            "Protect and restore keys using DKEK shares",
                        ))
                        .icon(Icon::default().path("icons/save.svg"))
                        .child(self.action_row(
                            crate::i18n::tr("Wrapped keys"),
                            crate::i18n::tr(
                                "Export an encrypted key or restore it into an unused slot.",
                            ),
                            &[Action::Wrap, Action::Unwrap],
                            cx,
                        ))
                        .child(self.action_row(
                            crate::i18n::tr("DKEK shares"),
                            crate::i18n::tr("Read the wrapping domain status or import a share."),
                            &[Action::Dkek],
                            cx,
                        )),
                )
                .child(
                    Card::new()
                        .title(crate::i18n::tr("Reset"))
                        .icon(Icon::default().path("icons/trash-2.svg"))
                        .child(self.action_row(
                            crate::i18n::tr("Factory reset HSM"),
                            crate::i18n::tr(
                                "Deletes all HSM keys and objects. This cannot be undone.",
                            ),
                            &[Action::Initialize],
                            cx,
                        )),
                );
        }
        PageView::build(
            "HSM",
            crate::i18n::tr("Manage SmartCard-HSM keys, certificates and PINs."),
            body,
            cx.theme(),
        )
    }
}

fn object_kind(id: u16) -> &'static str {
    match id >> 8 {
        0xCC => crate::i18n::tr("Private / secret key"),
        0xC4 => crate::i18n::tr("Key description"),
        0xCE => crate::i18n::tr("End-entity certificate"),
        0xCA => crate::i18n::tr("CA certificate"),
        0xC8 => crate::i18n::tr("Certificate description"),
        0xC9 => crate::i18n::tr("Data description"),
        0xCF => crate::i18n::tr("Readable data"),
        0xCD => crate::i18n::tr("Protected data"),
        _ => crate::i18n::tr("Object"),
    }
}

#[cfg(test)]
mod result_tests {
    use super::*;
    #[core::prelude::v1::test]
    fn object_text_and_binary_results_keep_copy_bytes() {
        let text = "a note\n中文";
        let output = operation_result(Action::Read, text.as_bytes()).unwrap();
        assert_eq!(output.display, text);
        assert_eq!(output.copy, text);
        let bytes = vec![0xFE; 60];
        let output = operation_result(Action::Read, &bytes).unwrap();
        assert!(output.display.contains('\n'));
        assert_eq!(hex::decode(output.copy).unwrap(), bytes);
        assert!(operation_result(Action::Pin, &[]).is_none());
        assert_eq!(
            operation_result(Action::Crypto, b"abc").unwrap().copy,
            "616263"
        );
    }
}
