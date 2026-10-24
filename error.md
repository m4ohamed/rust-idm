# 📝 سجل الأخطاء والحلول (Error Log & Fixes)

---

### 1. نقص حقول في مهيّئ `InterceptDownloadPayload` (Lines: 5069, 6579, 6675)

* **الخطأ (The Bug)**:
  ```text
  missing fields `description`, `expected_crc32`, `expected_md5` and 1 other field in initializer of `InterceptDownloadPayload`
  ```
  في الأسطر 5069 و 6579 و 6675 في ملف [main.rs](file:///c:/Users/m4oha/OneDrive/Desktop/rust-idm-v2/src/main.rs).

* **السبب الجذري (Root Cause)**:
  تمت إضافة حقول جديدة (`description`, `expected_crc32`, `expected_md5`, `mirror_urls`) لنموذج `InterceptDownloadPayload` الذي يطبّق `Default`، ولكن بعض الأماكن في الكود كانت تنشئ الكائن يدوياً دون تمرير الحقول الجديدة أو استخدام `..Default::default()`.

* **الحل الفعلي (The Fix)**:
  إضافة `..Default::default()` في مهيئات الكائن لملء أي حقول اختيارية افتراضياً تلقائياً بأمان:
  ```rust
  InterceptDownloadPayload {
      url: payload.url,
      source_page_url: payload.page_url,
      filename: Some(filename),
      cookies: payload.cookies,
      referer: payload.referer,
      user_agent: payload.user_agent,
      queue_name: Some("main".to_string()),
      expected_sha256: None,
      ..Default::default()
  }
  ```

---

### 2. خطأ نحوي في نص الترويسة بـ `HeaderValue` داخل الاختبار (Line: 6898)

* **الخطأ (The Bug)**:
  ```text
  suffixes on string literals are invalid: invalid suffix fallback
  prefix zip is unknown
  expected one of (, ), ,, ., ::, ?, or an operator
  this function takes 1 argument but 2 arguments were supplied
  ```
  في السطر 6898 في ملف [main.rs](file:///c:/Users/m4oha/OneDrive/Desktop/rust-idm-v2/src/main.rs).

* **السبب الجذري (Root Cause)**:
  عدم الهروب (escaping) من علامات التنصيص المزدوجة المحيطة بـ `fallback.zip` داخل السلسلة النصية:
  `"attachment; filename="fallback.zip"; filename*=..."`، مما جعل المحلل النحوي لـ Rust يغلق النص مبكراً ويعتبر `fallback` لاحقة غير صالحة.

* **الحل الفعلي (The Fix)**:
  إضافة الهروب المائل `\"` لعلامات التنصيص الداخلية:
  ```rust
  HeaderValue::from_static(
      "attachment; filename=\"fallback.zip\"; filename*=UTF-8''%D9%85%D9%84%D9%81_%D8%AA%D8%AC%D8%B1%D9%8A%D8%A8%D9%8A%202026.zip",
  )
  ```

---

### 3. تسمية المتغير مخالفة لمعايير Rust `non_snake_case` (Line: 2649)

* **الخطأ (The Bug)**:
  ```text
  variable `step_Override` should have a snake case name
  ```
  في السطر 2649 في ملف [main.rs](file:///c:/Users/m4oha/OneDrive/Desktop/rust-idm-v2/src/main.rs).

* **السبب الجذري (Root Cause)**:
  تسمية المتغير بحرف كبير `step_Override` بدلاً من نمط `snake_case` المعتمد في Rust.

* **الحل الفعلي (The Fix)**:
  تحويل اسم المتغير ومواضع استخدامه إلى `step_override`:
  ```rust
  let step_override = default_step.max(1);
  // ...
  .unwrap_or(step_override)
  ```

---

### 4. تعارض استعارة `self` (Borrow Checker E0506) في `native_ui.rs` (Line: 528)

* **الخطأ (The Bug)**:
  ```text
  error[E0506]: cannot assign to `*self.refresh_task_id` because it is borrowed
   --> src\native_ui.rs:528:33
    |
  527 | ... if let Some(task) = self.selected_download() {
    |                         ------------------------- `*self.refresh_task_id` is borrowed here
  528 | ...     self.refresh_task_id = task.task_id.clone();
    |         ^^^^^^^^^^^^^^^^^^^^ `*self.refresh_task_id` is assigned to here but it was already borrowed
  529 | ...     self.refresh_new_url = task.url.clone();
    |                                ---- borrow later used here
  ```

* **السبب الجذري (Root Cause)**:
  دالة `self.selected_download()` تستعير `&self` بصورة غير قابلة للتعديل (Immutable Borrow) ويبقى المرجع `task` حياً عبر السطور 527-529. محاولة إسناد قيمة لحقل `self.refresh_task_id` تتطلب استعارة قابلة للتعديل (`&mut self`)، مما يتعارض مع وجود استعارة نشطة لـ `self` في نفس الوقت.

* **الحل الفعلي (The Fix)**:
  استخراج واستنساخ القيم المطلوبة فقط (`task_id`, `url`) عبر `.map(...)` وإنهاء استعارة `self` فورياً قبل تنفيذ الإسناد:
  ```rust
  // قبل التعديل:
  if let Some(task) = self.selected_download() {
      self.refresh_task_id = task.task_id.clone();
      self.refresh_new_url = task.url.clone();
      self.show_refresh_dialog = true;
  }

  // بعد التعديل:
  if let Some((task_id, url)) = self
      .selected_download()
      .map(|task| (task.task_id.clone(), task.url.clone()))
  {
      self.refresh_task_id = task_id;
      self.refresh_new_url = url;
      self.show_refresh_dialog = true;
  }
  ```

---

### 5. عدم اعتراض ملفات ZIP والتحميلات الديناميكية في إضافة Chrome (chrome-extension)

* **الخطأ (The Bug)**:
  إضافة المتصفح لا ترسل ملفات ZIP أو تلتقط الملفات عند محاولة تحميلها، ويقوم المتصفح بتنزيل الملفات عبر مديره الداخلي المعتاد.

* **السبب الجذري (Root Cause)**:
  1. دالة `hasInterceptableExtension(url)` كانت تفحص فقط نهاية مسار الرابط `pathname`. عند التحميل من مواقع الويب الحديثة (مثل Google Drive, GitHub Releases, MediaFire, PHP Scripts)، يكون الرابط ديناميكياً بدون امتداد في المسار أو يحتوي على معلمات استعلام (`?file=...`) فيتم رفضه وتجاهله فوراً.
  2. الاعتماد الحصري على حدث `chrome.downloads.onCreated`؛ في هذا الحدث يكون حقل `downloadItem.filename` فارغاً `""` لأن المتصفح لم يستلم بعد ترويسات `Content-Disposition` من السيرفر.
  3. غياب الاستماع لحدث `chrome.downloads.onDeterminingFilename` الذي يملأ الاسم الفعلي ونوع الوسائط MIME ويسمح بإلغاء تحميل المتصفح بأمان.
  4. غياب مراقب النقر على وسوم الروابط `<a>` في صفحة الويب (`content.js`).

* **الحل الفعلي (The Fix)**:
  1. ترقية دالة فحص الامتداد لتفحص اسم الملف الفعلي `filename`، ومعاملات الاستعلام `searchParams`، ونوع المحتوى `mime`.
  2. الاستماع لحدث `chrome.downloads.onDeterminingFilename` لاعتراض التنزيلات بعد فك الترويسات وحذفها من المتصفح مع منع التكرار (`interceptedDownloadIds`).
  3. إضافة مراقب نقرات الروابط `<a>` في `content.js` لاعتراض روابط الملفات المباشرة فوراً مع إتاحة زر `Alt` لتجاوز الاعتراض عند الرغبة.
