# 📚 الدروس المستفادة والأنماط المعتمدة (Lessons Learned)

حلقة تحسين ذاتي وتوثيق رياضي وبرمجي للأنماط الخاطئة وتصحيحها لتفادي تكرارها.

---

## 1. تفادي تعارض الاستعارة في Rust (Borrow Checker & Self Mutation)

* **النمط الخاطئ (Anti-pattern)**:
  استدعاء دالة تستعير `&self` والاحتفاظ بالمرجع المستعار داخل كتلة شرطية مع محاولة تعديل أحد حقول `self` في نفس الوقت:
  ```rust
  if let Some(task) = self.selected_download() {
      // خطأ E0506: self مستعار عبر task ولا يمكن تعديل حقول self
      self.refresh_task_id = task.task_id.clone();
      self.refresh_new_url = task.url.clone();
  }
  ```

* **النمط الصحيح (Approved Pattern)**:
  استخراج واستنساخ القيم المرادة كبيانات مملوكة (Owned Values) عبر `.map(...)` أو `.cloned()` لإنهاء استعارة `self` قبل الدخول في جسم الكتلة الذي يجري التعديلات:
  ```rust
  if let Some((task_id, url)) = self
      .selected_download()
      .map(|task| (task.task_id.clone(), task.url.clone()))
  {
      self.refresh_task_id = task_id;
      self.refresh_new_url = url;
  }
  ```
---

## 2. اعتراض التنزيلات الديناميكية في إضافات المتصفح (Chrome Downloads Interception)

* **النمط الخاطئ (Anti-pattern)**:
  الاعتماد الحصري على `chrome.downloads.onCreated` وفحص `new URL(url).pathname` فقط، مما يؤدي لفشل التقاط كافة الروابط الديناميكية (Google Drive, GitHub Releases, Cloudflare, PHP endpoints) لأن مسارها لا ينتهي بامتداد الملف ولأن اسم الملف الحقيقي وترويسات `Content-Disposition` لم تصل بعد:
  ```javascript
  chrome.downloads.onCreated.addListener((item) => {
    // خطأ: item.filename فارغ غالباً في هذه اللحظة، والـ pathname قد لا يحمل الامتداد
    if (!url.endsWith(".zip")) return;
  });
  ```

* **النمط الصحيح (Approved Pattern)**:
  دمج الاستماع لـ `chrome.downloads.onDeterminingFilename` مع `onCreated` لفحص الاسم النهائي وترويسات الـ MIME ومطابقتها مع مسار الرابط ومعاملات الاستعلام `searchParams`، مع وضع واقٍ ضد المعالجة المزدوجة `Set<downloadId>`:
  ```javascript
  function handleDownload(item) {
    if (handledSet.has(item.id)) return;
    if (isTargetFile(item.url, item.filename, item.mime)) {
      handledSet.add(item.id);
      chrome.downloads.cancel(item.id, () => chrome.downloads.erase({ id: item.id }));
      dispatchToEngine(item);
    }
  }
  chrome.downloads.onCreated.addListener(handleDownload);
  chrome.downloads.onDeterminingFilename?.addListener(handleDownload);
  ```

---

## 3. توحيد مسارات الحفظ ومقاومة الأعطال أثناء استعادة الحالة (Resilient State Restoration)

* **النمط الخاطئ (Anti-pattern)**:
  1. تشتت تعريف المسار الافتراضي بين الوحدات (استخدام `./Downloads` في بعض الأماكن و `USERPROFILE/Downloads/RustIDM` في أماكن أخرى).
  2. استخدام عامل التمرير السريع `?` داخل حلقات مسح واستعادة الملفات مما يؤدي لإلغاء استعادة باقي المهام عند حدوث خطأ أو قفل لملف واحد:
  ```rust
  for cat in categories {
      let mut entries = fs::read_dir(&dir).await?; // خطأ: يفشل الدالة كاملة ويحرم المستخدم من باقي السجل
      while let Some(entry) = entries.next_entry().await? { ... }
  }
  ```

* **النمط الصحيح (Approved Pattern)**:
  تعريف دالة مرجعية موحدة للمسارات، وتطبيق نمط التسامح التام مع أخطاء القراءة الفردية واستكشاف كافة المجلدات الجذرية والفرعية:
  ```rust
  for dir in candidate_directories {
      let Ok(mut entries) = fs::read_dir(&dir).await else { continue; };
      while let Ok(Some(entry)) = entries.next_entry().await {
          // معالجة الملف واستمرارية الحلقة حتى لو فشل ملف فردي
      }
  }
  ```

