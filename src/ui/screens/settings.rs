//! Software settings in a flat, responsive grid of grouped cards.
use crate::{
    preferences::{self, Preferences},
    ui::components::{
        card::Card,
        form::{LabeledU8, select_state},
        page_view::PageView,
    },
};
use gpui::*;
use gpui_component::{
    ActiveTheme, Icon, h_flex,
    select::{SearchableVec, Select, SelectEvent, SelectItem, SelectState},
    switch::Switch,
    v_flex,
};
#[derive(Clone)]
struct TimeZoneItem(String);
impl SelectItem for TimeZoneItem {
    type Value = String;
    fn title(&self) -> SharedString {
        let now = chrono::Utc::now();
        let (offset, name) = if let Ok(zone) = self.0.parse::<chrono_tz::Tz>() {
            (
                now.with_timezone(&zone).format("%:z").to_string(),
                self.0.as_str(),
            )
        } else {
            (
                now.with_timezone(&chrono::Local).format("%:z").to_string(),
                crate::i18n::tr("Follow system"),
            )
        };
        format!("(UTC{offset}) {name}").into()
    }
    fn value(&self) -> &String {
        &self.0
    }
}
pub enum SettingsEvent {
    LanguageChanged,
}
pub struct SettingsViewModel {
    language: Entity<SelectState<Vec<LabeledU8>>>,
    time: Entity<SelectState<SearchableVec<TimeZoneItem>>>,
    error: Option<String>,
}
impl EventEmitter<SettingsEvent> for SettingsViewModel {}
impl SettingsViewModel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let p = preferences::get();
        let language = select_state(
            window,
            cx,
            &[
                (crate::i18n::tr("Follow system"), 0),
                (crate::i18n::tr("English"), 1),
                ("简体中文", 2),
            ],
            match p.language.as_str() {
                "en_US" => 1,
                "zh_CN" => 2,
                _ => 0,
            },
        );
        let mut zones = vec![TimeZoneItem("system".into()), TimeZoneItem("UTC".into())];
        zones.extend(
            chrono_tz::TZ_VARIANTS
                .iter()
                .filter(|zone| **zone != chrono_tz::UTC)
                .map(|zone| TimeZoneItem(zone.name().into())),
        );
        let selected = zones
            .iter()
            .position(|zone| zone.0 == p.time_zone)
            .unwrap_or(0);
        let time = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(zones),
                Some(gpui_component::IndexPath::default().row(selected)),
                window,
                cx,
            )
            .searchable(true)
        });
        cx.subscribe(
            &language,
            |this, _, event: &SelectEvent<Vec<LabeledU8>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.change(cx, |p| {
                        p.language = ["system", "en_US", "zh_CN"][*value as usize].into()
                    });
                }
            },
        )
        .detach();
        cx.subscribe(
            &time,
            |this, _, event: &SelectEvent<SearchableVec<TimeZoneItem>>, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    this.change(cx, |p| p.time_zone = value.clone());
                }
            },
        )
        .detach();
        Self {
            language,
            time,
            error: None,
        }
    }
    fn change(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Preferences)) {
        let old = preferences::get();
        let mut p = old.clone();
        edit(&mut p);
        let language_changed = p.language != old.language;
        match preferences::save(p) {
            Ok(()) => {
                self.error = None;
                if language_changed {
                    cx.emit(SettingsEvent::LanguageChanged);
                }
                cx.refresh_windows();
            }
            Err(e) => self.error = Some(e),
        }
        cx.notify();
    }
    fn field(title: &str, control: impl IntoElement, cx: &App) -> Div {
        v_flex()
            .w_full()
            .min_w_0()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(crate::i18n::text(title)),
            )
            .child(control)
    }

    fn toggle(title: &str, control: impl IntoElement) -> Div {
        h_flex()
            .w_full()
            .min_w_0()
            .gap_4()
            .justify_between()
            .child(div().flex_1().min_w_0().child(crate::i18n::text(title)))
            .child(div().flex_shrink_0().child(control))
    }
}
impl Render for SettingsViewModel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = preferences::get();
        let columns = if window.bounds().size.width > px(1100.) {
            2
        } else {
            1
        };
        let sync = Switch::new("clock-sync")
            .checked(p.sync_clock)
            .on_click(cx.listener(|this, value, _, cx| this.change(cx, |p| p.sync_clock = *value)));
        let cards = div()
            .grid()
            .grid_cols(columns)
            .gap_6()
            .child(
                Card::new()
                    .title(crate::i18n::tr("Language"))
                    .icon(Icon::default().path("icons/globe.svg"))
                    .child(Self::field(
                        "Display language",
                        Select::new(&self.language).w_full(),
                        cx,
                    )),
            )
            .child(
                Card::new()
                    .title(crate::i18n::tr("Time"))
                    .icon(Icon::default().path("icons/settings.svg"))
                    .child(Self::field(
                        "Time zone",
                        Select::new(&self.time).w_full(),
                        cx,
                    ))
                    .child(Self::toggle("Sync device clock", sync)),
            );
        let mut body = v_flex().w_full().gap_6().child(cards);
        if let Some(error) = &self.error {
            body = body.child(div().text_color(cx.theme().danger).child(error.clone()));
        }
        PageView::build(crate::i18n::tr("Settings"), "", body, cx.theme())
    }
}
