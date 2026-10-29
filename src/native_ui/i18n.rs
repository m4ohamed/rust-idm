//! محرك الترجمة المزدوج (العربية المشكّلة RTL / الإنجليزية LTR) لجميع عناصر StormGet

use super::{
    arabic::shape_arabic,
    types::{AppLanguage, OptionsTab, RowDensity, TableColumnId},
};
use crate::{DownloadStatus, FileCategory};

pub struct Loc {
    pub lang: AppLanguage,
}

#[allow(dead_code)]
impl Loc {
    pub fn new(lang: AppLanguage) -> Self {
        Self { lang }
    }

    #[inline]
    pub fn tr(&self, ar: &str, en: &'static str) -> String {
        match self.lang {
            AppLanguage::Arabic => shape_arabic(ar),
            AppLanguage::English => en.to_string(),
        }
    }

    #[inline]
    pub fn ar(&self, text: &str) -> String {
        match self.lang {
            AppLanguage::Arabic => shape_arabic(text),
            AppLanguage::English => text.to_string(),
        }
    }

    // 1. الهوية وعناوين النوافذ (Window Titles)
    pub fn app_title(&self) -> String {
        self.tr(
            "StormGet v2.3.6 — مدير التنزيلات المكتبي الاحترافي",
            "StormGet v2.3.6 — Professional Desktop Download Manager",
        )
    }
    pub fn add_download_window_title(&self) -> String {
        self.tr("StormGet — إضافة تنزيل جديد", "StormGet — Add New Download")
    }
    pub fn download_details_title(&self) -> String {
        self.tr("تفاصيل التنزيل", "Download Details")
    }
    pub fn scheduler_window_title(&self) -> String {
        self.tr(
            "StormGet — إدارة الجدولة والطوابير",
            "StormGet — Scheduler & Queue Manager",
        )
    }
    pub fn settings_window_title(&self) -> String {
        self.tr("StormGet — الإعدادات والتفضيلات", "StormGet — Settings & Preferences")
    }
    pub fn diagnostics_window_title(&self) -> String {
        self.tr(
            "StormGet — سجل الأخطاء ومعلومات التشخيص",
            "StormGet — Diagnostics & Activity Log",
        )
    }

    // 2. القوائم الرئيسية الـ 5 (Main Menu Bar: File, Downloads, View, Tools, Help)
    pub fn menu_file(&self) -> String {
        self.tr("ملف (File)", "File")
    }
    pub fn menu_downloads(&self) -> String {
        self.tr("التنزيلات (Downloads)", "Downloads")
    }
    pub fn menu_view(&self) -> String {
        self.tr("عرض (View)", "View")
    }
    pub fn menu_tools(&self) -> String {
        self.tr("أدوات (Tools)", "Tools")
    }
    pub fn menu_help(&self) -> String {
        self.tr("مساعدة (Help)", "Help")
    }
    pub fn menu_tasks(&self) -> String {
        self.tr("المهام", "Tasks")
    }

    // 3. أزرار شريط الأدوات الرئيسي (Main Toolbar Buttons & Tooltips)
    pub fn tb_add_url(&self) -> String {
        self.tr("تنزيل جديد", "New Download")
    }
    pub fn tb_paste_url(&self) -> String {
        self.tr("من الحافظة", "Paste URL")
    }
    pub fn tb_import(&self) -> String {
        self.tr("استيراد", "Import")
    }
    pub fn tb_resume(&self) -> String {
        self.tr("بدء / استئناف", "Start / Resume")
    }
    pub fn tb_stop(&self) -> String {
        self.tr("إيقاف مؤقت", "Pause")
    }
    pub fn tb_cancel(&self) -> String {
        self.tr("إلغاء", "Cancel")
    }
    pub fn tb_retry(&self) -> String {
        self.tr("إعادة محاولة", "Retry")
    }
    pub fn tb_stop_all(&self) -> String {
        self.tr("إيقاف الكل", "Pause All")
    }
    pub fn tb_delete(&self) -> String {
        self.tr("حذف", "Delete")
    }
    pub fn tb_del_completed(&self) -> String {
        self.tr("مسح المكتمل", "Clear Done")
    }
    pub fn tb_scheduler(&self) -> String {
        self.tr("المجدول", "Scheduler")
    }
    pub fn tb_options(&self) -> String {
        self.tr("الإعدادات", "Settings")
    }
    pub fn tb_start_queue(&self) -> String {
        self.tr("بدء الطابور", "Start Queue")
    }
    pub fn tb_stop_queue(&self) -> String {
        self.tr("إيقاف الطابور", "Stop Queue")
    }
    pub fn tb_grabber(&self) -> String {
        self.tr("أداة الزحف", "Site Grabber")
    }
    pub fn tb_batch(&self) -> String {
        self.tr("دفعة روابط", "Batch DL")
    }
    pub fn tb_tell_friend(&self) -> String {
        self.tr("فاحص ZIP", "ZIP Inspector")
    }

