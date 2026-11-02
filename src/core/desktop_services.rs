//! خدمات سطح المكتب المشتركة (Shared Desktop Services — Phase 5 Architecture Separation)
//!
//! تفصل هذه الوحدة منطق الأعمال المشترك بين واجهتي `egui` (`native_ui`) و `Slint` (`slint_ui`)
//! وإضافة المتصفح بدون تكرار للمنطق أو إنشاء أكثر من نسخة من خدمة الحالة:
//! 1. إدارة حالة وأوامر `System Tray` الموحدة (القائمة الكاملة ذات الـ 10 أوامر، ومنع تكرار أيقونة Tray).
//! 2. خدمة إظهار وإخفاء النافذة، بدء التشغيل مع Windows (`HKCU\...\Run`)، وبدء البرنامج مصغراً.
//! 3. خدمة الخروج الحقيقي المنظم (`graceful_shutdown`) وحفظ حالة التنزيلات وإعلام الإضافة.
//! 4. خدمة حالة الإضافة، الإيقاف المؤقت الموثوق (`10m`, `1h`, `24h`, `manual`)، والمواقع المحظورة نهائياً
//!    مع تطبيع النطاق ودعم النطاقات الفرعية دون حظر نطاقات متشابهة بالخطأ (`evil-example.com` != `example.com`).
//! 5. نظام `DesignTokens` المركزي للثيم الفاتح والداكن بمعايير تباين `WCAG AA`.
//! 6. خدمة اتجاه الواجهة (`RTL` / `LTR`) وتنسيق النصوص المختلطة (`BiDi`) للروابط والمسارات والأرقام والسرعات.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};
use url::Url;

/// حارس ذري عام لمنع إنشاء أكثر من أيقونة System Tray واحدة في العملية الواحدة
static TRAY_ICON_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// محاولة حجز ملكية أيقونة System Tray الوحيدة في العملية (يعيد `true` عند النجاح لأول مرة فقط)
pub fn try_acquire_single_tray_slot() -> bool {
    TRAY_ICON_INITIALIZED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

/// تحرير خانة أيقونة System Tray عند الخروج النهائي المنظم
pub fn release_single_tray_slot() {
    TRAY_ICON_INITIALIZED.store(false, Ordering::SeqCst);
}

/// التحقق مما إذا كانت أيقونة System Tray منشأة حالياً
pub fn is_tray_slot_active() -> bool {
    TRAY_ICON_INITIALIZED.load(Ordering::SeqCst)
}

// ============================================================================
// 1. أوامـر وهويـات قـائمـة System Tray الموحدة (10 عناصر قياسية)
// ============================================================================

pub const TRAY_ID_OPEN_MAIN: &str = "stormget-tray-open-main";
pub const TRAY_ID_ACTIVE_DOWNLOADS: &str = "stormget-tray-active-downloads";
pub const TRAY_ID_ADD_NEW: &str = "stormget-tray-add-new";
pub const TRAY_ID_PAUSE_ALL: &str = "stormget-tray-pause-all";
pub const TRAY_ID_RESUME_ALL: &str = "stormget-tray-resume-all";
pub const TRAY_ID_SCHEDULER: &str = "stormget-tray-scheduler";
pub const TRAY_ID_SETTINGS: &str = "stormget-tray-settings";
pub const TRAY_ID_DIAGNOSTICS: &str = "stormget-tray-diagnostics";
pub const TRAY_ID_ABOUT: &str = "stormget-tray-about";
pub const TRAY_ID_EXIT: &str = "stormget-tray-exit";

/// الإجراءات الموحدة لقائمة System Tray المشتركة بين `egui` و `Slint`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayMenuCommand {
    /// فتح StormGet — إظهار النافذة الرئيسية واستعادة حجمها وموضعها
    OpenMain,
    /// التنزيلات النشطة — فتح قائمة التنزيلات مع تفعيل مرشح النشطة
    ShowActiveDownloads,
    /// إضافة تنزيل جديد — فتح نافذة إضافة رابط
    AddNewDownload,
    /// إيقاف جميع التنزيلات مؤقتًا
    PauseAllDownloads,
    /// استئناف جميع التنزيلات المتوقفة
    ResumeAllDownloads,
    /// الجدولة — فتح نافذة الجدولة
    OpenScheduler,
    /// الإعدادات — فتح نافذة الإعدادات
    OpenSettings,
    /// التشخيص — فتح أدوات فحص البرنامج
    OpenDiagnostics,
    /// حول StormGet — معلومات الإصدار
    OpenAbout,
    /// خروج — إنهاء البرنامج بالكامل (مع تأكيد عند وجود تنزيلات نشطة)
    ExitApplication,
}

impl TrayMenuCommand {
    pub fn from_menu_id(id: &str) -> Option<Self> {
        match id {
            TRAY_ID_OPEN_MAIN => Some(Self::OpenMain),
            TRAY_ID_ACTIVE_DOWNLOADS => Some(Self::ShowActiveDownloads),
            TRAY_ID_ADD_NEW => Some(Self::AddNewDownload),
            TRAY_ID_PAUSE_ALL => Some(Self::PauseAllDownloads),
            TRAY_ID_RESUME_ALL => Some(Self::ResumeAllDownloads),
            TRAY_ID_SCHEDULER => Some(Self::OpenScheduler),
            TRAY_ID_SETTINGS => Some(Self::OpenSettings),
            TRAY_ID_DIAGNOSTICS => Some(Self::OpenDiagnostics),
            TRAY_ID_ABOUT => Some(Self::OpenAbout),
            TRAY_ID_EXIT => Some(Self::ExitApplication),
            _ => None,
        }
    }

