//! إدارة حالة النوافذ، استعادتها بأمان متعدد الشاشات، ومنع فقدان النافذة خارج الشاشة (Multi-Monitor Safe Window State Persistence)
//!
//! يطبق هذا المكون متطلبات المرحلة 11 والمرحلة 12 بالكامل:
//! 1. حفظ واستعادة حجم وموضع النافذة الرئيسية وحالة التكبير (`Maximized`) بشكل منفصل عن الحجم الطبيعي (`normal_width`, `normal_height`, `normal_x`, `normal_y`).
//! 2. عدم استعادة حالة التصغير (`Minimized`) تلقائياً عند بدء التشغيل حتى لا يظن المستخدم أن البرنامج لم يعمل.
//! 3. التحقق الصارم من حدود الشاشات المتاحة (`MonitorWorkArea`) ونقل النافذة تلقائياً إلى الشاشة المتاحة إذا فُصلت الشاشة الثانوية أو تغيرت دقة العرض.
//! 4. حفظ تفضيلات العرض: المظهر (`ThemeMode`)، اللغة (`AppLanguage`)، كثافة الصفوف (`RowDensity`)، حالة اللوحات الجانبية وشريط الأدوات وشريط الحالة، والأعمدة الظاهرة وترتيبها وعرضها.
//! 5. حفظ حالة كل نافذة ثانوية مستقلة (الحجم، الموضع، آخر تبويب مفتوح) بصورة مستقلة عن النافذة الرئيسية.

use super::types::{AppLanguage, BottomInspectorTab, OptionsTab, RowDensity, SortColumn, TableColumnId, ThemeMode};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// تمثيل مساحة العمل المرئية لشاشة متصلة (مع مراعاة شريط مهام Windows)
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MonitorWorkArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub is_primary: bool,
}

impl MonitorWorkArea {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// هندسة وموضع النافذة المحفوظة (مع فصل الحجم الطبيعي عن حالة التكبير)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedWindowGeometry {
    pub normal_width: f32,
    pub normal_height: f32,
    pub normal_x: Option<f32>,
    pub normal_y: Option<f32>,
    pub maximized: bool,
    pub was_minimized_on_close: bool,
    pub last_open_tab: String,
}

impl SavedWindowGeometry {
    pub fn new_default(width: f32, height: f32) -> Self {
        Self {
            normal_width: width,
            normal_height: height,
            normal_x: None,
            normal_y: None,
            maximized: false,
            was_minimized_on_close: false,
            last_open_tab: String::new(),
        }
    }

    /// تحديث الأبعاد الطبيعية فقط عندما لا تكون النافذة مكبرة أو مصغرة
    /// (قاعدة إلزامية: لا تستبدل الحجم العادي المحفوظ بأبعاد النافذة المكبّرة)
    pub fn update_from_live_viewport(
        &mut self,
        current_x: Option<f32>,
        current_y: Option<f32>,
        current_w: f32,
        current_h: f32,
        is_maximized: bool,
        is_minimized: bool,
    ) {
        self.was_minimized_on_close = is_minimized;
        if is_minimized {
            // لا نغير الحجم أو حالة التكبير أثناء كون النافذة مصغرة في شريط المهام
            return;
        }
        self.maximized = is_maximized;
        if !is_maximized && current_w >= 260.0 && current_h >= 160.0 {
            self.normal_width = current_w;
            self.normal_height = current_h;
            if let (Some(x), Some(y)) = (current_x, current_y) {
                if x > -15000.0 && y > -15000.0 && x < 30000.0 && y < 30000.0 {
                    self.normal_x = Some(x);
                    self.normal_y = Some(y);
                }
            }
        }
    }
}

/// حالة الواجهة الشاملة المحفوظة في `ui_state.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedUiState {
    pub version: u32,
    pub main_window: SavedWindowGeometry,
    pub secondary_windows: HashMap<String, SavedWindowGeometry>,
    pub theme_mode: ThemeMode,
    pub language: AppLanguage,
    pub row_density: RowDensity,
    pub font_scale: f32,
    pub show_sidebar: bool,
    pub show_toolbar: bool,
    pub show_status_bar: bool,
    pub show_bottom_inspector: bool,
    pub bottom_inspector_tab: BottomInspectorTab,
    pub last_options_tab: OptionsTab,
    pub visible_columns: Vec<TableColumnId>,
    pub column_widths: HashMap<TableColumnId, f32>,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub sidebar_width: f32,
    pub expand_all_downloads: bool,
    pub expand_unfinished: bool,
    pub expand_finished: bool,
    pub expand_queues: bool,
}