    // 4. اللوحة الجانبية والفئات (Sidebar Categories)
    pub fn cat_title(&self) -> String {
        self.tr("التصنيفات والطوابير", "Categories & Queues")
    }
    pub fn cat_all(&self) -> String {
        self.tr("جميع التنزيلات", "All Downloads")
    }
    pub fn cat_active(&self) -> String {
        self.tr("النشطة", "Active")
    }
    pub fn cat_queued(&self) -> String {
        self.tr("في الانتظار", "Queued")
    }
    pub fn cat_paused(&self) -> String {
        self.tr("المتوقفة مؤقتاً", "Paused")
    }
    pub fn cat_unfinished(&self) -> String {
        self.tr("غير المكتملة", "Unfinished")
    }
    pub fn cat_finished(&self) -> String {
        self.tr("المكتملة", "Completed")
    }
    pub fn cat_failed(&self) -> String {
        self.tr("الفاشلة", "Failed")
    }
    pub fn cat_compressed(&self) -> String {
        self.tr("الملفات المضغوطة", "Compressed")
    }
    pub fn cat_documents(&self) -> String {
        self.tr("المستندات", "Documents")
    }
    pub fn cat_music(&self) -> String {
        self.tr("الموسيقى والصوت", "Music")
    }
    pub fn cat_programs(&self) -> String {
        self.tr("البرامج والتطبيقات", "Programs")
    }
    pub fn cat_video(&self) -> String {
        self.tr("الفيديو والوسائط", "Video & Media")
    }
    pub fn cat_grabber_projects(&self) -> String {
        self.tr("مشاريع الزاحف", "Grabber Projects")
    }
    pub fn cat_queues(&self) -> String {
        self.tr("طوابير التنزيل", "Download Queues")
    }
    pub fn cat_main_queue(&self) -> String {
        self.tr("الطابور الرئيسي (main)", "Main Queue")
    }
    pub fn cat_sync_queue(&self) -> String {
        self.tr("طابور المزامنة (sync)", "Sync Queue")
    }

    // 5. أعمدة جدول التنزيلات (Table Columns)
    pub fn column_title(&self, col: TableColumnId) -> String {
        match col {
            TableColumnId::Status => self.tr("الحالة", "Status"),
            TableColumnId::Filename => self.tr("اسم الملف", "File Name"),
            TableColumnId::TotalSize => self.tr("الحجم الإجمالي", "Total Size"),
            TableColumnId::Downloaded => self.tr("تم تنزيله", "Downloaded"),
            TableColumnId::Progress => self.tr("التقدم", "Progress"),
            TableColumnId::Speed => self.tr("السرعة", "Speed"),
            TableColumnId::TimeLeft => self.tr("الوقت المتبقي", "Time Left"),
            TableColumnId::Connections => self.tr("الاتصالات", "Conns"),
            TableColumnId::Category => self.tr("الفئة", "Category"),
            TableColumnId::CreatedAt => self.tr("تاريخ الإضافة", "Date Added"),
            TableColumnId::SaveFolder => self.tr("مجلد الحفظ", "Save Folder"),
            TableColumnId::Queue => self.tr("الطابور", "Queue"),
        }
    }