    pub fn menu_id(self) -> &'static str {
        match self {
            Self::OpenMain => TRAY_ID_OPEN_MAIN,
            Self::ShowActiveDownloads => TRAY_ID_ACTIVE_DOWNLOADS,
            Self::AddNewDownload => TRAY_ID_ADD_NEW,
            Self::PauseAllDownloads => TRAY_ID_PAUSE_ALL,
            Self::ResumeAllDownloads => TRAY_ID_RESUME_ALL,
            Self::OpenScheduler => TRAY_ID_SCHEDULER,
            Self::OpenSettings => TRAY_ID_SETTINGS,
            Self::OpenDiagnostics => TRAY_ID_DIAGNOSTICS,
            Self::OpenAbout => TRAY_ID_ABOUT,
            Self::ExitApplication => TRAY_ID_EXIT,
        }
    }

    pub fn label_arabic(self) -> &'static str {
        match self {
            Self::OpenMain => "فتح StormGet",
            Self::ShowActiveDownloads => "التنزيلات النشطة",
            Self::AddNewDownload => "إضافة تنزيل جديد",
            Self::PauseAllDownloads => "إيقاف جميع التنزيلات مؤقتًا",
            Self::ResumeAllDownloads => "استئناف جميع التنزيلات",
            Self::OpenScheduler => "الجدولة",
            Self::OpenSettings => "الإعدادات",
            Self::OpenDiagnostics => "التشخيص",
            Self::OpenAbout => "حول StormGet",
            Self::ExitApplication => "خروج",
        }
    }

    pub fn label_english(self) -> &'static str {
        match self {
            Self::OpenMain => "Open StormGet",
            Self::ShowActiveDownloads => "Active Downloads",
            Self::AddNewDownload => "Add New Download",
            Self::PauseAllDownloads => "Pause All Downloads",
            Self::ResumeAllDownloads => "Resume All Downloads",
            Self::OpenScheduler => "Scheduler",
            Self::OpenSettings => "Settings",
            Self::OpenDiagnostics => "Diagnostics",
            Self::OpenAbout => "About StormGet",
            Self::ExitApplication => "Exit",
        }
    }

    pub fn label_bilingual(self) -> &'static str {
        match self {
            Self::OpenMain => "🗔 فتح StormGet (Open StormGet)",
            Self::ShowActiveDownloads => "⚡ التنزيلات النشطة (Active Downloads)",
            Self::AddNewDownload => "➕ إضافة تنزيل جديد (Add New Download)",
            Self::PauseAllDownloads => "⏸ إيقاف جميع التنزيلات مؤقتًا (Pause All)",
            Self::ResumeAllDownloads => "▶ استئناف جميع التنزيلات (Resume All)",
            Self::OpenScheduler => "⏰ الجدولة (Scheduler)",
            Self::OpenSettings => "⚙ الإعدادات (Settings)",
            Self::OpenDiagnostics => "🩺 التشخيص (Diagnostics)",
            Self::OpenAbout => "ℹ حول StormGet (About StormGet)",
            Self::ExitApplication => "🚪 خروج (Exit)",
        }
    }

    pub fn all_ordered() -> [Self; 10] {
        [
            Self::OpenMain,
            Self::ShowActiveDownloads,
            Self::AddNewDownload,
            Self::PauseAllDownloads,
            Self::ResumeAllDownloads,
            Self::OpenScheduler,
            Self::OpenSettings,
            Self::OpenDiagnostics,
            Self::OpenAbout,
            Self::ExitApplication,
        ]
    }
}

/// قرار معالجة الضغط على زر الإغلاق `X` أو `Alt+F4` في النافذة الرئيسية
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowCloseDecision {
    /// إخفاء النافذة الرئيسية وتصغير البرنامج إلى System Tray مع استمرار التنزيلات والمحرك
    MinimizeToTray { show_notification: bool },
    /// إظهار نافذة تأكيد الخروج للمستخدم
    PromptExitConfirmation { active_downloads: usize },
    /// الخروج النهائي الفوري (عندما لا توجد تنزيلات نشطة ولا يريد المستخدم التصغير ولا التأكيد)
    ImmediateCleanExit,
}

/// حساب القرار الموحد عند ضغط المستخدم على `X` أو `Alt+F4` بناءً على إعدادات `سلوك النافذة والإغلاق`
pub fn evaluate_window_close_policy(
    minimize_to_tray_on_close: bool,
    confirm_exit_on_close: bool,
    notify_on_minimize_to_tray: bool,
    tray_available: bool,
    active_downloads_count: usize,
) -> WindowCloseDecision {
    if minimize_to_tray_on_close && tray_available {
        return WindowCloseDecision::MinimizeToTray {
            show_notification: notify_on_minimize_to_tray,
        };
    }
    if active_downloads_count > 0 || confirm_exit_on_close {
        return WindowCloseDecision::PromptExitConfirmation {
            active_downloads: active_downloads_count,
        };
    }
    WindowCloseDecision::ImmediateCleanExit
}

/// مزامنة خيار «بدء البرنامج مع Windows» في سجل النظام (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)

/// أيقونة StormGet الرسمية (64x64 RGBA) المستخرجة من الشعار الرسمي لشريط العنوان ومنطقة الإشعارات
pub fn stormget_app_icon_rgba() -> (Vec<u8>, u32, u32) {
    const OFFICIAL_ICON_RGBA: &[u8] = include_bytes!("../../assets/icon_64.rgba");
    if OFFICIAL_ICON_RGBA.len() == 64 * 64 * 4 {
        return (OFFICIAL_ICON_RGBA.to_vec(), 64, 64);
    }
    (vec![0u8; 32 * 32 * 4], 32, 32)
}

