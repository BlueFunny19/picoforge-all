//! Audit controls, compact event list, and verification status.
use super::presentation::{event_detail, event_title};
use super::view_model::AuditViewModel;
use crate::ui::components::{button::standard, card::Card, information, page_view::PageView};
use crate::ui::models::device::audit;
use gpui::*;
use gpui_component::{
    ActiveTheme, Disableable, Icon, StyledExt, Theme, h_flex, switch::Switch, v_flex,
};

fn short_hex(bytes: &[u8; 32]) -> String {
    let h = hex::encode(bytes);
    format!("{}…{}", &h[..8], &h[h.len() - 8..])
}
fn event_row(entry: &audit::AuditEntry, theme: &Theme) -> AnyElement {
    let row = h_flex()
        .w_full()
        .h_full()
        .gap_3()
        .p_4()
        .border_1()
        .border_color(theme.border)
        .rounded_lg()
        .child(
            div()
                .flex_shrink_0()
                .p_2()
                .rounded_lg()
                .bg(rgb(0x252528))
                .child(Icon::default().path("icons/scroll-text.svg")),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(div().child(event_title(entry)))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(event_detail(entry)),
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(format!("#{}", entry.seq)),
        );
    div()
        .w_full()
        .h(px(88.))
        .pb_2()
        .child(row)
        .into_any_element()
}

impl AuditViewModel {
    fn journal_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(journal) = &self.journal else {
            return div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(crate::i18n::tr("No events loaded."))
                .into_any_element();
        };
        let fingerprint_card = |title: &str, summary: String, hash: &[u8; 32]| {
            v_flex()
                .w_full()
                .min_w_0()
                .gap_2()
                .p_4()
                .border_1()
                .border_color(theme.border)
                .rounded_lg()
                .child(div().font_semibold().child(title.to_string()))
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(summary),
                )
                .child(information::field(
                    crate::i18n::tr("Fingerprint"),
                    short_hex(hash),
                    &theme,
                ))
        };
        let query = self.event_search.read(cx).text().to_string().to_lowercase();
        let selected = crate::ui::components::form::selected_key(
            &self.event_filter,
            super::view_model::EVENT_FILTERS,
            cx,
        );
        let entries: Vec<_> = journal
            .entries
            .iter()
            .rev()
            .filter(|entry| {
                let category = match entry.event {
                    2 | 3 | 15 | 16 => 1,
                    4..=11 | 18..=20 | 23 => 2,
                    12..=14 => 4,
                    17 => 5,
                    _ => 3,
                };
                (selected == 0 || selected == category)
                    && crate::ui::components::collection::matches(
                        &query,
                        &format!(
                            "{} {} {}",
                            entry.seq,
                            event_title(entry),
                            event_detail(entry)
                        ),
                    )
            })
            .cloned()
            .collect();
        let height = crate::preferences::list_height(entries.len(), 88.);
        let rows = if entries.is_empty() {
            div()
                .p_4()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(if query.is_empty() && selected == 0 {
                    crate::i18n::tr("No recorded events.")
                } else {
                    crate::i18n::tr("No matching events.")
                })
                .into_any_element()
        } else {
            uniform_list("audit-events", entries.len(), move |range, _, cx| {
                range
                    .map(|index| event_row(&entries[index], cx.theme()))
                    .collect()
            })
            .track_scroll(self.event_scroll.clone())
            .w_full()
            .h(px(height))
            .into_any_element()
        };
        v_flex()
            .w_full()
            .gap_4()
            .child(
                information::grid()
                    .child(fingerprint_card(
                        crate::i18n::tr("Earlier history"),
                        crate::i18n::format(
                            "{0} earlier events summarized",
                            &[format!("{}", journal.start)],
                        ),
                        &journal.epoch,
                    ))
                    .child(fingerprint_card(
                        crate::i18n::tr("Current log"),
                        crate::i18n::format(
                            "{0} stored events",
                            &[format!("{}", journal.entries.len())],
                        ),
                        &journal.head,
                    )),
            )
            .child(crate::ui::components::collection::toolbar(
                &self.event_search,
                &self.event_filter,
            ))
            .child(crate::ui::components::collection::frame(
                "audit-events-frame",
                rows,
                &self.event_scroll,
                height,
                cx,
            ))
            .into_any_element()
    }

    fn verify_body(&self, theme: &Theme) -> Option<AnyElement> {
        let Some(v) = self.verification.as_ref() else {
            return Some(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(crate::i18n::tr("Select Verify to check this device's log."))
                    .into_any_element(),
            );
        };
        Some(
            v_flex()
                .w_full()
                .gap_4()
                .child(
                    div()
                        .font_semibold()
                        .text_color(if v.authentic() {
                            theme.green
                        } else {
                            theme.danger
                        })
                        .child(if v.authentic() {
                            crate::i18n::tr("Verification passed")
                        } else {
                            crate::i18n::tr("Verification failed")
                        }),
                )
                .child(
                    information::grid()
                        .child(information::field(
                            crate::i18n::tr("Signature"),
                            if v.signature_ok {
                                crate::i18n::tr("Valid")
                            } else {
                                crate::i18n::tr("Invalid")
                            },
                            theme,
                        ))
                        .child(information::field(
                            crate::i18n::tr("Log contents"),
                            if v.head_matches {
                                crate::i18n::tr("Match the signed log")
                            } else {
                                crate::i18n::tr("Do not match")
                            },
                            theme,
                        ))
                        .child(information::field(
                            crate::i18n::tr("Device identity"),
                            match v.expected_match {
                                Some(true) => crate::i18n::tr("Matches saved key"),
                                Some(false) => crate::i18n::tr("Does not match saved key"),
                                None => crate::i18n::tr("Not compared"),
                            },
                            theme,
                        ))
                        .child(information::field(
                            crate::i18n::tr("Device fingerprint"),
                            v.fingerprint.clone(),
                            theme,
                        )),
                )
                .into_any_element(),
        )
    }
}