    pub fn col_filename(&self) -> String {
        self.column_title(TableColumnId::Filename)
    }
    pub fn col_queue(&self) -> String {
        self.column_title(TableColumnId::Queue)
    }
    pub fn col_size(&self) -> String {
        self.column_title(TableColumnId::TotalSize)
    }
    pub fn col_status(&self) -> String {
        self.column_title(TableColumnId::Status)
    }
    pub fn col_time_left(&self) -> String {
        self.column_title(TableColumnId::TimeLeft)
    }
    pub fn col_transfer_rate(&self) -> String {
        self.column_title(TableColumnId::Speed)
    }
    pub fn col_last_try(&self) -> String {
        self.column_title(TableColumnId::CreatedAt)
    }
    pub fn col_description(&self) -> String {
        self.column_title(TableColumnId::Category)
    }

    // 6. القوائم السياقية (Context Menus)
    pub fn ctx_open(&self) -> String {
        self.tr("فتح الملف", "Open File")
    }
    pub fn ctx_open_with(&self) -> String {
        self.tr("فتح بواسطة...", "Open With...")
    }
    pub fn ctx_open_folder(&self) -> String {
        self.tr("فتح المجلد المحتوي على الملف", "Open Containing Folder")
    }
    pub fn ctx_redownload(&self) -> String {
        self.tr("إعادة تشغيل التنزيل من البداية", "Restart Download from Beginning")
    }
    pub fn ctx_resume(&self) -> String {
        self.tr("استئناف التنزيل", "Resume Download")
    }
    pub fn ctx_pause(&self) -> String {
        self.tr("إيقاف مؤقت", "Pause Download")
    }
    pub fn ctx_cancel(&self) -> String {
        self.tr("إلغاء التنزيل", "Cancel Download")
    }
    pub fn ctx_retry(&self) -> String {
        self.tr("إعادة المحاولة", "Retry Download")
    }
    pub fn ctx_refresh_url(&self) -> String {
        self.tr(
            "تحديث رابط التحميل المنتهي...",
            "Refresh Expired Download Link...",
        )
    }
    pub fn ctx_rename_file(&self) -> String {
        self.tr("تغيير اسم الملف / مجلد الحفظ...", "Rename / Change Save Folder...")
    }
    pub fn ctx_remove(&self) -> String {
        self.tr("حذف سجل التنزيل فقط (مع إبقاء الملف)", "Remove Task Record Only")
    }
    pub fn ctx_delete_with_file(&self) -> String {
        self.tr(
            "حذف السجل والملف نهائياً من القرص...",
            "Delete Record & File from Disk...",
        )
    }
    pub fn ctx_copy_url(&self) -> String {
        self.tr("نسخ رابط المصدر", "Copy Source URL")
    }
    pub fn ctx_copy_filename(&self) -> String {
        self.tr("نسخ اسم الملف", "Copy File Name")
    }
    pub fn ctx_copy_path(&self) -> String {
        self.tr("نسخ المسار الكامل", "Copy Full Local Path")
    }
    pub fn ctx_properties(&self) -> String {
        self.tr("عرض التفاصيل...", "View Details...")
    }
    pub fn ctx_view_error_log(&self) -> String {
        self.tr("عرض سبب الفشل وسجل الأخطاء...", "View Failure Reason & Error Log...")
    }
    pub fn ctx_reverify(&self) -> String {
        self.tr(
            "التحقق من سلامة الملف (SHA-256 / MD5 / CRC32)",
            "Verify File Integrity (SHA-256 / MD5 / CRC32)",
        )
    }

