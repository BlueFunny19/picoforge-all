//! Audit screen rendering.

use crate::ui::components::button::standard;
use crate::ui::components::card::Card;
use crate::ui::components::page_view::PageView;
use crate::ui::models::device::audit;
use crate::ui::screens::audit::view_model::AuditViewModel;
use gpui::*;
use gpui_component::Disableable;
use gpui_component::{ActiveTheme, Icon, StyledExt, Theme, h_flex, v_flex};

fn empty_state(heading: &str, body: String, theme: &Theme) -> AnyElement {
    v_flex()
        .items_center()
        .justify_center()
        .h_64()
        .gap_2()
        .border_1()
        .border_color(theme.border)
        .rounded_xl()
        .child(div().font_semibold().child(heading.to_string()))
        .child(
            div()
                .text_sm()
                .max_w(px(380.))
                .text_color(theme.muted_foreground)
                .child(body),
        )
        .into_any_element()
}

fn mono(theme: &Theme, s: String) -> AnyElement {
    div()
        .font_family("monospace")
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(s)
        .into_any_element()
}

fn short_hex(bytes: &[u8; 32]) -> String {
    let h = hex::encode(bytes);
    format!("{}…{}", &h[..8], &h[h.len() - 8..])
}

impl AuditViewModel {
    fn entry_row(entry: &audit::AuditEntry, theme: &Theme) -> AnyElement {
        h_flex()
            .gap_3()
            .py_1()
            .text_sm()
            .child(
                div()
                    .w(px(56.))
                    .text_color(theme.muted_foreground)
                    .child(entry.seq.to_string()),
            )
            .child(
                div()
                    .w(px(72.))
                    .text_color(theme.muted_foreground)
                    .child(format!("{:.1}s", entry.uptime_s())),
            )
            .child(div().w(px(160.)).font_medium().child(entry.event_label()))
            .child(
                div()
                    .w(px(40.))
                    .text_color(theme.muted_foreground)
                    .child(entry.aux.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .font_family("monospace")
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(entry.detail_hex()),
            )
            .into_any_element()
    }

    fn journal_body(&self, theme: &Theme) -> AnyElement {
        let Some(j) = &self.journal else {
            return div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Read the journal to view the security-event log.")
                .into_any_element();
        };

        let header = h_flex()
            .gap_3()
            .pb_1()
            .text_xs()
            .font_semibold()
            .text_color(theme.muted_foreground)
            .child(div().w(px(56.)).child("seq"))
            .child(div().w(px(72.)).child("uptime"))
            .child(div().w(px(160.)).child("event"))
            .child(div().w(px(40.)).child("aux"))
            .child(div().flex_1().child("detail"));

        let mut rows = vec![header.into_any_element()];
        for e in &j.entries {
            rows.push(Self::entry_row(e, theme));
        }
        if j.entries.is_empty() {
            rows.push(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("No live entries in the window.")
                    .into_any_element(),
            );
        }

        v_flex()
            .gap_2()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().child(format!(
                        "Window [{}, {}). {} entries, {} folded into the epoch",
                        j.start,
                        j.seq_next,
                        j.entries.len(),
                        j.start,
                    )))
                    .child(mono(theme, format!("epoch  {}", short_hex(&j.epoch))))
                    .child(mono(
                        theme,
                        format!("head   {}  (chain OK)", short_hex(&j.head)),
                    )),
            )
            .child(div().h(px(1.)).bg(theme.border))
            .child(v_flex().gap_0p5().children(rows))
            .into_any_element()
    }

    fn verify_body(&self, theme: &Theme) -> AnyElement {
        let Some(v) = &self.verification else {
            return div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("Verify a checkpoint to check the journal signature. An expected key also checks the device identity.")
                .into_any_element();
        };

        let (label, color) = if v.authentic() {
            ("Authentic ✓", theme.green)
        } else {
            ("Not trusted ✗", theme.danger)
        };

        let kv = |k: &str, val: String| {
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(k.to_string()),
                )
                .child(div().font_family("monospace").text_xs().child(val))
        };

        let expected_line = match v.expected_match {
            Some(true) => Some(("Pinned key", "matches ✓".to_string())),
            Some(false) => Some(("Pinned key", "MISMATCH ✗".to_string())),
            None => None,
        };

        let mut col = v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().w(px(10.)).h(px(10.)).rounded_full().bg(color))
                    .child(div().font_semibold().text_color(color).child(label)),
            )
            .child(
                crate::ui::components::information::grid()
                    .child(crate::ui::components::information::field(
                        "Signature",
                        if v.signature_ok {
                            "Verified"
                        } else {
                            "Invalid"
                        },
                        theme,
                    ))
                    .child(crate::ui::components::information::field(
                        "Chain head",
                        if v.head_matches { "Bound" } else { "Mismatch" },
                        theme,
                    ))
                    .child(crate::ui::components::information::field(
                        "Checkpoint sequence",
                        v.seq_signed.to_string(),
                        theme,
                    )),
            )
            .child(kv("Attestation key", v.pubkey_hex.clone()))
            .child(kv(
                "Fingerprint (pin later with Expected key)",
                v.fingerprint.clone(),
            ));
        if let Some((k, val)) = expected_line {
            col = col.child(kv(k, val));
        }
        col.into_any_element()
    }
}