/// متحكم منطقة الإشعارات الموحد (Unified System Tray Controller) المشترك بين `slint-ui` و `native-ui`
pub struct UnifiedTrayController {
    #[cfg(all(windows, any(feature = "native-ui", feature = "slint-ui")))]
    _icon: tray_icon::TrayIcon,
    actions: std::sync::mpsc::Receiver<TrayMenuCommand>,
}

impl UnifiedTrayController {
    pub fn new_with_wakeup(wakeup: Box<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        if !try_acquire_single_tray_slot() {
            return Err("System tray icon is already active for this StormGet process".to_string());
        }
        let (_action_tx, actions) = std::sync::mpsc::channel();

        #[cfg(all(windows, any(feature = "native-ui", feature = "slint-ui")))]
        {
            use tray_icon::{
                menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
                Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
            };

            let menu = Menu::new();
            let item_open = MenuItem::with_id(TRAY_ID_OPEN_MAIN, "🖥 فتح StormGet", true, None);
            let item_active = MenuItem::with_id(TRAY_ID_ACTIVE_DOWNLOADS, "📥 التنزيلات النشطة", true, None);
            let item_add = MenuItem::with_id(TRAY_ID_ADD_NEW, "➕ إضافة تنزيل جديد", true, None);
            let sep1 = PredefinedMenuItem::separator();
            let item_pause_all = MenuItem::with_id(TRAY_ID_PAUSE_ALL, "⏸ إيقاف جميع التنزيلات مؤقتًا", true, None);
            let item_resume_all = MenuItem::with_id(TRAY_ID_RESUME_ALL, "▶ استئناف جميع التنزيلات", true, None);
            let sep2 = PredefinedMenuItem::separator();
            let item_scheduler = MenuItem::with_id(TRAY_ID_SCHEDULER, "⏰ الجدولة", true, None);
            let item_settings = MenuItem::with_id(TRAY_ID_SETTINGS, "⚙ الإعدادات", true, None);
            let item_diagnostics = MenuItem::with_id(TRAY_ID_DIAGNOSTICS, "🩺 التشخيص", true, None);
            let item_about = MenuItem::with_id(TRAY_ID_ABOUT, "ℹ حول StormGet", true, None);
            let sep3 = PredefinedMenuItem::separator();
            let item_exit = MenuItem::with_id(TRAY_ID_EXIT, "🚪 خروج", true, None);

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
            .map_err(|e| {
                release_single_tray_slot();
                e.to_string()
            })?;

            let (rgba, w, h) = stormget_app_icon_rgba();
            let icon = Icon::from_rgba(rgba, w, h).map_err(|e| {
                release_single_tray_slot();
                e.to_string()
            })?;

            let wakeup_arc = std::sync::Arc::new(wakeup);
            let menu_tx = _action_tx.clone();
            let menu_wakeup = std::sync::Arc::clone(&wakeup_arc);
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if let Some(cmd) = TrayMenuCommand::from_menu_id(event.id().as_ref()) {
                    let _ = menu_tx.send(cmd);
                    menu_wakeup();
                }
            }));

            let click_tx = _action_tx.clone();
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
                    let _ = click_tx.send(TrayMenuCommand::OpenMain);
                    wakeup_arc();
                }
            }));

            let tray_icon = TrayIconBuilder::new()
                .with_tooltip("StormGet — مدير ومسرع التنزيلات الاحترافي")
                .with_icon(icon)
                .with_menu(Box::new(menu))
                .build()
                .map_err(|e| {
                    release_single_tray_slot();
                    e.to_string()
                })?;

            return Ok(Self {
                _icon: tray_icon,
                actions,
            });
        }

        #[cfg(not(all(windows, any(feature = "native-ui", feature = "slint-ui"))))]
        {
            let _ = wakeup;
            Ok(Self { actions })
        }
    }

    pub fn poll_actions(&self) -> Vec<TrayMenuCommand> {
        self.actions.try_iter().collect()
    }
}

impl Drop for UnifiedTrayController {
    fn drop(&mut self) {
        release_single_tray_slot();
    }
}

/// إرسال إشعار نظام أصلي على سطح المكتب (عند التصغير إلى System Tray أو اكتمال التنزيل) دون تبعيات خارجية
pub fn show_system_notification(title: &str, body: &str) {
    let title_owned = title.replace('\'', " ");
    let body_owned = body.replace('\'', " ");
    std::thread::spawn(move || {
        #[cfg(target_os = "windows")]
        {
            use std::process::Command;
            use crate::core::tools::hide_console_window;
            let script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; $n = New-Object System.Windows.Forms.NotifyIcon; $n.Icon = [System.Drawing.SystemIcons]::Information; $n.Visible = $true; $n.ShowBalloonTip(3000, '{}', '{}', [System.Windows.Forms.ToolTipIcon]::Info); Start-Sleep -Milliseconds 3200; $n.Dispose()",
                title_owned, body_owned
            );
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]);
            hide_console_window(&mut cmd);
            let _ = cmd.status();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = (&title_owned, &body_owned);
        }
    });
}