    // 7. الحقول والأزرار العامة (General Labels & Buttons)
    pub fn category_label(&self) -> String {
        self.tr("الفئة", "Category")
    }
    pub fn save_folder_label(&self) -> String {
        self.tr("مجلد الحفظ", "Save Folder")
    }
    pub fn file_name_label(&self) -> String {
        self.tr("اسم الملف", "File Name")
    }
    pub fn remember_path_label(&self, category: &str) -> String {
        match self.lang {
            AppLanguage::Arabic => self.ar(&format!("تذكر هذا المسار الافتراضي لفئة \"{category}\"")),
            AppLanguage::English => format!("Remember this default path for \"{category}\""),
        }
    }
    pub fn description_label(&self) -> String {
        self.tr("الوصف / ملاحظات", "Description")
    }
    pub fn start_download(&self) -> String {
        self.tr("بدء التنزيل الآن", "Start Download Now")
    }
    pub fn download_later(&self) -> String {
        self.tr("إضافة إلى قائمة الانتظار", "Add to Queue (Later)")
    }
    pub fn cancel(&self) -> String {
        self.tr("إلغاء", "Cancel")
    }
    pub fn download_complete(&self) -> String {
        self.tr("اكتمل التنزيل بنجاح", "Download Complete")
    }
    pub fn open_folder(&self) -> String {
        self.tr("فتح المجلد", "Open Folder")
    }
    pub fn minimize_to_tray(&self) -> String {
        self.tr("إبقاء البرنامج في الخلفية (Tray)", "Keep Running in Tray")
    }
    pub fn exit(&self) -> String {
        self.tr("خروج من البرنامج", "Exit Application")
    }

    // 8. نصوص حالات التنزيل الدقيقة (Exact Download Status Texts)
    pub fn status_text(&self, status: DownloadStatus) -> String {
        match status {
            DownloadStatus::Queued => self.tr("في الانتظار", "Queued"),
            DownloadStatus::Downloading => self.tr("جارٍ التنزيل", "Downloading"),
            DownloadStatus::Paused => self.tr("متوقف مؤقتاً", "Paused"),
            DownloadStatus::Completed => self.tr("مكتمل", "Completed"),
            DownloadStatus::Failed => self.tr("فشل", "Failed"),
            DownloadStatus::NeedsUrlRefresh => self.tr("انتهت صلاحية الرابط", "Link Expired"),
            DownloadStatus::RefreshingUrl => self.tr("جارٍ تحديث الرابط", "Refreshing Link"),
            DownloadStatus::Assembling => self.tr("جارٍ دمج الأجزاء", "Merging Segments"),
            DownloadStatus::VerifyingChecksum => self.tr("جارٍ التحقق", "Verifying Checksum"),
        }
    }

    pub fn status_text_for_task(&self, task: &crate::DownloadStateMeta) -> String {
        if task.is_cancelled_by_user() {
            return self.tr("ملغى", "Cancelled");
        }
        if task.status == DownloadStatus::Completed && !task.final_file_path().exists() {
            return self.tr("مكتمل (الملف مفقود)", "Completed (File Missing)");
        }
        self.status_text(task.status)
    }

    pub fn category_name(&self, cat: FileCategory) -> String {
        match cat {
            FileCategory::Compressed => self.tr("مضغوطة", "Compressed"),
            FileCategory::Documents => self.tr("مستندات", "Documents"),
            FileCategory::Music => self.tr("موسيقى", "Music"),
            FileCategory::Programs => self.tr("برامج", "Programs"),
            FileCategory::Video => self.tr("فيديو ووسائط", "Video & Media"),
            FileCategory::General => self.tr("عام", "General"),
        }
    }

    pub fn density_name(&self, d: RowDensity) -> String {
        match d {
            RowDensity::Compact => self.tr("مضغوطة (Compact)", "Compact"),
            RowDensity::Comfortable => self.tr("مريحة (Comfortable)", "Comfortable"),
            RowDensity::Spacious => self.tr("واسعة (Spacious)", "Spacious"),
        }
    }