impl Render for AuditViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const TITLE: &str = "Audit";
        const SUBTITLE: &str = "Tamper-evident security journal.";

        if let Some((heading, body)) = self.gate(cx).message() {
            let theme = cx.theme();
            return PageView::build(TITLE, SUBTITLE, empty_state(heading, body, theme), theme)
                .into_any_element();
        }

        let busy = self.loading || self.status_loading;
        let read_btn = standard("audit-read", cx)
            .label("Read journal")
            .disabled(busy)
            .on_click(cx.listener(|this, _, window, cx| this.open_read(window, cx)));
        let verify_btn = standard("audit-verify", cx)
            .label("Verify")
            .disabled(busy)
            .on_click(cx.listener(|this, _, window, cx| this.open_verify(window, cx)));
        let toggle_btn = match self.enabled {
            Some(enabled) => standard("audit-toggle", cx)
                .label(if enabled { "Disable" } else { "Enable" })
                .disabled(busy)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.open_toggle(!enabled, window, cx)),
                ),
            None => standard("audit-retry", cx)
                .label("Retry")
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.refresh_status(cx))),
        };
        let theme = cx.theme();
        let row = |title: &str, description: String, action: AnyElement| {
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .gap_4()
                .p_4()
                .border_1()
                .border_color(theme.border)
                .rounded_lg()
                .child(
                    v_flex().min_w_0().gap_1().child(title.to_string()).child(
                        div()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(description),
                    ),
                )
                .child(action)
        };
        let status = if let Some(error) = &self.status_error {
            format!("Could not read status. {error}")
        } else if self.status_loading {
            "Reading status…".into()
        } else {
            match self.enabled {
                Some(true) => "On. Security events are being recorded.".into(),
                Some(false) => "Off. New security events are not being recorded.".into(),
                None => "Status unavailable. Retry to read it again.".into(),
            }
        };
        let status_card = Card::new()
            .title("Journal settings")
            .description("Choose whether the device records security events")
            .icon(Icon::default().path("icons/book-open.svg"))
            .child(row(
                "Record security events",
                status,
                toggle_btn.into_any_element(),
            ));
        let journal_card = Card::new()
            .title("Security events")
            .description("Review device activity and configuration changes")
            .icon(Icon::default().path("icons/scroll-text.svg"))
            .child(
                v_flex()
                    .gap_4()
                    .child(row(
                        "Read journal",
                        "Load the events stored on this device.".into(),
                        read_btn.into_any_element(),
                    ))
                    .child(self.journal_body(theme)),
            );
        let verify_card = Card::new()
            .title("Journal verification")
            .description("Check the journal signature and device identity")
            .icon(Icon::default().path("icons/shield-check.svg"))
            .child(
                v_flex()
                    .gap_4()
                    .child(row(
                        "Verify checkpoint",
                        "Compare an expected device key when you have one.".into(),
                        verify_btn.into_any_element(),
                    ))
                    .child(self.verify_body(theme)),
            );

        let content = v_flex()
            .gap_6()
            .child(status_card)
            .child(journal_card)
            .child(verify_card);
        PageView::build(TITLE, SUBTITLE, content, theme).into_any_element()
    }
}