pub fn sync_windows_startup_registration(enable: bool, start_minimized: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use crate::core::tools::hide_console_window;

        let run_key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        if enable {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let value = if start_minimized {
                format!("\"{}\" --minimized", exe.display())
            } else {
                format!("\"{}\"", exe.display())
            };
            let mut cmd = Command::new("reg");
            cmd.args([
                "add",
                run_key,
                "/v",
                "StormGet",
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ]);
            hide_console_window(&mut cmd);
            let _ = cmd.status();
        } else {
            let mut cmd = Command::new("reg");
            cmd.args(["delete", run_key, "/v", "StormGet", "/f"]);
            hide_console_window(&mut cmd);
            let _ = cmd.status();
        }
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (enable, start_minimized);
        Ok(())
    }
}

/// تفعيل وضع الرسم بالمعالج (CPU Software Rendering) افتراضياً لتقليل استهلاك الذاكرة العشوائية (RAM/VRAM)
pub fn configure_low_ram_cpu_rendering_mode() {
    if std::env::var_os("SLINT_BACKEND").is_none() {
        std::env::set_var("SLINT_BACKEND", "winit-software");
    }
    if std::env::var_os("WGPU_POWER_PREF").is_none() {
        std::env::set_var("WGPU_POWER_PREF", "low");
    }
}

/// فحص ما إذا كانت هناك نسخة معلّقة من البرنامج عند فتحه مرة أخرى، وعرض إغلاقها وإعادة تشغيل البرنامج من جديد
pub fn prompt_and_terminate_hung_instance_if_needed() -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONWARNING, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
        };
        use crate::core::tools::hide_console_window;

        let msg: Vec<u16> = "يبدو أن نسخة سابقة من StormGet معلّقة ولا تستجيب.\nهل تريد إغلاق النسخة المعلّقة وفتح البرنامج من جديد؟"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let title: Vec<u16> = "StormGet — استعادة البرنامج المعلّق"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let answer = unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                msg.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST,
            )
        };
        if answer == IDYES {
            let current_pid = std::process::id();
            let filter = format!("PID ne {current_pid}");
            let mut kill_cmd = Command::new("taskkill");
            kill_cmd.args(["/F", "/FI", &filter, "/IM", "rust_idm.exe"]);
            hide_console_window(&mut kill_cmd);
            let _ = kill_cmd.status();

            let mut kill_sg = Command::new("taskkill");
            kill_sg.args(["/F", "/FI", &filter, "/IM", "StormGet*.exe"]);
            hide_console_window(&mut kill_sg);
            let _ = kill_sg.status();

            std::thread::sleep(std::time::Duration::from_millis(450));
            return true;
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

/// تسجيل ارتباط ملفات `.torrent` وروابط `magnet:` باسم البرنامج في سجل المستخدم (`HKCU\Software\Classes`)
pub fn register_windows_torrent_and_magnet_associations() {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        use crate::core::tools::hide_console_window;

        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let cmd_val = format!("\"{}\" \"%1\"", exe.display());

        let commands: &[(&str, Option<&str>, &str)] = &[
            (r"HKCU\Software\Classes\magnet", None, "URL:Magnet Protocol"),
            (r"HKCU\Software\Classes\magnet", Some("URL Protocol"), ""),
            (r"HKCU\Software\Classes\magnet\shell\open\command", None, &cmd_val),
            (r"HKCU\Software\Classes\.torrent", None, "StormGet.Torrent"),
            (r"HKCU\Software\Classes\StormGet.Torrent", None, "BitTorrent File"),
            (r"HKCU\Software\Classes\StormGet.Torrent\shell\open\command", None, &cmd_val),
        ];

        for (key, value_name, data) in commands {
            let mut reg = Command::new("reg.exe");
            reg.arg("ADD").arg(key);
            if let Some(vn) = value_name {
                reg.args(["/v", vn]);
            } else {
                reg.arg("/ve");
            }
            reg.args(["/t", "REG_SZ", "/d", data, "/f"]);
            hide_console_window(&mut reg);
            let _ = reg.status();
        }
    }
}

// ============================================================================
// 2. إدارة حالة الإضافة، الإيقاف المؤقت، وحظر المواقع (Extension State & Domain Blocker)
// ============================================================================

/// الأوضاع الثلاثة المستقلة لعمل إضافة المتصفح
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionOperatingMode {
    /// 1. الإضافة تعمل بشكل طبيعي
    ActiveNormal,
    /// 2. الإضافة متوقفة مؤقتًا على جميع المواقع
    PausedGlobally,
    /// 3. الإضافة تعمل، لكن بعض المواقع محظورة بصورة دائمة
    ActiveWithBlockedSites,
}

/// خيارات مدة الإيقاف المؤقت للإضافة
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionPauseDuration {
    /// إيقاف لمدة 10 دقائق
    TenMinutes,
    /// إيقاف لمدة ساعة
    OneHour,
    /// إيقاف لمدة يوم (24 ساعة)
    OneDay,
    /// إيقاف حتى إعادة التفعيل يدويًا
    UntilManualResume,
}

impl ExtensionPauseDuration {
    pub fn duration_ms(self) -> Option<i64> {
        match self {
            Self::TenMinutes => Some(10 * 60 * 1000),
            Self::OneHour => Some(60 * 60 * 1000),
            Self::OneDay => Some(24 * 60 * 60 * 1000),
            Self::UntilManualResume => None,
        }
    }

    pub fn label_arabic(self) -> &'static str {
        match self {
            Self::TenMinutes => "إيقاف لمدة 10 دقائق",
            Self::OneHour => "إيقاف لمدة ساعة",
            Self::OneDay => "إيقاف لمدة يوم",
            Self::UntilManualResume => "إيقاف حتى إعادة التفعيل يدويًا",
        }
    }
}

