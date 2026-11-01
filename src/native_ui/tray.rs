//! متحكم منطقة الإشعارات الموحد (System Tray Controller — Phase 1 & Phase 5)
//!
//! يطبق:
//! - أيقونة StormGet بجوار الساعة في Windows مع منع إنشاء أيقونات مكررة (`try_acquire_single_tray_slot`).
//! - الضغط بالزر الأيسر: إعادة إظهار النافذة إذا كانت مخفية، إعادتها للوضع الطبيعي إذا كانت مصغرة، وجعلها في المقدمة مع استعادة آخر حجم وموضع محفوظين.
//! - الضغط بالزر الأيمن: قائمة سياقية احترافية تضم العناصر الـ 10 المطلوبة مع فواصل منظمة وأيقونات واضحة:
//!   1. فتح StormGet (`OpenMain`)
//!   2. التنزيلات النشطة (`ShowActiveDownloads`)
//!   3. إضافة تنزيل جديد (`AddNewDownload`)
//!   4. إيقاف جميع التنزيلات مؤقتًا (`PauseAllDownloads`)
//!   5. استئناف جميع التنزيلات (`ResumeAllDownloads`)
//!   6. الجدولة (`OpenScheduler`)
//!   7. الإعدادات (`OpenSettings`)
//!   8. التشخيص (`OpenDiagnostics`)
//!   9. حول StormGet (`OpenAbout`)
//!   10. خروج (`ExitApplication`)

use crate::core::desktop_services::{
    release_single_tray_slot, try_acquire_single_tray_slot, TrayMenuCommand,
    TRAY_ID_ABOUT, TRAY_ID_ACTIVE_DOWNLOADS, TRAY_ID_ADD_NEW, TRAY_ID_DIAGNOSTICS, TRAY_ID_EXIT,
    TRAY_ID_OPEN_MAIN, TRAY_ID_PAUSE_ALL, TRAY_ID_RESUME_ALL, TRAY_ID_SCHEDULER, TRAY_ID_SETTINGS,
};
use std::sync::mpsc::{self, Receiver, Sender};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

pub use crate::core::desktop_services::TrayMenuCommand as TrayAction;

pub struct TrayController {
    icon: TrayIcon,
    actions: Receiver<TrayMenuCommand>,
}

impl TrayController {
    pub fn new(ctx: eframe::egui::Context) -> Result<Self, String> {
        let repaint_cb: Box<dyn Fn() + Send + Sync> = Box::new(move || {
            ctx.request_repaint();
        });
        Self::new_with_wakeup(repaint_cb)
    }

    pub fn new_with_wakeup(wakeup: Box<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        if !try_acquire_single_tray_slot() {
            return Err("System tray icon is already active for this StormGet process".to_string());
        }

        let menu = build_stormget_tray_menu().map_err(|err| {
            release_single_tray_slot();
            err
        })?;

        let (rgba, w, h) = super::stormget_app_icon_rgba();
        let icon = Icon::from_rgba(rgba, w, h).map_err(|error| {
            release_single_tray_slot();
            error.to_string()
        })?;

        let (action_tx, actions) = mpsc::channel();
        install_event_handlers(action_tx, wakeup);

        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("StormGet — مدير ومسرع التنزيلات الاحترافي")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| {
                release_single_tray_slot();
                error.to_string()
            })?;

        Ok(Self {
            icon: tray_icon,
            actions,
        })
    }

    pub fn poll_actions(&self) -> Vec<TrayMenuCommand> {
        self.actions.try_iter().collect()
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.icon.set_visible(visible);
    }

    pub fn set_tooltip(&self, tooltip: &str) {
        let _ = self.icon.set_tooltip(Some(tooltip));
    }
}

impl Drop for TrayController {
    fn drop(&mut self) {
        release_single_tray_slot();
    }
}

fn build_stormget_tray_menu() -> Result<Menu, String> {
    let menu = Menu::new();

    let item_open = MenuItem::with_id(
        TRAY_ID_OPEN_MAIN,
        TrayMenuCommand::OpenMain.label_bilingual(),
        true,
        None,
    );
    let item_active = MenuItem::with_id(
        TRAY_ID_ACTIVE_DOWNLOADS,
        TrayMenuCommand::ShowActiveDownloads.label_bilingual(),
        true,
        None,
    );
    let item_add = MenuItem::with_id(
        TRAY_ID_ADD_NEW,
        TrayMenuCommand::AddNewDownload.label_bilingual(),
        true,
        None,
    );
    let sep1 = PredefinedMenuItem::separator();

    let item_pause_all = MenuItem::with_id(
        TRAY_ID_PAUSE_ALL,
        TrayMenuCommand::PauseAllDownloads.label_bilingual(),
        true,
        None,
    );
    let item_resume_all = MenuItem::with_id(
        TRAY_ID_RESUME_ALL,
        TrayMenuCommand::ResumeAllDownloads.label_bilingual(),
        true,
        None,
    );
    let sep2 = PredefinedMenuItem::separator();

    let item_scheduler = MenuItem::with_id(
        TRAY_ID_SCHEDULER,
        TrayMenuCommand::OpenScheduler.label_bilingual(),
        true,
        None,
    );
    let item_settings = MenuItem::with_id(
        TRAY_ID_SETTINGS,
        TrayMenuCommand::OpenSettings.label_bilingual(),
        true,
        None,
    );
    let item_diagnostics = MenuItem::with_id(
        TRAY_ID_DIAGNOSTICS,
        TrayMenuCommand::OpenDiagnostics.label_bilingual(),
        true,
        None,
    );
    let item_about = MenuItem::with_id(
        TRAY_ID_ABOUT,
        TrayMenuCommand::OpenAbout.label_bilingual(),
        true,
        None,
    );
    let sep3 = PredefinedMenuItem::separator();

    let item_exit = MenuItem::with_id(
        TRAY_ID_EXIT,
        TrayMenuCommand::ExitApplication.label_bilingual(),
        true,
        None,
    );

    menu.append_items(&[
        &item_open,
        &item_active,
        &item_add,
        &sep1,
        &item_pause_all,
        &item_resume_all,
        &sep2,
        &item_scheduler,
        &item_settings,
        &item_diagnostics,
        &item_about,
        &sep3,
        &item_exit,
    ])
    .map_err(|error| error.to_string())?;

    Ok(menu)
}

fn install_event_handlers(
    action_tx: Sender<TrayMenuCommand>,
    wakeup: Box<dyn Fn() + Send + Sync>,
) {
    let wakeup_arc = std::sync::Arc::new(wakeup);
    let menu_action_tx = action_tx.clone();
    let menu_wakeup = std::sync::Arc::clone(&wakeup_arc);

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(cmd) = TrayMenuCommand::from_menu_id(event.id().as_ref()) {
            let _ = menu_action_tx.send(cmd);
            menu_wakeup();
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if matches!(
            event,
            TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
        ) {
            let _ = action_tx.send(TrayMenuCommand::OpenMain);
            wakeup_arc();
        }
    }));
}
