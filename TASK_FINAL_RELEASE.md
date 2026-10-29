# خطة وسجل تنفيذ النسخة النهائية الكاملة — StormGet v2.3.0 (Final Gold Release)

يوضح هذا الملف جميع المهام والفجوات الهندسية بالترتيب الزمني لتحويل **StormGet v2.3.0** من نسخة مرشحة للإطلاق (`Release Candidate 90%`) إلى **نسخة إنتاج نهائية متكاملة 100% (`Final Gold Release`)** مع حالة تنفيذ كل بند والملفات البرمجية المرتبطة به.

## 1. جدول متابعة الفجوات والمهام بالترتيب

| الترتيب | المهمة / الفجوة الهندسية | الحالة | الملفات المنفذة داخل المشروع |
| :--- | :--- | :--- | :--- |
| **المهمة 0** | تحويل الشعار الرسمي إلى `.ico` متعدد الدقات و `.png` و `.rgba` وربطه بالملف التنفيذي والواجهتين وإضافة المتصفح | مكتمل `[x]` | `assets/icon.ico`, `assets/icon.png`, `assets/icon_64.rgba`, `assets/app.rc`, `build.rs`, `src/native_ui/mod.rs`, `ui/app.slint`, `chrome-extension/icons/*` |
| **الفجوة 1** | ربط مزايا المحرك الجديدة بواجهتي `egui Native UI` و `Slint UI` (وضع الألعاب، المعاينة الحية، طبيب التحميل، فحص الذكاء المسبق، التورنت `magnet:?`) | مكتمل `[x]` | `src/native_ui/app.rs`, `src/native_ui/dialogs.rs`, `ui/app.slint`, `src/slint_ui/mod.rs` |
| **الفجوة 2** | التفعيل الحي للمتحكم التكيفي بالاتصالات (`AdaptiveController 4 → 16 → 32`) ودمج مسارات الشبكات المتعددة (`MultiNetworkLanePool`) داخل حلقة التحميل الفعلية | مكتمل `[x]` | `src/core/segmenter.rs`, `src/core/lanes.rs`, `src/api/routes.rs` |
| **الفجوة 3** | تفعيل شريط تقدم شريط مهام ويندوز (`ITaskbarList3`) في حلقة تحديث الواجهتين (`Slint` و `egui`) | مكتمل `[x]` | `src/core/winfx.rs`, `src/native_ui/app.rs`, `src/slint_ui/mod.rs` |
| **الفجوة 4** | استكمال أوامر وأحداث عامل الخلفية (`UiCommand` & `UiEvent`) للربط المباشر بدون وسطاء خارجيين | مكتمل `[x]` | `src/native_ui/types.rs`, `src/native_ui/worker.rs` |
| **الفجوة 5** | إعداد سكربت المثبت الرسمي (`Inno Setup`) وأداة توقيع الإصدارات الرقمية (`Ed25519 sign-release`) وتحديث `BUILD.bat` | مكتمل `[x]` | `installer/StormGet-Setup.iss`, `scripts/sign_release.py`, `src/main.rs`, `BUILD.bat` |

## 2. تفاصيل التنفيذ الهندسي لكل مهمة

### المهمة 0: الشعار الرسمي والأيقونات متعددة الدقات (`.ico` + `.png` + `.rgba`)
- **بناء ملف `StormGet.ico` القياسي لويندوز**: يضم 7 دقات مدمجة بشفافية كاملة (`16x16`, `24x24`, `32x32`, `48x48`, `64x64`, `128x128`, `256x256`) لعرض فائق الوضوح على جميع شاشات `DPI` العالية وشريط المهام وسطح المكتب.
- **الربط التلقائي بالملف التنفيذي `.exe`**: إنشاء `assets/app.rc` وتحديث `build.rs` ليقوم بتضمين الأيقونة تلقائياً في `rust_idm.exe` عند البناء على ويندوز عبر `rc.exe` أو `llvm-rc` أو `windres`.
- **الربط بواجهتي سطح المكتب وصينية النظام (`System Tray`)**:
  - في `src/native_ui/mod.rs`: تحديث `stormget_app_icon_rgba()` لتقرأ الأيقونة الرسمية المدمجة `assets/icon_64.rgba` (`64x64 RGBA`) مباشرة في شريط العنوان وجميع النوافذ الفرعية وأيقونة صينية النظام (`tray.rs`).
  - في `ui/app.slint`: إضافة `icon: @image-url("../assets/icon.png");` لجميع النوافذ الـ 6 المستقلة (`MainWindow`, `AddDownloadWindow`, `DetailsWindow`, `SchedulerWindow`, `SettingsWindow`, `DiagnosticsWindow`).