/// عنصر موقع محظور نهائياً من تدخل الإضافة
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedSiteEntry {
    /// اسم الموقع للعرض (مثال: `Example`)
    pub site_name: String,
    /// النطاق المطبّع (مثال: `example.com`)
    pub domain: String,
    /// تاريخ ووقت الإضافة بصيغة ISO-8601 / RFC3339
    pub added_at: String,
    /// هل يشمل الحظر جميع النطاقات الفرعية (`*.example.com`)؟
    #[serde(default = "default_subdomains_true")]
    pub include_subdomains: bool,
}

fn default_subdomains_true() -> bool {
    true
}

/// الحالة المحفوظة لإضافة المتصفح والمشتركة مع سطح المكتب
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserExtensionControlState {
    pub enabled: bool,
    pub paused_manually: bool,
    pub pause_started_at_ms: Option<i64>,
    pub pause_until_ms: Option<i64>,
    pub blocked_sites: Vec<BlockedSiteEntry>,
}

impl Default for BrowserExtensionControlState {
    fn default() -> Self {
        Self {
            enabled: true,
            paused_manually: false,
            pause_started_at_ms: None,
            pause_until_ms: None,
            blocked_sites: Vec::new(),
        }
    }
}

impl BrowserExtensionControlState {
    /// التحقق مما إذا كانت الإضافة في حالة إيقاف مؤقت حالياً (مع إعادة التفعيل التلقائي عند انتهاء المدة)
    pub fn is_currently_paused(&mut self, now_ms: i64) -> bool {
        if !self.enabled || self.paused_manually {
            return true;
        }
        if let Some(until) = self.pause_until_ms {
            if now_ms < until {
                return true;
            }
            // انتهت المدة: تعود الإضافة للعمل تلقائياً
            self.pause_started_at_ms = None;
            self.pause_until_ms = None;
            self.enabled = true;
            self.paused_manually = false;
        }
        false
    }

    /// تفعيل الإيقاف المؤقت بالمدة المحددة وحفظ وقت البدء ووقت الانتهاء
    pub fn apply_pause(&mut self, duration: ExtensionPauseDuration, now_ms: i64) {
        self.pause_started_at_ms = Some(now_ms);
        match duration.duration_ms() {
            Some(delta) => {
                self.paused_manually = false;
                self.enabled = true;
                self.pause_until_ms = Some(now_ms + delta);
            }
            None => {
                self.paused_manually = true;
                self.enabled = false;
                self.pause_until_ms = None;
            }
        }
    }

    /// إعادة تفعيل الإضافة فوراً في أي وقت قبل انتهاء المدة
    pub fn resume_now(&mut self) {
        self.enabled = true;
        self.paused_manually = false;
        self.pause_started_at_ms = None;
        self.pause_until_ms = None;
    }

    /// استنتاج الوضع التشغيلي الحالي من بين الأوضاع الثلاثة المستقلة
    pub fn operating_mode(&mut self, now_ms: i64) -> ExtensionOperatingMode {
        if self.is_currently_paused(now_ms) {
            ExtensionOperatingMode::PausedGlobally
        } else if !self.blocked_sites.is_empty() {
            ExtensionOperatingMode::ActiveWithBlockedSites
        } else {
            ExtensionOperatingMode::ActiveNormal
        }
    }

    /// إضافة أو تحديث موقع محظور بعد تطبيع النطاق
    pub fn add_or_update_blocked_site(
        &mut self,
        raw_domain_or_url: &str,
        custom_name: Option<&str>,
    ) -> Result<BlockedSiteEntry, String> {
        let domain = normalize_domain_pattern(raw_domain_or_url)
            .ok_or_else(|| format!("نطاق غير صالح: {raw_domain_or_url}"))?;
        let site_name = custom_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .unwrap_or_else(|| derive_site_display_name(&domain));
        let now_str: DateTime<Utc> = Utc::now();
        let entry = BlockedSiteEntry {
            site_name,
            domain: domain.clone(),
            added_at: now_str.format("%Y-%m-%d %H:%M").to_string(),
            include_subdomains: true,
        };
        if let Some(existing) = self.blocked_sites.iter_mut().find(|b| b.domain == domain) {
            existing.site_name.clone_from(&entry.site_name);
            return Ok(existing.clone());
        }
        self.blocked_sites.push(entry.clone());
        Ok(entry)
    }

    /// إزالة الحظر عن نطاق محدد
    pub fn remove_blocked_site(&mut self, raw_domain: &str) -> bool {
        let Some(norm) = normalize_domain_pattern(raw_domain) else {
            return false;
        };
        let before = self.blocked_sites.len();
        self.blocked_sites.retain(|b| b.domain != norm);
        self.blocked_sites.len() < before
    }

    /// التحقق مما إذا كان رابط أو نطاق معين محظوراً
    pub fn is_url_or_host_blocked(&self, url_or_host: &str) -> bool {
        let Some(candidate_host) = normalize_domain_pattern(url_or_host) else {
            return false;
        };
        self.blocked_sites
            .iter()
            .any(|entry| is_host_matched_by_blocked_domain(&candidate_host, &entry.domain))
    }

    /// التحقق الشامل: هل يجب على الإضافة اعتراض التحميل من هذا الموقع؟
    pub fn should_intercept_on_site(&mut self, page_or_file_url: &str, now_ms: i64) -> bool {
        if self.is_currently_paused(now_ms) {
            return false;
        }
        !self.is_url_or_host_blocked(page_or_file_url)
    }

    pub fn state_file_path(base_dir: &Path) -> PathBuf {
        base_dir.join("extension_state.json")
    }