    pub fn options_tab_name(&self, tab: OptionsTab) -> String {
        match tab {
            OptionsTab::General => self.tr("عام (General)", "General"),
            OptionsTab::Appearance => self.tr("المظهر (Appearance)", "Appearance"),
            OptionsTab::Downloads => self.tr("التنزيلات (Downloads)", "Downloads"),
            OptionsTab::Connections => self.tr("الاتصالات (Connections)", "Connections"),
            OptionsTab::BrowserIntegration => {
                self.tr("تكامل المتصفح (Browser)", "Browser Integration")
            }
            OptionsTab::Media => self.tr("الوسائط (Media)", "Media & Tools"),
            OptionsTab::Notifications => self.tr("الإشعارات (Notifications)", "Notifications"),
            OptionsTab::Advanced => self.tr("متقدم (Advanced)", "Advanced"),
        }
    }

    pub fn format_bytes(&self, bytes: u64) -> String {
        if bytes == 0 {
            return self.tr("غير معروف", "Unknown");
        }
        let (units, b_unit) = match self.lang {
            AppLanguage::Arabic => (["ب", "ك.ب", "م.ب", "غ.ب", "ت.ب"], "ب"),
            AppLanguage::English => (["B", "KB", "MB", "GB", "TB"], "B"),
        };
        let mut value = bytes as f64;
        let mut unit = 0;
        while value >= 1024.0 && unit < units.len() - 1 {
            value /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            if self.lang == AppLanguage::Arabic {
                shape_arabic(&format!("{bytes} {b_unit}"))
            } else {
                format!("{bytes} {b_unit}")
            }
        } else if self.lang == AppLanguage::Arabic {
            shape_arabic(&format!("{value:.2} {}", units[unit]))
        } else {
            format!("{value:.2} {}", units[unit])
        }
    }

    pub fn format_downloaded_bytes(&self, bytes: u64) -> String {
        if bytes == 0 {
            return match self.lang {
                AppLanguage::Arabic => shape_arabic("0 ب"),
                AppLanguage::English => "0 B".to_string(),
            };
        }
        self.format_bytes(bytes)
    }

    pub fn format_speed(&self, bps: u64) -> String {
        if bps == 0 {
            return match self.lang {
                AppLanguage::Arabic => shape_arabic("0 ك.ب/ث"),
                AppLanguage::English => "0 KB/s".to_string(),
            };
        }
        let suffix = match self.lang {
            AppLanguage::Arabic => "ث",
            AppLanguage::English => "s",
        };
        let res = if bps >= 1024 * 1024 * 1024 {
            let gb = bps as f64 / (1024.0 * 1024.0 * 1024.0);
            let u = if self.lang == AppLanguage::Arabic { "غ.ب" } else { "GB" };
            format!("{gb:.2} {u}/{suffix}")
        } else if bps >= 1024 * 1024 {
            let mb = bps as f64 / (1024.0 * 1024.0);
            let u = if self.lang == AppLanguage::Arabic { "م.ب" } else { "MB" };
            format!("{mb:.2} {u}/{suffix}")
        } else {
            let kb = bps as f64 / 1024.0;
            let u = if self.lang == AppLanguage::Arabic { "ك.ب" } else { "KB" };
            format!("{kb:.1} {u}/{suffix}")
        };

        if self.lang == AppLanguage::Arabic {
            shape_arabic(&res)
        } else {
            res
        }
    }

    pub fn time_left_str(&self, seconds: u64) -> String {
        match self.lang {
            AppLanguage::Arabic => {
                let s = if seconds >= 3600 {
                    format!("{} س {} د", seconds / 3600, (seconds % 3600) / 60)
                } else if seconds >= 60 {
                    format!("{} د {} ث", seconds / 60, seconds % 60)
                } else {
                    format!("{seconds} ث")
                };
                shape_arabic(&s)
            }
            AppLanguage::English => {
                if seconds >= 3600 {
                    format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
                } else if seconds >= 60 {
                    format!("{}m {}s", seconds / 60, seconds % 60)
                } else {
                    format!("{seconds}s")
                }
            }
        }
    }
}