impl Default for PersistedUiState {
    fn default() -> Self {
        let default_cols = TableColumnId::default_visible_columns();
        let mut widths = HashMap::new();
        for col in TableColumnId::all_columns() {
            widths.insert(col, col.default_width());
        }
        Self {
            version: 1,
            main_window: SavedWindowGeometry::new_default(1140.0, 680.0),
            secondary_windows: Self::default_secondary_geometries(),
            theme_mode: ThemeMode::Dark,
            language: AppLanguage::Arabic,
            row_density: RowDensity::Comfortable,
            font_scale: 1.0,
            show_sidebar: true,
            show_toolbar: true,
            show_status_bar: true,
            show_bottom_inspector: true,
            bottom_inspector_tab: BottomInspectorTab::SegmentMap,
            last_options_tab: OptionsTab::General,
            visible_columns: default_cols,
            column_widths: widths,
            sort_column: SortColumn::UpdatedAt,
            sort_ascending: false,
            sidebar_width: 196.0,
            expand_all_downloads: true,
            expand_unfinished: true,
            expand_finished: true,
            expand_queues: true,
        }
    }
}

impl PersistedUiState {
    pub fn default_secondary_geometries() -> HashMap<String, SavedWindowGeometry> {
        let mut map = HashMap::new();
        map.insert(
            "add_download".to_string(),
            SavedWindowGeometry::new_default(620.0, 440.0),
        );
        map.insert(
            "download_details".to_string(),
            SavedWindowGeometry::new_default(660.0, 560.0),
        );
        map.insert(
            "scheduler".to_string(),
            SavedWindowGeometry::new_default(760.0, 520.0),
        );
        map.insert(
            "settings".to_string(),
            SavedWindowGeometry::new_default(780.0, 560.0),
        );
        map.insert(
            "batch".to_string(),
            SavedWindowGeometry::new_default(600.0, 380.0),
        );
        map.insert(
            "grabber".to_string(),
            SavedWindowGeometry::new_default(640.0, 460.0),
        );
        map.insert(
            "zip_preview".to_string(),
            SavedWindowGeometry::new_default(680.0, 460.0),
        );
        map.insert(
            "diagnostics".to_string(),
            SavedWindowGeometry::new_default(720.0, 500.0),
        );
        map
    }

    pub fn ui_state_file_path(base_dir: &Path) -> PathBuf {
        base_dir.join("ui_state.json")
    }

    /// قراءة حالة الواجهة من القرص دون استبدال الملف المحفوظ بصمت عند وجود خطأ عابر
    pub fn load_from_disk(base_dir: &Path) -> Result<Self, String> {
        let path = Self::ui_state_file_path(base_dir);
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = fs::read(&path)
            .map_err(|e| format!("تعذر قراءة ملف حالة الواجهة {:?}: {}", path, e))?;
        let mut loaded: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("ملف حالة الواجهة غير صالح {:?}: {}", path, e))?;

        // ضمان وجود عمود واحد على الأقل ظاهر (اسم الملف والحالة)
        if loaded.visible_columns.is_empty() {
            loaded.visible_columns = TableColumnId::default_visible_columns();
        }
        loaded.font_scale = loaded.font_scale.clamp(0.85, 1.35);
        loaded.sidebar_width = loaded.sidebar_width.clamp(145.0, 360.0);
        for (k, v) in Self::default_secondary_geometries() {
            loaded.secondary_windows.entry(k).or_insert(v);
        }
        Ok(loaded)
    }

    /// حفظ ذري لحالة الواجهة على القرص
    pub fn save_to_disk_atomic(&self, base_dir: &Path) -> Result<(), String> {
        fs::create_dir_all(base_dir)
            .map_err(|e| format!("تعذر إنشاء مجلد الإعدادات {:?}: {}", base_dir, e))?;
        let path = Self::ui_state_file_path(base_dir);
        let tmp_path = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("تعذر تسلسل حالة الواجهة: {}", e))?;
        fs::write(&tmp_path, &bytes)
            .map_err(|e| format!("تعذر كتابة ملف حالة الواجهة المؤقت: {}", e))?;
        fs::rename(&tmp_path, &path)
            .map_err(|e| format!("تعذر تثبيت ملف حالة الواجهة النهائي: {}", e))?;
        Ok(())
    }

    /// استعادة التخطيط الافتراضي للواجهة والأعمدة والنوافذ
    pub fn reset_layout_to_defaults(&mut self) {
        let preserved_lang = self.language;
        let preserved_theme = self.theme_mode;
        *self = Self::default();
        self.language = preserved_lang;
        self.theme_mode = preserved_theme;
    }
}