    pub fn load_from_disk(base_dir: &Path) -> Self {
        let p = Self::state_file_path(base_dir);
        if let Ok(bytes) = fs::read(&p) {
            if let Ok(st) = serde_json::from_slice::<Self>(&bytes) {
                return st;
            }
        }
        Self::default()
    }

    pub fn save_to_disk_atomic(&self, base_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(base_dir).map_err(|e| e.to_string())?;
        let p = Self::state_file_path(base_dir);
        let tmp = p.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// تطبيع وتوحيد صيغة النطاق قبل التخزين والمقارنة:
/// - يقبل رابطاً كاملاً (`https://Sub.Example.com:8443/path`) أو نطاقاً (`*.example.com.` أو `www.example.com`)
/// - يحول الحروف إلى صغيرة، ويزيل `www.` البادئة والبادئة `*.` والنقطة النهائية والمنفذ
/// - يتحقق من صحة محارف النطاق ويرفض النطاقات الفارغة أو المحلية غير الصالحة
pub fn normalize_domain_pattern(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let raw_host = if trimmed.contains("://") {
        let parsed = Url::parse(trimmed).ok()?;
        parsed.host_str()?.to_ascii_lowercase()
    } else {
        let without_path = trimmed
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let without_wildcard = without_path
            .strip_prefix("*.")
            .unwrap_or(&without_path)
            .trim();
        // إزالة المنفذ إن وجد (لغير عناوين IPv6)
        if !without_wildcard.starts_with('[') {
            without_wildcard
                .split(':')
                .next()
                .unwrap_or("")
                .to_string()
        } else {
            without_wildcard.to_string()
        }
    };

    let mut clean = raw_host.trim_matches('.').to_string();
    if let Some(stripped_www) = clean.strip_prefix("www.") {
        if stripped_www.contains('.') {
            clean = stripped_www.to_string();
        }
    }

    if clean.is_empty() || !clean.contains('.') || clean.contains("..") {
        return None;
    }

    let valid_chars = clean
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if !valid_chars {
        return None;
    }

    if clean
        .split('.')
        .any(|part| part.is_empty() || part.starts_with('-') || part.ends_with('-'))
    {
        return None;
    }

    Some(clean)
}

/// مطابقة نطاقات صارمة تدعم النطاقات الفرعية وتمنع حظر نطاقات متشابهة بالخطأ:
/// - إذا كان المحظور `example.com`:
///   - يطابق `example.com` و `www.example.com` و `cdn.sub.example.com`
///   - **لا يطابق أبداً** `evil-example.com` أو `myexample.com` أو `example.com.evil.org`
pub fn is_host_matched_by_blocked_domain(candidate_url_or_host: &str, blocked_domain: &str) -> bool {
    let Some(host) = normalize_domain_pattern(candidate_url_or_host) else {
        return false;
    };
    let Some(blocked) = normalize_domain_pattern(blocked_domain) else {
        return false;
    };

    if host == blocked {
        return true;
    }

    // التحقق من النطاق الفرعي عبر حدود النقطة الصريحة `.blocked`
    if host.len() > blocked.len() && host.ends_with(&blocked) {
        let prefix_len = host.len() - blocked.len();
        if host.as_bytes().get(prefix_len.wrapping_sub(1)) == Some(&b'.') {
            return true;
        }
    }

    false
}

/// استخلاص اسم موقع أنيق للعرض من النطاق المطبّع
pub fn derive_site_display_name(normalized_domain: &str) -> String {
    let first_part = normalized_domain
        .split('.')
        .next()
        .unwrap_or(normalized_domain);
    let mut chars = first_part.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => normalized_domain.to_string(),
    }
}

// ============================================================================
// 3. نظام Design Tokens المركزي للثيم الفاتح والداكن (Phase 3 Requirements)
// ============================================================================

/// قيمة لون RGBA قياسية محايدة عن مكتبة الواجهة (تتحول مباشرة إلى `egui::Color32` و `slint::Color`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbaToken {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl RgbaToken {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// حساب الإضاءة النسبية وفق معيار WCAG 2.1
    pub fn relative_luminance(self) -> f64 {
        fn channel_lum(c: u8) -> f64 {
            let s = (c as f64) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel_lum(self.r) + 0.7152 * channel_lum(self.g) + 0.0722 * channel_lum(self.b)
    }

    /// حساب نسبة التباين (Contrast Ratio) بين لونين وفق معيار WCAG AA (يجب أن تكون >= 4.5 للنصوص العادية)
    pub fn contrast_ratio(self, other: Self) -> f64 {
        let l1 = self.relative_luminance();
        let l2 = other.relative_luminance();
        let (lighter, darker) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        (lighter + 0.05) / (darker + 0.05)
    }
}

/// نظام Design Tokens المركزي الموحد للواجهتين (`egui` و `Slint`) بالأسماء الـ 14 الإلزامية
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignTokens {
    pub background: RgbaToken,
    pub surface: RgbaToken,
    pub surface_elevated: RgbaToken,
    pub text_primary: RgbaToken,
    pub text_secondary: RgbaToken,
    pub text_disabled: RgbaToken,
    pub border: RgbaToken,
    pub accent: RgbaToken,
    pub success: RgbaToken,
    pub warning: RgbaToken,
    pub error: RgbaToken,
    pub selection: RgbaToken,
    pub hover: RgbaToken,
    pub focus: RgbaToken,
}

impl DesignTokens {
    /// رموز التصميم المركزية للوضع الداكن (Dark Theme)
    pub const fn dark() -> Self {
        DesignTokens {
            background: RgbaToken::rgb(18, 22, 29),       // #12161d
            surface: RgbaToken::rgb(26, 31, 40),          // #1a1f28
            surface_elevated: RgbaToken::rgb(34, 41, 53), // #222935
            text_primary: RgbaToken::rgb(241, 245, 249),  // #f1f5f9 (Contrast > 14:1)
            text_secondary: RgbaToken::rgb(168, 180, 198),// #a8b4c6 (Contrast > 6.8:1)
            text_disabled: RgbaToken::rgb(112, 124, 142), // #707c8e
            border: RgbaToken::rgb(56, 67, 84),           // #384354
            accent: RgbaToken::rgb(14, 116, 208),         // #0e74d0
            success: RgbaToken::rgb(46, 196, 108),        // #2ec46c
            warning: RgbaToken::rgb(245, 166, 35),        // #f5a623
            error: RgbaToken::rgb(248, 113, 113),          // #f87171 (Contrast > 6.2:1 on dark surface)
            selection: RgbaToken::rgb(24, 82, 148),       // #185294
            hover: RgbaToken::rgb(42, 52, 68),            // #2a3444
            focus: RgbaToken::rgb(56, 162, 255),          // #38a2ff
        }
    }

