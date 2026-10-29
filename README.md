# التقرير الهندسي الشامل ووثيقة المعمارية التنفيذية: StormGet v2.3.4 (Final Gold Release)

توثق هذه الوثيقة المعمارية الكاملة لبرنامج **StormGet v2.3.4** (المعروف داخلياً باسم الحزمة `rust_idm`)، وهو مدير ومسرّع تنزيلات سطح مكتب احترافي مبني بالكامل بلغة **Rust**، يجمع بين واجهتي سطح مكتب كاملتين (**Slint UI** العربية الأصلية و **egui Native UI**)، ومحرك تقسيم ديناميكي نصفي تكيفي (`AdaptiveController 4 → 16 → 32`)، ودمج الشبكات المتعددة (`Multi-Network Lane Bonding`)، وتكامل المتصفح واستخراج تدفقات يوتيوب (`SABR poToken` و `HLS AES-128` و `MPEG-DASH`)، ودعم التورنت (`BitTorrent / magnet:?`)، والتحديث الذاتي الموقّع رقمياً بـ `Ed25519`.

## 1. أبرز المزايا المعمارية والأمنية في الإصدار v2.3.4

1. **حصانة أمنية كاملة لخادم الاتصال المحلي (`src/api/routes.rs` & `src/ipc/native_host.rs` & `chrome-extension/background.js`)**:
   - توليد رمز جلسة عشوائي مشفر بطول 256-بت (`bridge.json` بصلاحيات `0600`) عند بدء المحرك والتحقق منه في زمن ثابت (`constant_time_token_eq`).
   - تسليم الرمز السري لإضافة المتصفح عبر قناة `Chrome Native Messaging` المحمية بنظام التشغيل (`PING`) وإلزام `Authorization: Bearer <bridge_token>` على **100% من طلبات المنفذ المحلي `16888`** عند تفعيل الرمز، بحيث لا تُقبل ترويسة `X-RustIDM: 1` الثابتة وحدها مطلقاً حتى مع تزوير `Origin`.
   - تنفيذ أوامر ما بعد الطابور (`PostQueueAction::CustomCommand`) عبر التحقق الصارم من المسار التنفيذي (`validate_safe_executable_path`) وتشغيله مباشرة كعنصر واحد مع عزل `Stdio::null()` ودون تقسيم على المسافات لمنع حقن المعاملات (`Argument Injection`) ولدعم المسارات التي تحتوي على مسافات.
2. **محرك التقسيم الديناميكي التكيفي والكتابة الدفعية (`src/core/segmenter.rs` & `src/core/adaptive.rs` & `src/core/lanes.rs`)**:
   - **الملفات المتناثرة والكتابة الدفعية (`SegmentWriteCoalescer`)**: تفعيل `FSCTL_SET_SPARSE` على ويندوز وتجميع شرائح الشبكة الصغيرة في مخزن `256 KB` لكل قطاع قبل استدعاء `write_at` لتقليل استدعاءات القرص العشوائية بمقدار 16x–32x.
   - **المتحكم التكيفي بالاتصالات (`AdaptiveController`)**: مراقبة السرعة لحظياً ومضاعفة الاتصالات تلقائياً طالما تحقق نمواً حقيقياً، والتراجع الفوري إلى النصف عند استلام `HTTP 429 / 503` (`on_throttled`).
   - **دمج الشبكات المتعددة (`MultiNetworkLanePool`)**: توزيع طلبات `Range` الخاصة بالملف الواحد عبر عدة بطاقات شبكة (`Wi-Fi` + `Ethernet` + `USB Tethering`) مع عزل المسارات البطيئة أو المتعثرة تلقائياً.
3. **سلامة الاستئناف والتحقق التلقائي من بصمة الناشر (`src/core/resume_verify.rs` & `src/core/publisher.rs`)**:
   - أخذ عينات تحقق (`Sample-Based Resume Verification`) من الملف الجزئي ومطابقتها مع الخادم عند غياب `ETag`، وحساب بصمة `SHA-256` لكل قطاع مكتمل لإصلاح التلف الجزئي وحده دون إعادة تحميل الملف بالكامل.
   - البحث التلقائي عن ملفات `SHA256SUMS` و `SHA2-256SUMS` و `<filename>.sha256` الرسمية من موقع الناشر للتحقق التلقائي من سلامة التوزيعات والبرامج.
4. **واجهتا سطح مكتب متكاملتان (`Slint UI` + `egui Native UI`) وتكامل ويندوز (`src/core/winfx.rs`)**:
   - **واجهة `Slint` (`--features slint-ui`)**: دعم أصلي لتشكيل ومحاذاة اللغة العربية (`RTL / BiDi`) مع نوافذ نظام تشغيل مستقلة واتصال مباشر بالمحرك في الذاكرة (`Zero-HTTP Polling`).
   - **واجهة `egui` (`--features native-ui`)**: جدول افتراضي سريع (`show_rows`)، تحديد متعدد (`Ctrl / Shift / Ctrl+A`)، مخطط سرعة وخريطة مقاطع مرسومة بـ `egui::Painter`، مع تخزين ذكي للمظهر لمنع إعادة بناء الخطوط في كل إطار.
   - **شريط تقدم شريط مهام ويندوز (`ITaskbarList3`)، ووضع الألعاب (`🎮 Game Mode`)، والمعاينة الحية أثناء التحميل (`🎬 Play Preview`)، وطبيب التحميل (`🩺 Download Doctor`)**.
5. **خلو كامل من `unwrap()` في المشروع وتصفير البذرة السرية في الذاكرة**:
   - تم استبدال جميع استدعاءات `.unwrap()` بمعالجة أخطاء آمنة واسترداد الأقفال المسمومة (`unwrap_or_else(|p| p.into_inner())`)، مع تصفير البذرة السرية لمفتاح التوقيع (`write_volatile` Zeroization) فور استخدامها.

## 2. البناء والتشغيل والتوقيع الرقمي (Build, Run & Signing)

```powershell
# 1. تشغيل البناء التلقائي الشامل على ويندوز (يبني Slint UI ويختبر المحرك وينشئ المثبت):
.\BUILD.bat

# 2. بناء وتشغيل واجهة Slint العربية الأصلية السلسة (مباشرة في الذاكرة):
cargo run --release --features slint-ui

# 3. بناء وتشغيل واجهة egui الكلاسيكية المحسنة:
cargo run --release --features native-ui

# 4. تشغيل المحلل الدلالي الثابت وحزمة اختبارات التحقق الهندسي الشاملة:
python scripts/rust_static_analyzer.py
cargo test --release -- --nocapture
cargo run --release -- verify

# 5. توقيع إصدار جديد رقمياً بـ Ed25519 وتوليد version.json و SHA256SUMS و SHA2-256SUMS بأمان
# (يُقرأ المفتاح الخاص من متغير البيئة STORMGET_SIGNING_SEED_HEX أو --key-file مع تصفيره من الذاكرة دون طباعته):
$env:STORMGET_SIGNING_SEED_HEX = "<64-hex-private-seed>"
cargo run --release -- sign-release --version 2.3.4 --url "https://github.com/StormGet/StormGet/releases/download/v2.3.4/StormGet-Setup.exe" --installer "target/release/rust_idm.exe" --output "dist/version.json"
```
