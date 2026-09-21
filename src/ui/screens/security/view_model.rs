//! Read the connected device's hardware security state.
use crate::hal::rescue::{self, RootStatus};
use crate::ui::app::AppModels;
use crate::ui::models::device::{DeviceEvent, DeviceRepo, FirmwareType};
use crate::ui::screens::offboard::{OffboardEvent, OffboardViewModel};
use gpui::*;
use gpui_component::WindowExt;

pub struct SecurityViewModel {
    pub device: Entity<DeviceRepo>,
    pub tools: Entity<OffboardViewModel>,
    pub root: Option<RootStatus>,
    pub error: Option<String>,
    pub loading: bool,
    _task: Option<Task<()>>,
}
impl SecurityViewModel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, models: &AppModels) -> Self {
        let tools = cx.new(|cx| OffboardViewModel::new(window, cx, models));
        cx.observe(&tools, |_, _, cx| cx.notify()).detach();
        cx.subscribe_in(
            &tools,
            window,
            |_, _, event: &OffboardEvent, window, cx| match event {
                OffboardEvent::Notification(message) => {
                    window.push_notification(message.clone(), cx);
                }
            },
        )
        .detach();
        let device = models.device.clone();
        cx.subscribe(&device, |this: &mut Self, _, _: &DeviceEvent, cx| {
            this.load(cx);
            cx.notify();
        })
        .detach();
        let mut this = Self {
            device,
            tools,
            root: None,
            error: None,
            loading: false,
            _task: None,
        };
        this.load(cx);
        // This view is cached for the application session; no setting is persisted.
        show_entry_warning(window, cx);
        this
    }
    pub fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        self.root = None;
        self.error = None;
        if !self
            .device
            .read(cx)
            .status
            .as_ref()
            .is_some_and(|s| s.firmware_type == FirmwareType::PicoAll)
        {
            return;
        }
        self.loading = true;
        self._task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { rescue::read_root_status() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.loading = false;
                match result {
                    Ok(root) => this.root = Some(root),
                    Err(e) => this.error = Some(e.to_string()),
                }
                cx.notify();
            });
        }));
    }
}

struct SecurityAcknowledgement {
    opened: std::time::Instant,
    acknowledged: bool,
}
impl SecurityAcknowledgement {
    fn remaining(&self) -> u64 {
        10u64.saturating_sub(self.opened.elapsed().as_secs())
    }
    fn can_close(&self) -> bool {
        self.remaining() == 0 && self.acknowledged
    }
}
impl Render for SecurityAcknowledgement {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui_component::{ActiveTheme, Disableable, h_flex, switch::Switch, v_flex};
        let remaining = self.remaining();
        v_flex().w_full().gap_5()
            .child(crate::ui::components::notice::warning(
                "Permanent hardware changes",
                "Secure Boot and Secure Lock cannot be undone. Keep a backup of the original trusted signing key before changing protection settings.",
                true))
            .child(h_flex().w_full().items_center().gap_3().p_3().rounded_lg()
                .bg(rgb(0x18181b)).border_1().border_color(cx.theme().border)
                .child(Switch::new("security-acknowledgement").checked(self.acknowledged)
                    .on_click(cx.listener(|this, checked, _, cx| {
                        this.acknowledged = *checked;
                        cx.notify();
                    })))
                .child(div().flex_1().text_sm().child("I understand these changes are permanent.")))
            .child(h_flex().justify_end().child(
                crate::ui::components::button::standard("security-continue", cx)
                    .label(if remaining > 0 { format!("Continue in {remaining}s") } else { "Continue".into() })
                    .disabled(!self.can_close())
                    .on_click(cx.listener(|this, _, w, cx| {
                        if this.can_close() { w.close_dialog(cx); }
                    }))))
    }
}
fn show_entry_warning(window: &mut Window, cx: &mut App) {
    let acknowledgement = cx.new(|cx| {
        cx.spawn(async |this: WeakEntity<SecurityAcknowledgement>, cx| {
            for _ in 0..10 {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        SecurityAcknowledgement {
            opened: std::time::Instant::now(),
            acknowledged: false,
        }
    });
    window.open_dialog(cx, move |dialog, window, _| {
        dialog
            .title("Before changing security settings")
            .width(px(560.).min(window.viewport_size().width - px(48.)))
            .margin_top(((window.viewport_size().height - px(340.)) / 2.).max(px(24.)))
            .overlay(true)
            .close_button(false)
            .overlay_closable(false)
            .keyboard(false)
            .on_cancel(|_, _, _| false)
            .on_ok(|_, _, _| false)
            .child(acknowledgement.clone())
    });
}

#[cfg(test)]
mod acknowledgement_tests {
    use super::SecurityAcknowledgement;
    use std::time::{Duration, Instant};

    #[test]
    fn acknowledgement_and_ten_seconds_are_both_required() {
        let mut state = SecurityAcknowledgement {
            opened: Instant::now(),
            acknowledged: false,
        };
        assert!(!state.can_close());
        state.acknowledged = true;
        assert!(!state.can_close());
        state.opened = Instant::now() - Duration::from_secs(10);
        assert!(state.can_close());
        state.acknowledged = false;
        assert!(!state.can_close());
    }
}