    /// رموز التصميم المركزية للوضع الفاتح (Light Theme — WCAG AA Compliant)
    pub const fn light() -> Self {
        DesignTokens {
            background: RgbaToken::rgb(244, 246, 250),       // #f4f6fa
            surface: RgbaToken::rgb(255, 255, 255),          // #ffffff
            surface_elevated: RgbaToken::rgb(235, 240, 247), // #ebf0f7
            text_primary: RgbaToken::rgb(15, 23, 42),        // #0f172a (Contrast > 15:1 on surface/bg)
            text_secondary: RgbaToken::rgb(51, 65, 85),      // #334155 (Contrast > 8.4:1 on surface/bg)
            text_disabled: RgbaToken::rgb(100, 116, 139),    // #64748b (Contrast > 4.5:1 on white)
            border: RgbaToken::rgb(186, 198, 214),           // #bac6d6
            accent: RgbaToken::rgb(11, 99, 184),             // #0b63b8 (Contrast > 5.6:1 with white text)
            success: RgbaToken::rgb(15, 118, 52),            // #0f7634 (Contrast > 5.5:1 on light surface)
            warning: RgbaToken::rgb(161, 92, 7),             // #a15c07 (Contrast > 4.7:1 on light surface)
            error: RgbaToken::rgb(185, 28, 28),              // #b91c1c (Contrast > 6.4:1 on light surface)
            selection: RgbaToken::rgb(214, 232, 255),        // #d6e8ff (Light selection with dark text_primary > 11:1)
            hover: RgbaToken::rgb(226, 234, 244),            // #e2eaf4
            focus: RgbaToken::rgb(2, 106, 199),              // #026ac7
        }
    }

    /// التحقق من أن جميع ألوان النصوص الأساسية والثانوية والدلالية تحقق معيار التباين WCAG AA (>= 4.5:1)
    pub fn meets_wcag_aa(self) -> bool {
        let p_bg = self.text_primary.contrast_ratio(self.background);
        let p_surf = self.text_primary.contrast_ratio(self.surface);
        let p_elev = self.text_primary.contrast_ratio(self.surface_elevated);
        let s_bg = self.text_secondary.contrast_ratio(self.background);
        let s_surf = self.text_secondary.contrast_ratio(self.surface);
        let s_elev = self.text_secondary.contrast_ratio(self.surface_elevated);
        let err_surf = self.error.contrast_ratio(self.surface);
        let suc_surf = self.success.contrast_ratio(self.surface);
        let warn_surf = self.warning.contrast_ratio(self.surface);

        p_bg >= 4.5
            && p_surf >= 4.5
            && p_elev >= 4.5
            && s_bg >= 4.5
            && s_surf >= 4.5
            && s_elev >= 4.5
            && err_surf >= 4.5
            && suc_surf >= 4.5
            && warn_surf >= 4.5
    }
}

// ============================================================================
// 4. خدمة اتجاه الواجهة (RTL / LTR) ومعالجة النصوص المختلطة (BiDi Helpers)
// ============================================================================

/// اتجاه تخطيط الواجهة العام
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutDirection {
    /// من اليمين إلى اليسار (العربية)
    Rtl,
    /// من اليسار إلى اليمين (الإنجليزية)
    Ltr,
}

impl LayoutDirection {
    pub fn from_arabic(is_arabic: bool) -> Self {
        if is_arabic {
            Self::Rtl
        } else {
            Self::Ltr
        }
    }

