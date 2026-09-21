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