---

## 4. تجنب أخطاء SOP/CSP عند التعامل مع سكربتات سياق الصفحة (Manifest V3 Main World Hooks)

* **النمط الخاطئ (Anti-pattern)**:
  حقن وسم `<script src="chrome-extension://...">` بشكل أعمى من `content.js` دون فحص ما إذا كان السكربت يعمل مسبقاً عبر `world: "MAIN"` في `manifest.json`، ودون مراعاة المواقع ذات الـ CSP الصارم كـ YouTube والأطر المعزولة:
  ```javascript
  // خطأ: يطلق Unsafe attempt to load URL ... Domains, protocols and ports must match على YouTube
  const s = document.createElement("script");
  s.src = chrome.runtime.getURL("media-hook.js");
  document.documentElement.appendChild(s);
  ```

* **النمط الصحيح (Approved Pattern)**:
  1. جعل سكربت الـ MAIN world يضع راية تأكيد على الـ DOM المشترك (`document.documentElement.setAttribute("data-stormget-media-hook", "active")`).
  2. في الـ content script، فحص وجود الراية قبل أي محاولة حقن احتياطية، وتخطي الحقن تماماً على المواقع التي تعتمد `world: "MAIN"` وتمنع وسوم السكربت كـ YouTube:
  ```javascript
  const isHookActive = document.documentElement?.getAttribute("data-stormget-media-hook") === "active";
  const isYoutube = /(^|\.)youtube\.com$/i.test(location.hostname || "");
  if (!isHookActive && !isYoutube && location.origin !== "null") {
    // حقن احتياطي فقط للمتصفحات القديمة التي تفتقر لدعم world: "MAIN"
  }
  ```

---

## 5. عرض أسماء الملفات ومسارات النظام العربية في واجهات Rust بدون تشويه (egui Arabic Reshaping)

* **النمط الخاطئ (Anti-pattern)**:
  تمرير أسماء الملفات ومسارات القرص الديناميكية وسجلات الأحداث مباشرة إلى `RichText::new(&task.filename).monospace()` مما يؤدي لانقلاب الحروف وتقطعها لأن `egui` يفتقر لمحرك HarfBuzz ويعتمد الرسم الأفقي LTR:
  ```rust
  // خطأ: تظهر الحروف مقلوبة ومفصولة ط ي ق ي ر ط
  ui.label(RichText::new(&task.filename).monospace());
  ui.label(RichText::new(final_path.display().to_string()).monospace());
  ```

* **النمط الصحيح (Approved Pattern)**:
  1. معالجة أسماء الملفات والسجلات عبر `shape_arabic()` لتحويلها إلى أشكال العرض المتصلة وعكسها للسياق LTR.
  2. استخدام دالة مخصصة لمسارات الملفات `shape_file_path()` لتعريب أجزاء المسار دون عكس محركات الأقراص والفواصل (`C:\...`).
  3. استخدام الخطوط المتناسبة (`Proportional`) بدلاً من `monospace` لتتصل الحروف بانسيابية:
  ```rust
  ui.label(RichText::new(super::arabic::shape_arabic(&task.filename)));
  ui.label(RichText::new(super::arabic::shape_file_path(&final_path)));
  ```

---

## 6. تمرير معاملات Windows Explorer لتحديد الملفات وتجنب خطأ التنصيص المزدوج (`raw_arg`)

* **النمط الخاطئ (Anti-pattern)**:
  تمرير وسائط سطر الأوامر التي تحتوي على مفاتيح ومسارات مقتبسة عبر `Command::arg("/select,\"path\"")`. في نظام ويندوز، تقوم مكتبة Rust القياسية بتغليف الوسيط بأكمله داخل علامات تنصيص إضافية مما يفسد مفتاح `/select,` فيتجاهله Explorer ويفتح مجلد البرنامج الافتراضي:
  ```rust
  // خطأ: ينتج عنه explorer "/select,\"C:\path\file.mp4\"" فيفتح مجلد التطبيق بدلاً من مجلد التحميل
  let arg = format!("/select,\"{}\"", path.display());
  Command::new("explorer").arg(arg).spawn();
  ```

* **النمط الصحيح (Approved Pattern)**:
  استخدام الامتداد الخام المباشر لنظام Windows: `std::os::windows::process::CommandExt::raw_arg` لتمرير المعامل بالضبط كما يفهمه Windows Explorer:
  ```rust
  use std::os::windows::process::CommandExt;
  Command::new("explorer")
      .raw_arg(format!("/select,\"{}\"", path.display()))
      .spawn();
  ```