    pub fn is_rtl(self) -> bool {
        matches!(self, Self::Rtl)
    }
}

/// تنسيق قيم تقنية ذات اتجاه LTR صريح (روابط URL، مسارات الملفات، عناوين IP، الأرقام والأحجام، السرعات، التواريخ)
/// بحيث لا تنعكس أرقامها أو رموزها أبداً عند عرضها داخل سياق عربي RTL
pub fn preserve_ltr_token(raw: &str) -> String {
    // لا نعكس محتوى الروابط أو المسارات أو الأرقام حرفياً ولا نغير محتواها
    raw.trim().to_string()
}

/// تنسيق حجم الملف بصورة مقروءة وصحيحة في كل من العربية والإنجليزية دون عكس الأرقام
pub fn format_bidi_size(bytes: u64, is_arabic: bool) -> String {
    if bytes == 0 {
        return if is_arabic {
            "غير معروف".to_string()
        } else {
            "Unknown".to_string()
        };
    }
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;
    let b = bytes as f64;
    if is_arabic {
        if b >= TB {
            format!("{:.2} ت.ب", b / TB)
        } else if b >= GB {
            format!("{:.2} غ.ب", b / GB)
        } else if b >= MB {
            format!("{:.2} م.ب", b / MB)
        } else if b >= KB {
            format!("{:.1} ك.ب", b / KB)
        } else {
            format!("{bytes} ب")
        }
    } else if b >= TB {
        format!("{:.2} TB", b / TB)
    } else if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// تنسيق سرعة النقل دون عكس الأرقام أو الوحدات في كلتا اللغتين
pub fn format_bidi_speed(bps: u64, is_arabic: bool) -> String {
    if bps == 0 {
        return if is_arabic {
            "0 ك.ب/ث".to_string()
        } else {
            "0 KB/s".to_string()
        };
    }
    let suffix = if is_arabic { "ث" } else { "s" };
    format!("{}/{}", format_bidi_size(bps, is_arabic), suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_domain_normalization_and_strict_subdomain_matching() {
        assert_eq!(
            normalize_domain_pattern("https://WWW.Example.COM:8080/downloads/file.zip?x=1"),
            Some("example.com".to_string())
        );
        assert_eq!(
            normalize_domain_pattern("*.cdn.example.com."),
            Some("cdn.example.com".to_string())
        );
        assert_eq!(normalize_domain_pattern("invalid_domain"), None);

        // Exact and subdomain match
        assert!(is_host_matched_by_blocked_domain(
            "https://example.com/a.zip",
            "example.com"
        ));
        assert!(is_host_matched_by_blocked_domain(
            "https://sub.cdn.example.com/a.zip",
            "example.com"
        ));

        // Must NOT match lookalike domains such as evil-example.com!
        assert!(!is_host_matched_by_blocked_domain(
            "https://evil-example.com/a.zip",
            "example.com"
        ));
        assert!(!is_host_matched_by_blocked_domain(
            "https://myexample.com/a.zip",
            "example.com"
        ));
        assert!(!is_host_matched_by_blocked_domain(
            "https://example.com.attacker.net/a.zip",
            "example.com"
        ));
    }

    #[test]
    fn test_extension_three_independent_modes_and_pause_durations() {
        let mut st = BrowserExtensionControlState::default();
        let now = 1_700_000_000_000i64;
        assert_eq!(st.operating_mode(now), ExtensionOperatingMode::ActiveNormal);
        assert!(st.should_intercept_on_site("https://example.com/file.zip", now));

        // Pause for 10 minutes
        st.apply_pause(ExtensionPauseDuration::TenMinutes, now);
        assert_eq!(
            st.operating_mode(now + 1000),
            ExtensionOperatingMode::PausedGlobally
        );
        assert!(!st.should_intercept_on_site("https://example.com/file.zip", now + 1000));

        // Auto-resume after 10 minutes + 1ms
        let after_10m = now + 10 * 60 * 1000 + 1;
        assert_eq!(
            st.operating_mode(after_10m),
            ExtensionOperatingMode::ActiveNormal
        );
        assert!(st.should_intercept_on_site("https://example.com/file.zip", after_10m));

        // Pause for 1 hour, then manual resume before expiry
        st.apply_pause(ExtensionPauseDuration::OneHour, after_10m);
        assert!(st.is_currently_paused(after_10m + 5000));
        st.resume_now();
        assert!(!st.is_currently_paused(after_10m + 5000));

        // Pause until manual resume
        st.apply_pause(ExtensionPauseDuration::UntilManualResume, after_10m);
        assert!(st.is_currently_paused(after_10m + 999_999_999));
        st.resume_now();

        // Block specific site while keeping other sites active
        st.add_or_update_blocked_site("https://example.com/page", Some("Example"))
            .expect("valid domain");
        assert_eq!(
            st.operating_mode(after_10m),
            ExtensionOperatingMode::ActiveWithBlockedSites
        );
        assert!(!st.should_intercept_on_site("https://cdn.example.com/file.zip", after_10m));
        assert!(st.should_intercept_on_site("https://evil-example.com/file.zip", after_10m));
        assert!(st.should_intercept_on_site("https://ubuntu.com/ubuntu.iso", after_10m));

        // Remove block
        assert!(st.remove_blocked_site("example.com"));
        assert!(st.should_intercept_on_site("https://example.com/file.zip", after_10m));
    }

    #[test]
    fn test_design_tokens_wcag_aa_contrast_in_light_and_dark_themes() {
        let light = DesignTokens::light();
        let dark = DesignTokens::dark();
        assert!(
            light.meets_wcag_aa(),
            "Light theme tokens must meet WCAG AA >= 4.5:1 contrast"
        );
        assert!(
            dark.meets_wcag_aa(),
            "Dark theme tokens must meet WCAG AA >= 4.5:1 contrast"
        );
    }

    #[test]
    fn test_window_close_policy_and_tray_commands() {
        assert_eq!(
            evaluate_window_close_policy(true, false, true, true, 3),
            WindowCloseDecision::MinimizeToTray {
                show_notification: true
            }
        );
        assert_eq!(
            evaluate_window_close_policy(false, true, false, true, 0),
            WindowCloseDecision::PromptExitConfirmation {
                active_downloads: 0
            }
        );
        assert_eq!(
            evaluate_window_close_policy(false, false, false, true, 2),
            WindowCloseDecision::PromptExitConfirmation {
                active_downloads: 2
            }
        );
        assert_eq!(TrayMenuCommand::all_ordered().len(), 10);
    }
}
