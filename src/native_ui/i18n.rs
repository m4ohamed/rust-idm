use super::types::AppLanguage;
use crate::{DownloadStatus, FileCategory};

pub struct Loc {
    pub lang: AppLanguage,
}

impl Loc {
    pub fn new(lang: AppLanguage) -> Self {
        Self { lang }
    }

    // Menus
    pub fn menu_tasks(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "المهام",
            AppLanguage::English => "Tasks",
        }
    }
    pub fn menu_file(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "ملف",
            AppLanguage::English => "File",
        }
    }
    pub fn menu_downloads(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "التحميل",
            AppLanguage::English => "Downloads",
        }
    }
    pub fn menu_view(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "عرض",
            AppLanguage::English => "View",
        }
    }
    pub fn menu_help(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "تعليمات",
            AppLanguage::English => "Help",
        }
    }

    // Toolbar Buttons
    pub fn tb_add_url(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إضافة رابط",
            AppLanguage::English => "Add URL",
        }
    }
    pub fn tb_resume(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إستئناف",
            AppLanguage::English => "Resume",
        }
    }
    pub fn tb_stop(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إيقاف",
            AppLanguage::English => "Stop",
        }
    }
    pub fn tb_stop_all(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إيقاف الكل",
            AppLanguage::English => "Stop All",
        }
    }
    pub fn tb_delete(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "حذف",
            AppLanguage::English => "Delete",
        }
    }
    pub fn tb_del_completed(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "حذف المكتمل...",
            AppLanguage::English => "Del Comp...",
        }
    }
    pub fn tb_options(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "خيارات",
            AppLanguage::English => "Options",
        }
    }
    pub fn tb_scheduler(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الجدولة",
            AppLanguage::English => "Scheduler",
        }
    }
    pub fn tb_start_queue(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "بدء المنتظرة",
            AppLanguage::English => "Start Qu...",
        }
    }
    pub fn tb_stop_queue(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إيقاف المنتظرة...",
            AppLanguage::English => "Stop Qu...",
        }
    }
    pub fn tb_grabber(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "أداة الإمساك",
            AppLanguage::English => "Grabber",
        }
    }
    pub fn tb_batch(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "حزمة روابط",
            AppLanguage::English => "Batch DL",
        }
    }
    pub fn tb_tell_friend(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "حث صديق",
            AppLanguage::English => "Tell Friend",
        }
    }

    // Categories Tree
    pub fn cat_title(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الفئات",
            AppLanguage::English => "Categories",
        }
    }
    pub fn cat_all(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "كل التحميلات",
            AppLanguage::English => "All Downloads",
        }
    }
    pub fn cat_compressed(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "مضغوطة",
            AppLanguage::English => "Compressed",
        }
    }
    pub fn cat_documents(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "مستندات",
            AppLanguage::English => "Documents",
        }
    }
    pub fn cat_music(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "موسيقى",
            AppLanguage::English => "Music",
        }
    }
    pub fn cat_programs(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "برامج",
            AppLanguage::English => "Programs",
        }
    }
    pub fn cat_video(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "فيديو",
            AppLanguage::English => "Video",
        }
    }
    pub fn cat_unfinished(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الناقصة",
            AppLanguage::English => "Unfinished",
        }
    }
    pub fn cat_finished(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "المكتملة",
            AppLanguage::English => "Finished",
        }
    }
    pub fn cat_grabber_projects(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "مشاريع أداة الإمساك",
            AppLanguage::English => "Grabber Projects",
        }
    }
    pub fn cat_queues(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الملفات المنتظرة",
            AppLanguage::English => "Queues",
        }
    }
    pub fn cat_main_queue(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "منتظرة الرئيسية",
            AppLanguage::English => "Main Queue",
        }
    }
    pub fn cat_sync_queue(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "مزامنة المنتظرة",
            AppLanguage::English => "Sync Queue",
        }
    }

    // Table Columns
    pub fn col_filename(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إسم الملف",
            AppLanguage::English => "File Name",
        }
    }
    pub fn col_queue(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "ق",
            AppLanguage::English => "Q",
        }
    }
    pub fn col_size(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الحجم",
            AppLanguage::English => "Size",
        }
    }
    pub fn col_status(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الحالة",
            AppLanguage::English => "Status",
        }
    }
    pub fn col_time_left(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "الوقت المتبقي",
            AppLanguage::English => "Time left",
        }
    }
    pub fn col_transfer_rate(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "معدل النقل",
            AppLanguage::English => "Transfer rate",
        }
    }
    pub fn col_last_try(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "تاريخ آخر محاولة",
            AppLanguage::English => "Last Try Date",
        }
    }
    pub fn col_description(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "وصف",
            AppLanguage::English => "Description",
        }
    }

    // Context Menu Items
    pub fn ctx_open(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "فتح",
            AppLanguage::English => "Open",
        }
    }
    pub fn ctx_open_with(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "فتح بواسطة...",
            AppLanguage::English => "Open with...",
        }
    }
    pub fn ctx_open_folder(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "فتح المجلد",
            AppLanguage::English => "Open folder",
        }
    }
    pub fn ctx_redownload(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إعادة تحميل",
            AppLanguage::English => "Redownload",
        }
    }
    pub fn ctx_resume(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إستئناف التحميل",
            AppLanguage::English => "Resume download",
        }
    }
    pub fn ctx_pause(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إيقاف التحميل",
            AppLanguage::English => "Pause / Stop download",
        }
    }
    pub fn ctx_refresh_url(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "تحديث عنوان التحميل (رابط منتهي)...",
            AppLanguage::English => "Refresh download address (Expired link)...",
        }
    }
    pub fn ctx_remove(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إزالة السجل",
            AppLanguage::English => "Remove Task Record",
        }
    }
    pub fn ctx_delete_with_file(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "حذف المهمة مع الملف من القرص",
            AppLanguage::English => "Delete Task and Local File",
        }
    }
    pub fn ctx_copy_url(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "نسخ عنوان الرابط",
            AppLanguage::English => "Copy Download URL",
        }
    }
    pub fn ctx_copy_path(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "نسخ مسار الملف المحلي",
            AppLanguage::English => "Copy Local File Path",
        }
    }
    pub fn ctx_properties(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "خصائص وتفاصيل الاتصال...",
            AppLanguage::English => "Properties & Connection Details...",
        }
    }
    pub fn ctx_reverify(&self) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => "إعادة التحقق من البصمة (SHA256/MD5/CRC32)",
            AppLanguage::English => "Re-verify Checksum (SHA256/MD5/CRC32)",
        }
    }

    // Status Badges
    pub fn status_text(&self, status: DownloadStatus) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => match status {
                DownloadStatus::Completed => "إكتمال",
                DownloadStatus::Downloading => "تحميل",
                DownloadStatus::Paused => "إيقاف",
                DownloadStatus::Failed => "خطأ",
                DownloadStatus::Queued => "في الانتظار",
                DownloadStatus::NeedsUrlRefresh => "رابط منتهي",
                DownloadStatus::RefreshingUrl => "تحديث الرابط",
                DownloadStatus::Assembling => "تجميع الملف",
                DownloadStatus::VerifyingChecksum => "فحص البصمة",
            },
            AppLanguage::English => match status {
                DownloadStatus::Completed => "Complete",
                DownloadStatus::Downloading => "Downloading",
                DownloadStatus::Paused => "Paused",
                DownloadStatus::Failed => "Failed",
                DownloadStatus::Queued => "Queued",
                DownloadStatus::NeedsUrlRefresh => "Link Expired",
                DownloadStatus::RefreshingUrl => "Refreshing Link",
                DownloadStatus::Assembling => "Assembling",
                DownloadStatus::VerifyingChecksum => "Verifying",
            },
        }
    }

    pub fn category_name(&self, cat: FileCategory) -> &'static str {
        match self.lang {
            AppLanguage::Arabic => match cat {
                FileCategory::Compressed => "مضغوطة",
                FileCategory::Documents => "مستندات",
                FileCategory::Music => "موسيقى",
                FileCategory::Programs => "برامج",
                FileCategory::Video => "فيديو",
                FileCategory::General => "عام",
            },
            AppLanguage::English => match cat {
                FileCategory::Compressed => "Compressed",
                FileCategory::Documents => "Documents",
                FileCategory::Music => "Music",
                FileCategory::Programs => "Programs",
                FileCategory::Video => "Video",
                FileCategory::General => "General",
            },
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
            format!("{bytes} {b_unit}")
        } else {
            format!("{value:.2} {}", units[unit])
        }
    }

    pub fn format_speed(&self, bps: u64) -> String {
        let suffix = match self.lang {
            AppLanguage::Arabic => "ثانية",
            AppLanguage::English => "sec",
        };
        if bps >= 1024 * 1024 * 1024 {
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
        }
    }

    pub fn time_left_str(&self, seconds: u64) -> String {
        match self.lang {
            AppLanguage::Arabic => {
                if seconds >= 3600 {
                    format!("{} س {} د", seconds / 3600, (seconds % 3600) / 60)
                } else if seconds >= 60 {
                    format!("{} د {} ث", seconds / 60, seconds % 60)
                } else {
                    format!("{seconds} ثانية")
                }
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
