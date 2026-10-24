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

