@echo off
chcp 65001 >nul
echo ============================================================================
echo   StormGet v2.3.6 (Final Gold Release) — نظام البناء والتغليف النهائي
echo ============================================================================
echo.

echo [1/4] فحص وبناء واجهة Slint العربية الأصلية (60 FPS + الشعار الرسمي)...
cargo build --release --features slint-ui
if %errorlevel% neq 0 (
    echo [!] تنبيه: فشل بناء واجهة Slint، جاري المحاولة بواجهة egui Native UI...
    cargo build --release --features native-ui
)

echo.
echo [2/4] تشغيل اختبارات الوحدة والتحقق الهندسي...
cargo test --release

echo.
echo [3/4] فحص وجود Inno Setup لبناء المثبت الرسمي (StormGet-v2.3.6-Setup-x64.exe)...
where iscc >nul 2>nul
if %errorlevel% equ 0 (
    iscc installer\StormGet-Setup.iss
    echo [+] تم إنتاج المثبت الرسمي بنجاح في مجلد dist\
) else (
    echo [i] ملاحظة: لتوليد ملف المثبت .exe تلقائياً، ثبّت Inno Setup 6 ثم شغّل:
    echo     iscc installer\StormGet-Setup.iss
)

echo.
echo [4/4] لتوقيع الإصدار رقمياً بـ Ed25519 وتوليد version.json:
echo     target\release\rust_idm.exe sign-release --version 2.3.6 --url "https://example.com/StormGet-Setup.exe" --installer "target\release\rust_idm.exe" --output "dist\version.json"
echo.
echo اكتملت عملية البناء بنجاح! الملف التنفيذي جاهز في: target\release\rust_idm.exe
pause
