use super::*;
use crate::i18n::LocalizedPlaceholder;
use std::{cell::RefCell, io::Read, rc::Rc};

#[derive(Default)]
struct ImportState {
    file_mode: bool,
    name: String,
    bytes: Vec<u8>,
    reading: bool,
}

fn read_file(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|_| crate::i18n::tr("Could not open this file.").to_string())?;
    if !file
        .metadata()
        .map_err(|_| crate::i18n::tr("Could not read file information."))?
        .is_file()
    {
        return Err(crate::i18n::tr("Select a regular file.").into());
    }
    let mut bytes = Vec::new();
    file.take((hsm::MAX_OBJECT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| crate::i18n::tr("Could not read this file.").to_string())?;
    if bytes.len() > hsm::MAX_OBJECT_BYTES {
        return Err(crate::i18n::tr("File is too large. Maximum file size: 1,800 bytes.").into());
    }
    if bytes.is_empty() {
        return Err(crate::i18n::tr("This file is empty. Choose a file with content.").into());
    }
    Ok(bytes)
}

impl HsmViewModel {
    pub(super) fn open_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        dialog::open_confirm(crate::i18n::tr("Reset HSM"), crate::i18n::tr("Delete all HSM keys and objects and restore the default settings? This cannot be undone.").into(), crate::i18n::tr("Reset HSM"), gpui_component::button::ButtonVariant::Danger, window, cx,
            move |status, _, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.loading = true;
                    cx.notify();
                    this._task = Some(cx.spawn(async move |this, cx| {
                        let result = cx.background_executor().spawn(async {
                            hsm::reset_defaults()
                        }).await;
                        let _ = this.update(cx, |this, cx| {
                            this.loading = false;
                            match result {
                                Ok(()) => {
                                            let _ = status.update(cx, |s, cx| s.set_success(crate::i18n::tr("HSM reset.").into(), cx));
                                    this.load(cx);
                                }
                                Err(e) => { let _ = status.update(cx, |s, cx| s.set_error(e.to_string(), cx)); }
                            }
                            cx.notify();
                        });
                    }));
                });
            });
    }

    pub(super) fn open_object_editor(
        &mut self,
        id: Option<u16>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let default = self.default_pin(false);
        let pin = default.input(window, cx);
        let content = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(4)
                .localized_placeholder("Enter the content to store", cx)
        });
        let errors = FormErrors::default();
        errors.watch(0, &pin, window, cx);
        errors.watch(1, &content, window, cx);
        let imported = Rc::new(RefCell::new(ImportState::default()));
        let types: &'static [(&str, u8)] = &[
            ("Protected data", 0xCD),
            ("Public data", 0xCF),
            ("Certificate", 0xCE),
            ("CA certificate", 0xCA),
        ];
        let kind = select_state(window, cx, types, 0);
        let weak = cx.entity().downgrade();
        let submit = {
            let pin = pin.clone();
            let content = content.clone();
            let kind = kind.clone();
            let errors = errors.clone();
            let imported = imported.clone();
            Rc::new(move |window: &mut Window, cx: &mut App| {
                errors.clear();
                let pin = pin.read(cx).text().to_string();
                errors.required(0, crate::i18n::tr("User PIN"), &pin);
                let import = imported.borrow();
                if import.reading {
                    errors.set(1, crate::i18n::tr("Wait for the file to finish loading."));
                }
                let bytes = if import.file_mode {
                    import.bytes.clone()
                } else {
                    content.read(cx).text().to_string().into_bytes()
                };
                if let Err(e) = hsm::validate_object_data(&bytes) {
                    errors.set(1, e.to_string());
                }
                if !errors.valid(window) {
                    return;
                }
                let choice = selected_key(&kind, types, cx);
                let args = vec![
                    pin,
                    id.map(|id| format!("{id:04X}")).unwrap_or_default(),
                    hex::encode(bytes),
                ];
                window.close_dialog(cx);
                let status =
                    dialog::open_status_dialog(crate::i18n::tr("Saving object"), window, cx);
                let _ = weak.update(cx, |this, cx| {
                    this.run(Action::Write, args, choice, status, cx)
                });
            })
        };
        window.open_dialog(cx, move |d, _, cx| {
            let imported_text = imported.clone();
            let errors_text = errors.clone();
            let imported_file = imported.clone();
            let errors_file = errors.clone();
            let file = imported.borrow();
            let mut form = v_flex()
                .gap_3()
                .child(info_card(crate::i18n::tr(
                    "Maximum content or file size: 1,800 bytes. Text is stored as UTF-8.",
                )))
                .children(default.field(&errors, 0, crate::i18n::tr("User PIN"), &pin, true));
            if let Some(id) = id {
                form = form.child(crate::i18n::format(
                    "Replacing object {0}",
                    &[format!("{:04X}", id)],
                ));
            } else {
                form = form
                    .child(crate::i18n::tr("Object type"))
                    .child(Select::new(&kind).w_full());
            }
            form = form.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("object-text")
                            .label(crate::i18n::tr("Enter text"))
                            .disabled(file.reading)
                            .on_click(move |_, w, _| {
                                let mut import = imported_text.borrow_mut();
                                import.file_mode = false;
                                errors_text.clear();
                                w.refresh();
                            }),
                    )
                    .child(
                        Button::new("object-file")
                            .label(crate::i18n::tr("Choose file"))
                            .disabled(file.reading)
                            .on_click(move |_, w, cx| {
                                let receiver = cx.prompt_for_paths(PathPromptOptions {
                                    files: true,
                                    directories: false,
                                    multiple: false,
                                    prompt: Some(
                                        crate::i18n::tr("Import object (maximum 1,800 bytes)")
                                            .into(),
                                    ),
                                });
                                let handle = w.window_handle();
                                let imported = imported_file.clone();
                                let errors = errors_file.clone();
                                imported.borrow_mut().reading = true;
                                w.refresh();
                                cx.spawn(async move |cx| {
                                    let selection = receiver.await;
                                    let result = match selection {
                                        Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                                            Some(path) => {
                                                let name = path
                                                    .file_name()
                                                    .unwrap_or_default()
                                                    .to_string_lossy()
                                                    .into_owned();
                                                Some(
                                                    cx.background_executor()
                                                        .spawn(async move {
                                                            read_file(&path)
                                                                .map(|bytes| (name, bytes))
                                                        })
                                                        .await,
                                                )
                                            }
                                            None => None,
                                        },
                                        Ok(Ok(None)) => None,
                                        _ => Some(Err(crate::i18n::tr(
                                            "Could not open the file picker.",
                                        )
                                        .into())),
                                    };
                                    let _ = cx.update_window(handle, |_, window, _| {
                                        let mut import = imported.borrow_mut();
                                        import.reading = false;
                                        if let Some(result) = result {
                                            import.file_mode = true;
                                            import.bytes.clear();
                                            import.name.clear();
                                            errors.clear();
                                            match result {
                                                Ok((name, bytes)) => {
                                                    import.name = name;
                                                    import.bytes = bytes;
                                                }
                                                Err(e) => errors.set(1, e),
                                            }
                                        }
                                        window.refresh();
                                    });
                                })
                                .detach();
                            }),
                    ),
            );
            if file.file_mode {
                form = form.child(if file.name.is_empty() {
                    crate::i18n::tr("No file selected").into()
                } else {
                    crate::i18n::format(
                        "{0} ({1} / 1,800 bytes)",
                        &[format!("{}", file.name), format!("{}", file.bytes.len())],
                    )
                });
                // Reuse the field error without exposing binary content as hex.
                if let Some(error) = errors.message(1) {
                    form = form.child(div().text_sm().text_color(rgb(0xef4444)).child(error));
                }
            } else {
                form = form
                    .child(errors.field(1, crate::i18n::tr("Content"), &content, true))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(crate::i18n::format(
                                "{0} / 1,800 bytes",
                                &[format!("{}", content.read(cx).text().to_string().len())],
                            )),
                    );
            }
            if file.reading {
                form = form.child(crate::i18n::tr("Reading file…"));
            }
            let ok = submit.clone();
            let button = submit.clone();
            d.title(if id.is_some() {
                crate::i18n::tr("Replace object")
            } else {
                crate::i18n::tr("Add object")
            })
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
                    Button::new("save")
                        .primary()
                        .label(crate::i18n::tr("Save object"))
                        .on_click(move |_, w, cx| submit(w, cx)),
                ]
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::read_file;
    #[test]
    fn file_size_limit_is_enforced_before_import() {
        let path = std::env::temp_dir().join(format!(
            "picoforge-object-size-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        for (size, accepted) in [(0, false), (1800, true), (1801, false), (65536, false)] {
            std::fs::write(&path, vec![0xAB; size]).unwrap();
            assert_eq!(read_file(&path).is_ok(), accepted);
        }
        std::fs::remove_file(&path).unwrap();
        assert!(read_file(&std::env::temp_dir()).is_err());
    }
}