/// نتيجة التحقق من صلاحية أبعاد وموضع النافذة مقابل الشاشات المتصلة
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedWindowPlacement {
    pub width: f32,
    pub height: f32,
    pub x: f32,
    pub y: f32,
    pub maximized: bool,
    pub relocated_to_available_monitor: bool,
}

/// التحقق الهندسي الصارم من موضع وحجم النافذة عند التشغيل أو الفتح:
/// - إذا كانت النافذة محفوظة على شاشة ثانية تم فصلها، تُنقل تلقائياً إلى وسط الشاشة الأساسية المتاحة.
/// - إذا كان الحجم المحفوظ أكبر من مساحة عمل الشاشة المتاحة، يُقلص ليلائم الشاشة.
/// - يضمن بقاء شريط عنوان النافذة ضمن المنطقة المرئية لكي يتمكن المستخدم من سحبها دائماً.
pub fn validate_and_clamp_window_geometry(
    saved: &SavedWindowGeometry,
    min_width: f32,
    min_height: f32,
    monitors: &[MonitorWorkArea],
) -> ValidatedWindowPlacement {
    let fallback_monitor = MonitorWorkArea {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
        is_primary: true,
    };

    let primary_monitor = monitors
        .iter()
        .copied()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first().copied())
        .unwrap_or(fallback_monitor);

    let raw_w = saved.normal_width.max(min_width);
    let raw_h = saved.normal_height.max(min_height);

    // البحث عن الشاشة التي تحتوي على مركز شريط عنوان النافذة المحفوظة (أو جزء كافٍ منها >= 100px)
    if let (Some(sx), Some(sy)) = (saved.normal_x, saved.normal_y) {
        let titlebar_probe_x = sx + (raw_w * 0.5).min(200.0);
        let titlebar_probe_y = sy + 16.0;

        let matching_monitor = monitors.iter().copied().find(|m| {
            let horiz_overlap = (sx + raw_w).min(m.right()) - sx.max(m.x);
            let vert_overlap = (sy + raw_h).min(m.bottom()) - sy.max(m.y);
            let title_visible = titlebar_probe_x >= m.x
                && titlebar_probe_x <= m.right()
                && titlebar_probe_y >= m.y
                && titlebar_probe_y <= m.bottom();
            title_visible || (horiz_overlap >= 120.0 && vert_overlap >= 80.0)
        });

        if let Some(mon) = matching_monitor {
            let max_w = (mon.width - 24.0).max(min_width);
            let max_h = (mon.height - 24.0).max(min_height);
            let w = raw_w.clamp(min_width, max_w);
            let h = raw_h.clamp(min_height, max_h);
            let clamped_x = sx.clamp(mon.x, (mon.right() - w).max(mon.x));
            let clamped_y = sy.clamp(mon.y, (mon.bottom() - h).max(mon.y));

            return ValidatedWindowPlacement {
                width: w,
                height: h,
                x: clamped_x,
                y: clamped_y,
                maximized: saved.maximized,
                relocated_to_available_monitor: false,
            };
        }
    }

    // الشاشة السابقة غير متصلة أو لا يوجد موضع محفوظ -> توسيط النافذة في الشاشة الأساسية
    let max_w = (primary_monitor.width - 32.0).max(min_width);
    let max_h = (primary_monitor.height - 32.0).max(min_height);
    let w = raw_w.clamp(min_width, max_w);
    let h = raw_h.clamp(min_height, max_h);
    let centered_x = primary_monitor.x + ((primary_monitor.width - w) * 0.5).max(0.0);
    let centered_y = primary_monitor.y + ((primary_monitor.height - h) * 0.5).max(0.0);

    ValidatedWindowPlacement {
        width: w,
        height: h,
        x: centered_x,
        y: centered_y,
        maximized: saved.maximized,
        relocated_to_available_monitor: saved.normal_x.is_some() && saved.normal_y.is_some(),
    }
}