impl Render for AuditViewModel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const TITLE: &str = "Audit";
        const SUBTITLE: &str = "Security events and log verification.";
        if let Some((heading, body)) = self.gate(cx).message() {
            let empty = v_flex()
                .gap_2()
                .child(div().font_semibold().child(heading))
                .child(body);
            return PageView::build(TITLE, SUBTITLE, empty, cx.theme()).into_any_element();
        }
        let busy = self.loading || self.status_loading;
        let toggle = match self.enabled {
            Some(enabled) => Switch::new("audit-enabled")
                .checked(enabled)
                .disabled(busy)
                .on_click(cx.listener(|this, checked, w, cx| this.open_toggle(*checked, w, cx)))
                .into_any_element(),
            None => standard("audit-retry", cx)
                .label(crate::i18n::tr("Retry"))
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.refresh_status(cx)))
                .into_any_element(),
        };
        let read = standard("audit-read", cx)
            .label(crate::i18n::tr("Read log"))
            .disabled(busy)
            .on_click(cx.listener(|this, _, w, cx| this.open_read(w, cx)));
        let verify = standard("audit-verify", cx)
            .label(crate::i18n::tr("Verify"))
            .disabled(busy)
            .on_click(cx.listener(|this, _, w, cx| this.open_verify(w, cx)));
        let theme = cx.theme().clone();
        let status = match (self.status_loading, self.enabled) {
            (true, _) => crate::i18n::tr("Reading status…").into(),
            (_, Some(true)) => crate::i18n::tr("Recording is on").into(),
            (_, Some(false)) => crate::i18n::tr("Recording is off").into(),
            _ => self
                .status_error
                .clone()
                .unwrap_or_else(|| crate::i18n::tr("Status unavailable").into()),
        };
        let settings = Card::new()
            .title(crate::i18n::tr("Event recording"))
            .icon(Icon::default().path("icons/book-open.svg"))
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(theme.muted_foreground)
                            .child(status),
                    )
                    .child(div().flex_shrink_0().child(toggle)),
            );
        let journal = Card::new()
            .title(crate::i18n::tr("Security events"))
            .icon(Icon::default().path("icons/scroll-text.svg"))
            .header_right(read)
            .child(self.journal_body(cx));
        let verify = Card::new()
            .title(crate::i18n::tr("Verification"))
            .icon(Icon::default().path("icons/shield-check.svg"))
            .header_right(verify)
            .children(self.verify_body(&theme));
        PageView::build(
            TITLE,
            SUBTITLE,
            v_flex()
                .w_full()
                .gap_6()
                .child(settings)
                .child(journal)
                .child(verify),
            &theme,
        )
        .into_any_element()
    }
}