---

## 7. دورة حياة نوافذ التنزيل المستقلة (قبل التحميل، أثناء التحميل، والاكتمال)

* **النمط الخاطئ (Anti-pattern)**:
  1. بدء التنزيل فوراً من نقطة نهاية الاعتراض `/api/v1/intercept` في الخلفية متجاوزاً قائمة `pending_intercepts` ونافذة بدء التحميل.
  2. تنزيل البث أو الفيديو عبر أدوات خارجية (مثل yt-dlp) في مهمة فرعية دون مراقبة مستمرة لنمو حجم الملف وتحديث السرعة الحية في `meta`.
  3. إنهاء التنزيل دون إظهار نافذة الاكتمال مع أزرار تشغيل الملف أو كشفه في المجلد.

* **النمط الصحيح (Approved Pattern)**:
  1. وضع التنزيل في قائمة `pending_intercepts` وإيقاظ نافذة سطح المكتب لعرض خيارات الحفظ وبدء التحميل.
  2. فور بدء التنزيل، فتح نافذة التقدم المنفصلة تلقائياً وتشغيل مراقب تدفق حي للملف (`progress_monitor`) لتحديث نسبة الإنجاز والسرعة كل 250ms.
  3. عند وصول التحميل إلى `Completed`، إظهار نافذة الاكتمال المستقلة فوراً (`draw_completion_viewport`) مع أزرار: فتح الملف، فتح المجلد، وإغلاق.

---

## 8. محاذاة اتجاه اليمين لليسار (RTL) في واجهات egui والشبكات ثنائية الأعمدة

* **النمط الخاطئ (Anti-pattern)**:
  1. الاعتماد على `egui::Grid` بتوزيع عمودي ثابت (العمود 0 للعنوان والعمود 1 للقيمة) في الواجهات العربية، مما يثبت العناوين على اليسار والقيم على اليمين.
  2. عكس مصفوفة أعمدة الجدول `visible_cols.reverse()` لعكس الجدول عربياً، مما يؤدي لدفع عمود اسم الملف خارج العرض المتاح واختفائه.
  3. تفكيك امتدادات الملفات والرموز اللاتينية المتصلة (`256p.mp4`) في المحلل وعكسها حرفياً لتصبح `mp4.256p`.

* **النمط الصحيح (Approved Pattern)**:
  1. إنشاء دالة سطرية مرنة `show_row` تعكس ترتيب إدراج خلايا الشبكة: عندما تكون اللغة عربية (`is_rtl`)، يتم إدراج القيمة في العمود 0 (يسار) والعنوان في العمود 1 (يمين) مع تطبيق `Layout::right_to_left(Align::Center)`.
  2. الإبقاء على ترتيب أعمدة الجدول ثابتاً بحيث يظل اسم الملف هو العمود الأول دائماً في كافة اللغات.
  3. دمج مقاطع الأحرف اللاتينية المتصلة برموز ترقيم (`.`، `_`، `-`) في مقطع LTR واحد لا يقبل العكس الداخلي.

---

## 9. تطبيق حارس النسخة الواحدة الموثوق (Single Instance Enforcement) وإيقاظ النافذة القائمة

* **النمط الخاطئ (Anti-pattern)**:
  عند فحص الخادم المحلي واكتشاف أنه يعمل، إرسال إشارة `/desktop/show` ثم إعادة `Ok(())` في دالة بدء المحرك؛ مما يجعل عملية الواجهة (`run()`) تعتقد أن المحرك قد بدأ للتو فتقوم بتشغيل نافذة ثانية وإنشاء أيقونة Tray إضافية.

* **النمط الصحيح (Approved Pattern)**:
  1. جعل دالة بدء المحرك تعيد تعداداً صريحاً `EngineStartStatus::AlreadyRunning` عند وجود نسخة نشطة مسبقاً.
  2. إرسال أمر تنشيط النافذة للنسخة القائمة واستدعاء `bring_existing_window_to_front()` عبر Windows API لاستعادة النافذة والتركيز عليها فوراً.
  3. إنهاء العملية الجديدة فوراً بإرجاع `Ok(())` من `run()` دون استدعاء محرك الرسوميات أو إنشاء أيقونة Tray مكررة.

---

## 10. استقلالية ميزات الـ UI في Cargo وتشكيل الجمل الرقمية العربية (Cargo Feature Isolation & BiDi Numbers)

