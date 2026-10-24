use super::{arabic::shape_arabic, types::AppLanguage};
use crate::{DownloadStatus, FileCategory};

pub struct Loc {
    pub lang: AppLanguage,
}

impl Loc {
    pub fn new(lang: AppLanguage) -> Self {
        Self { lang }
    }

    #[inline]
    fn tr(&self, ar: &str, en: &'static str) -> String {
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

    pub fn download_details_title(&self) -> String {
        self.tr("تفاصيل التنزيل", "Download details")
    }
    pub fn category_label(&self) -> String {
        self.tr("الفئات", "Category")
    }
    pub fn save_folder_label(&self) -> String {
        self.tr("مجلد الحفظ", "Save folder")
    }
    pub fn file_name_label(&self) -> String {
        self.tr("اسم الملف", "File name")
    }
    pub fn remember_path_label(&self, category: &str) -> String {
        match self.lang {
            AppLanguage::Arabic => self.ar(&format!("تذكر هذا المسار لمجلد \"{category}\"")),
            AppLanguage::English => format!("Remember this path for the \"{category}\" folder"),
        }
    }
    pub fn description_label(&self) -> String {
        self.tr("الوصف", "Description")
    }
    pub fn start_download(&self) -> String {
        self.tr("بدء التحميل", "Start download")
    }
    pub fn download_later(&self) -> String {
        self.tr("التحميل لاحقاً", "Download later")
    }
    pub fn cancel(&self) -> String {
        self.tr("إلغاء", "Cancel")
    }
    pub fn download_complete(&self) -> String {
        self.tr("اكتمل التنزيل", "Download complete")
    }
    pub fn open_folder(&self) -> String {
        self.tr("فتح المجلد", "Open folder")
    }
    pub fn minimize_to_tray(&self) -> String {
        self.tr("تصغير إلى شريط النظام", "Minimize to tray")
    }
    pub fn exit(&self) -> String {
        self.tr("خروج", "Exit")
    }

    // Menus
    pub fn menu_tasks(&self) -> String {
        self.tr("المهام", "Tasks")
    }
    pub fn menu_file(&self) -> String {
        self.tr("ملف", "File")
    }
    pub fn menu_downloads(&self) -> String {
        self.tr("التحميل", "Downloads")
    }
    pub fn menu_view(&self) -> String {
        self.tr("عرض", "View")
    }
    pub fn menu_help(&self) -> String {
        self.tr("تعليمات", "Help")
    }

    // Toolbar Buttons
    pub fn tb_add_url(&self) -> String {
        self.tr("إضافة رابط", "Add URL")
    }
    pub fn tb_resume(&self) -> String {
        self.tr("إستئناف", "Resume")
    }
    pub fn tb_stop(&self) -> String {
        self.tr("إيقاف", "Stop")
    }
    pub fn tb_stop_all(&self) -> String {
        self.tr("إيقاف الكل", "Stop All")
    }
    pub fn tb_delete(&self) -> String {
        self.tr("حذف", "Delete")
    }
    pub fn tb_del_completed(&self) -> String {
        self.tr("حذف المكتمل...", "Del Comp...")
    }
    pub fn tb_options(&self) -> String {
        self.tr("خيارات", "Options")
    }
    pub fn tb_scheduler(&self) -> String {
        self.tr("الجدولة", "Scheduler")
    }
    pub fn tb_start_queue(&self) -> String {
        self.tr("بدء المنتظرة", "Start Qu...")
    }
    pub fn tb_stop_queue(&self) -> String {
        self.tr("إيقاف المنتظرة...", "Stop Qu...")
    }
    pub fn tb_grabber(&self) -> String {
        self.tr("أداة الإمساك", "Grabber")
    }
    pub fn tb_batch(&self) -> String {
        self.tr("حزمة روابط", "Batch DL")
    }
    pub fn tb_tell_friend(&self) -> String {
        self.tr("حث صديق", "Tell Friend")
    }

    // Categories Tree
    pub fn cat_title(&self) -> String {
        self.tr("الفئات", "Categories")
    }
    pub fn cat_all(&self) -> String {
        self.tr("كل التحميلات", "All Downloads")
    }
    pub fn cat_compressed(&self) -> String {
        self.tr("مضغوطة", "Compressed")
    }
    pub fn cat_documents(&self) -> String {
        self.tr("مستندات", "Documents")
    }
    pub fn cat_music(&self) -> String {
        self.tr("موسيقى", "Music")
    }
    pub fn cat_programs(&self) -> String {
        self.tr("برامج", "Programs")
    }
    pub fn cat_video(&self) -> String {
        self.tr("فيديو", "Video")
    }
    pub fn cat_unfinished(&self) -> String {
        self.tr("الناقصة", "Unfinished")
    }
    pub fn cat_finished(&self) -> String {
        self.tr("المكتملة", "Finished")
    }
    pub fn cat_grabber_projects(&self) -> String {
        self.tr("مشاريع أداة الإمساك", "Grabber Projects")
    }
    pub fn cat_queues(&self) -> String {
        self.tr("الملفات المنتظرة", "Queues")
    }
    pub fn cat_main_queue(&self) -> String {
        self.tr("منتظرة الرئيسية", "Main Queue")
    }
    pub fn cat_sync_queue(&self) -> String {
        self.tr("مزامنة المنتظرة", "Sync Queue")
    }

    // Table Columns
    pub fn col_filename(&self) -> String {
        self.tr("إسم الملف", "File Name")
    }
    pub fn col_queue(&self) -> String {
        self.tr("ق", "Q")
    }
    pub fn col_size(&self) -> String {
        self.tr("الحجم", "Size")
    }
    pub fn col_status(&self) -> String {
        self.tr("الحالة", "Status")
    }
    pub fn col_time_left(&self) -> String {
        self.tr("الوقت المتبقي", "Time left")
    }
    pub fn col_transfer_rate(&self) -> String {
        self.tr("معدل النقل", "Transfer rate")
    }
    pub fn col_last_try(&self) -> String {
        self.tr("تاريخ آخر محاولة", "Last Try Date")
    }
    pub fn col_description(&self) -> String {
        self.tr("وصف", "Description")
    }

    // Context Menu Items
    pub fn ctx_open(&self) -> String {
        self.tr("فتح", "Open")
    }
    pub fn ctx_open_with(&self) -> String {
        self.tr("فتح بواسطة...", "Open with...")
    }
    pub fn ctx_open_folder(&self) -> String {
        self.tr("فتح المجلد", "Open folder")
    }
    pub fn ctx_redownload(&self) -> String {
        self.tr("إعادة تحميل", "Redownload")
    }
    pub fn ctx_resume(&self) -> String {
        self.tr("إستئناف التحميل", "Resume download")
    }
    pub fn ctx_pause(&self) -> String {
        self.tr("إيقاف التحميل", "Pause / Stop download")
    }
    pub fn ctx_refresh_url(&self) -> String {
        self.tr("تحديث عنوان التحميل (رابط منتهي)...", "Refresh download address (Expired link)...")
    }
    pub fn ctx_remove(&self) -> String {
        self.tr("إزالة السجل", "Remove Task Record")
    }
    pub fn ctx_delete_with_file(&self) -> String {
        self.tr("حذف المهمة مع الملف من القرص", "Delete Task and Local File")
    }
    pub fn ctx_copy_url(&self) -> String {
        self.tr("نسخ عنوان الرابط", "Copy Download URL")
    }
    pub fn ctx_copy_path(&self) -> String {
        self.tr("نسخ مسار الملف المحلي", "Copy Local File Path")
    }
    pub fn ctx_properties(&self) -> String {
        self.tr("خصائص وتفاصيل الاتصال...", "Properties & Connection Details...")
    }
    pub fn ctx_reverify(&self) -> String {
        self.tr("إعادة التحقق من البصمة (SHA256/MD5/CRC32)", "Re-verify Checksum (SHA256/MD5/CRC32)")
    }

    // Status Badges
    pub fn status_text(&self, status: DownloadStatus) -> String {
        match status {
            DownloadStatus::Completed => self.tr("إكتمال", "Complete"),
            DownloadStatus::Downloading => self.tr("تحميل", "Downloading"),
            DownloadStatus::Paused => self.tr("إيقاف", "Paused"),
            DownloadStatus::Failed => self.tr("خطأ", "Failed"),
            DownloadStatus::Queued => self.tr("في الانتظار", "Queued"),
            DownloadStatus::NeedsUrlRefresh => self.tr("رابط منتهي", "Link Expired"),
            DownloadStatus::RefreshingUrl => self.tr("تحديث الرابط", "Refreshing Link"),
            DownloadStatus::Assembling => self.tr("تجميع الملف", "Assembling"),
            DownloadStatus::VerifyingChecksum => self.tr("فحص البصمة", "Verifying"),
        }
    }

    pub fn category_name(&self, cat: FileCategory) -> String {
        match cat {
            FileCategory::Compressed => self.tr("مضغوطة", "Compressed"),
            FileCategory::Documents => self.tr("مستندات", "Documents"),
            FileCategory::Music => self.tr("موسيقى", "Music"),
            FileCategory::Programs => self.tr("برامج", "Programs"),
            FileCategory::Video => self.tr("فيديو", "Video"),
            FileCategory::General => self.tr("عام", "General"),
        }
    }

    pub fn format_bytes(&self, bytes: u64) -> String {
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
        } else {
            if self.lang == AppLanguage::Arabic {
                shape_arabic(&format!("{value:.2} {}", units[unit]))
            } else {
                format!("{value:.2} {}", units[unit])
            }
        }
    }

    pub fn format_speed(&self, bps: u64) -> String {
        let suffix = match self.lang {
            AppLanguage::Arabic => "ثانية",
            AppLanguage::English => "sec",
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
                    format!("{seconds} ثانية")
                };
                shape_arabic(&s)
            }
            AppLanguage::English => {
                if seconds >= 3600 {
                    format!("{} h {} m", seconds / 3600, (seconds % 3600) / 60)
                } else if seconds >= 60 {
                    format!("{} m {} s", seconds / 60, seconds % 60)
                } else {
                    format!("{seconds} sec")
                }
            }
        }
    }
}