- **توليد أيقونات إضافة المتصفح**: توليد `icon16.png` و `icon32.png` و `icon48.png` و `icon128.png` داخل `chrome-extension/icons/` وربطها في `manifest.json` و `manifest.firefox.json`.

### الفجوة 1 و 4: ربط المزايا المتقدمة بالواجهتين وعامل الخلفية (`worker.rs`)
- **وضع الألعاب والخنق الذكي (`🎮 وضع الألعاب / Game Mode`)**: زر مباشر في شريط الأدوات الرئيسي لكل من `Slint UI` و `egui Native UI` يفعل أو يوقف وضع الألعاب فوراً عبر `UiCommand::ToggleGameMode` و `on_toggle_game_mode`.
- **المعاينة الحية أثناء التحميل (`🎬 معاينة أثناء التحميل / Play Preview`)**: متاحة في القائمة السياقية (كليك يمين) وفي نافذة تفاصيل المهمة لملفات الفيديو والصوت، وتقوم بتشغيل الجزء المحمّل بأمان عبر `VLC / PotPlayer / MPV` دون قفل ملف `.part`.
- **طبيب التحميل الذكي (`🩺 تشخيص العطل / Download Doctor`)**: زر تشخيص فوري للمهام المتوقفة أو الفاشلة يحلل سبب العطل (`SessionExpired`, `LinkExpired`, `RateLimited`, `DiskFull`, `RangeRejected`) ويعرض التفسير العربي والإجراء العلاجي المقترح داخل نافذة التفاصيل وإشعارات البرنامج.
- **فحص الذكاء المسبق (`🛡 Pre-Download Intelligence`) ودعم التورنت (`magnet:?` / `.torrent`)**: عند فحص الرابط في نافذة الإضافة، يظهر تقييم الذكاء المسبق (كشف صفحات الويب الخادعة، الروابط المؤقتة، والتكرار الذكي)، كما تقبل نافذة الإضافة روابط `magnet:?` وملفات `.torrent` مباشرة.

### الفجوة 2: التفعيل الحي للمتحكم التكيفي بالاتصالات ودمج مسارات الشبكات (`segmenter.rs`)
- **التدرج التكيفي للاتصالات (`AdaptiveController`)**: يبدأ التحميل بالعدد المبدئي للاتصالات ويقيس معدل النقل كل ثانية داخل `DynamicDownloadJob::execute` ليضاعف الاتصالات تلقائياً عندما تحقق نمواً حقيقياً في السرعة، ويتراجع فوراً إلى النصف (`on_throttled`) عند رصد `HTTP 429 Too Many Requests` أو `503 Service Unavailable`.
- **توزيع القطاعات عبر مسارات الشبكة المتعددة (`MultiNetworkLanePool`)**: داخل `download_segment`، يتم اختيار مسار الشبكة الأقل عبئاً والأعلى سرعة (`pick_lane_for_url`) مع تتبع العدادات الحية للاتصالات والبايتات وعزل أي مسار متعثر تلقائياً.

### الفجوة 3: شريط التقدم على أيقونة البرنامج في شريط مهام ويندوز (`ITaskbarList3`)
- إضافة `update_process_taskbar_progress` في `src/core/winfx.rs` واستدعاؤها تلقائياً في دورة تحديث `src/slint_ui/mod.rs` و `src/native_ui/app.rs` لعرض الشريط الأخضر (`Normal`) أثناء التحميل، الأصفر (`Paused`) عند الإيقاف المؤقت، والأحمر (`Error`) عند فشل مهمة.

### الفجوة 5: المثبت النهائي (`Inno Setup`) وتوقيع الإصدارات الرقمي (`Ed25519`)
- **سكربت المثبت `installer/StormGet-Setup.iss`**: يجمع `rust_idm.exe`، الأيقونة الرسمية `assets/icon.ico`، الخطوط، وإضافة المتصفح في مثبت ويندوز احترافي `StormGet-v2.3.0-Setup-x64.exe` متوافق مع نظام التحديث الذاتي الآمن.
- **أمر التوقيع الرقمي `rust_idm sign-release`**: أمر CLI مدمج في `src/main.rs` (مع سكربت مساعد `scripts/sign_release.py`) يحسب بصمة `SHA-256` للمثبت ويوقعه رقمياً بـ `Ed25519` لتوليد ملف `version.json` الموثق.