/// اكتشاف الشاشات المتصلة ومساحة العمل الفعلية (مع مراعاة شريط مهام Windows)
pub fn detect_connected_monitors() -> Vec<MonitorWorkArea> {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CMONITORS, SM_CXFULLSCREEN, SM_CYFULLSCREEN,
            SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
        };
        unsafe {
            let primary_w = GetSystemMetrics(SM_CXFULLSCREEN).max(1024) as f32;
            let primary_h = GetSystemMetrics(SM_CYFULLSCREEN).max(680) as f32;
            let mut list = vec![MonitorWorkArea {
                x: 0.0,
                y: 0.0,
                width: primary_w,
                height: primary_h,
                is_primary: true,
            }];

            let count = GetSystemMetrics(SM_CMONITORS);
            if count > 1 {
                let vx = GetSystemMetrics(SM_XVIRTUALSCREEN) as f32;
                let vy = GetSystemMetrics(SM_YVIRTUALSCREEN) as f32;
                let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN) as f32;
                let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN) as f32;
                if vw > primary_w || vx < 0.0 || vy < 0.0 {
                    list.push(MonitorWorkArea {
                        x: vx,
                        y: vy,
                        width: vw,
                        height: vh,
                        is_primary: false,
                    });
                }
            }
            return list;
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![MonitorWorkArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
            is_primary: true,
        }]
    }
}

#[cfg(test)]
mod window_state_tests {
    use super::*;

    #[test]
    fn relocates_offscreen_window_from_disconnected_second_monitor() {
        let single_monitor = vec![MonitorWorkArea {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1040.0,
            is_primary: true,
        }];

        // النافذة كانت محفوظة على شاشة ثانية عند X = 2400
        let saved = SavedWindowGeometry {
            normal_width: 1100.0,
            normal_height: 680.0,
            normal_x: Some(2400.0),
            normal_y: Some(120.0),
            maximized: false,
            was_minimized_on_close: true,
            last_open_tab: String::new(),
        };

        let validated = validate_and_clamp_window_geometry(&saved, 840.0, 500.0, &single_monitor);
        assert!(validated.relocated_to_available_monitor);
        assert!(validated.x >= 0.0 && validated.x + validated.width <= 1920.0);
        assert!(validated.y >= 0.0 && validated.y + validated.height <= 1040.0);
    }

    #[test]
    fn preserves_normal_bounds_when_maximized_or_minimized() {
        let mut geom = SavedWindowGeometry::new_default(1040.0, 640.0);
        geom.update_from_live_viewport(Some(100.0), Some(80.0), 1080.0, 660.0, false, false);
        assert_eq!(geom.normal_width, 1080.0);
        assert_eq!(geom.normal_height, 660.0);
        assert_eq!(geom.normal_x, Some(100.0));

        // عند تكبير النافذة إلى 1920x1080 يجب ألا يُستبدل الحجم الطبيعي 1080x660
        geom.update_from_live_viewport(Some(0.0), Some(0.0), 1920.0, 1080.0, true, false);
        assert!(geom.maximized);
        assert_eq!(geom.normal_width, 1080.0);
        assert_eq!(geom.normal_height, 660.0);
        assert_eq!(geom.normal_x, Some(100.0));

        // عند تصغير النافذة إلى شريط المهام يجب ألا تُمسح حالة التكبير أو الحجم الطبيعي
        geom.update_from_live_viewport(Some(-32000.0), Some(-32000.0), 160.0, 28.0, false, true);
        assert!(geom.was_minimized_on_close);
        assert!(geom.maximized);
        assert_eq!(geom.normal_width, 1080.0);
    }

    #[test]
    fn clamps_oversized_window_to_monitor_work_area() {
        let small_laptop_screen = vec![MonitorWorkArea {
            x: 0.0,
            y: 0.0,
            width: 1366.0,
            height: 728.0,
            is_primary: true,
        }];
        let saved_from_4k = SavedWindowGeometry {
            normal_width: 2200.0,
            normal_height: 1400.0,
            normal_x: Some(50.0),
            normal_y: Some(50.0),
            maximized: false,
            was_minimized_on_close: false,
            last_open_tab: String::new(),
        };

        let validated = validate_and_clamp_window_geometry(&saved_from_4k, 840.0, 500.0, &small_laptop_screen);
        assert!(validated.width <= 1366.0);
        assert!(validated.height <= 728.0);
        assert!(validated.x >= 0.0);
        assert!(validated.y >= 0.0);
    }
}