* **النمط الخاطئ (Anti-pattern)**:
  1. استخدام دوال من ميزات مخصصة (مثل `reqwest::blocking` التابعة لـ `native-ui`) داخل كود `slint-ui` دون تفعيلها في خيارات `Cargo.toml`.
  2. محاولة عكس النصوص والأرقام يدوياً كـ `"({}(ﺖﻳﺎﺑ "` وعرضها بخط `monospace`، مما يعطل محرك التشكيل التلقائي ويؤدي لظهور أقواس مقلوبة وحروف مبعثرة.

* **النمط الصحيح (Approved Pattern)**:
  1. الاعتماد على مكتبات Rust القياسية الخفيفة (مثل `std::net::TcpStream`) للمهام البسيطة لتقليل الاعتماديات، وتفعيل الميزات المشتركة بوضوح في `Cargo.toml`.
  2. صياغة النص العربي الطبيعي كجملة كاملة (`"{val} ({bytes} بايت)"`)، وتمريره لمحرك التشكيل `shape_arabic()` مع الخطوط المتناسبة لضمان ترابط الحروف والأقواس بدقة.

---

## 11. رصد الروابط الديناميكية واعتراض رسائل أخطاء النظام في شريط الحالة (Dynamic Links & OS Error Shaping)

* **النمط الخاطئ (Anti-pattern)**:
  1. قصر اعتراض الروابط على فحص الامتداد في نهاية المسار `pathname` فقط، مما يفقد القدرة على التقاط روابط المنصات السحابية ومخرجات CI (مثل GitHub Actions Artifacts و Google Drive).
  2. عرض نصوص أخطاء النظام المدمجة (التي تحتوي على خليط عربي وإنجليزي مثل `تعذر حفظ ... (os error 112)`) مباشرة دون تمريرها على `shape_arabic()` مما يسبب انقلاب الحروف العربية.

* **النمط الصحيح (Approved Pattern)**:
  1. فحص أنماط الروابط الديناميكية (`/artifacts/\d+`، معاملات التصدير والتنزيل) في كل من `content.js` و `background.js` مع التقاط الرابط النهائي الموقّع `finalUrl`.
  2. تمرير أي نص خطأ يحتوي على محارف عربية عبر `shape_arabic()` قبل رسمه في شريط الحالة لضمان وضوح وقراءة رسائل التحذير بانسيابية.

---

## 12. تجنب التمدد الرأسي في egui وتشكيل أسماء الملفات العربية في كافة اللغات (Layout Vertical Stretch & Ubiquitous Arabic Shaping)

* **النمط الخاطئ (Anti-pattern)**:
  1. استخدام `ui.with_layout(Layout::left_to_right(Align::Center), ...)` مباشرة على الـ `ui` الرئيسي داخل `CentralPanel`؛ مما يجعل egui يوسع الحاوية لتشمل كامل ارتفاع النافذة المتاح ويدفع كل العناصر التالية خارج أسفل النافذة.
  2. ربط استدعاء `shape_arabic()` بشرط `if is_rtl` فقط؛ فإذا كان التطبيق باللغة الإنجليزية واسم الملف يحتوي على كلمات عربية، تُعرض الحروف مفصولة ومقطعة.

* **النمط الصحيح (Approved Pattern)**:
  1. استخدام `ui.horizontal` للترويسات و `ui.horizontal_wrapped` للتبويبات لحصر ارتفاع السطر ومنع التمدد الرأسي.
  2. تشكيل أسماء الملفات العربية دائماً عبر `shape_arabic()` بغض النظر عن لغة واجهة المستخدم.

---

## 13. دعم رموز الخطوط الشاملة وتفادي محارف الـ Emoji Variation Selectors (Unicode Glyphs & Emoji Selectors)

* **النمط الخاطئ (Anti-pattern)**:
  1. استخدام محارف الإيموجي ذات التحديد الثنائي (`\u{FE0F}`) كـ `"☀️"` و `"🌙"` دون تحميل خط الرموز، مما يجعل الخط الأساسي يفشل في رسم المحرف الثاني فيرسمه كمربع `▢`.
  2. الاعتماد على خط نظام واحد للنصوص فقط (`segoeui.ttf`) دون تضمين خط الرموز المستقل (`seguisym.ttf`).

* **النمط الصحيح (Approved Pattern)**:
  1. تحميل خط `seguisym.ttf` كـ fallback font ضمن عائلات الخطوط في `egui`.
  2. استخدام الرموز الأحادية الخالصة (`\u{2600}` للشمس و `\u{263E}` للهلال) لتجنب أخطاء محارف التحديد التعبيري المفقودة.







