# سكربت تسجيل RustIDM Native Messaging Host لمتصفحي Google Chrome و Microsoft Edge على نظام Windows
param(
    [string]$ExtensionId = ""
)

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$ProjectDir = Split-Path -Parent $ScriptDir

# البحث عن المسار التنفيذي لبرنامج rust_idm
$ExePath = Join-Path $ProjectDir "target\release\rust_idm.exe"
if (-not (Test-Path $ExePath)) {
    # محاولة البحث في مجلد البرنامج الحالي
    $Candidates = Get-ChildItem -Path $ProjectDir -Filter "rust_idm*.exe" -Recurse -ErrorAction SilentlyContinue | Where-Object { $_.FullName -notmatch "target\\debug" }
    if ($Candidates) {
        $ExePath = $Candidates[0].FullName
    } else {
        $ExePath = Join-Path $ProjectDir "rust_idm.exe"
    }
}

if (-not $ExtensionId) {
    Write-Host "يرجى إدخال معرف إضافة كروم (Extension ID من صفحة chrome://extensions):" -ForegroundColor Cyan
    $ExtensionId = Read-Host "Extension ID"
    if (-not $ExtensionId) {
        Write-Error "يجب توفير Extension ID لإكمال التسجيل."
        exit 1
    }
}

$ManifestPath = Join-Path $ScriptDir "com.rustidm.native_host.json"
$ManifestContent = @{
    name = "com.rustidm.native_host"
    description = "RustIDM Studio Native Messaging Host"
    path = $ExePath
    type = "stdio"
    allowed_origins = @(
        "chrome-extension://$ExtensionId/"
    )
} | ConvertTo-Json -Depth 5

Set-Content -Path $ManifestPath -Value $ManifestContent -Encoding UTF8
Write-Host "تم تحديث ملف: $ManifestPath" -ForegroundColor Green

# تسجيل المفتاح في Windows Registry لمتصفح Chrome
$ChromeRegKey = "HKCU:\Software\Google\Chrome\NativeMessagingHosts\com.rustidm.native_host"
New-Item -Path $ChromeRegKey -Force | Out-Null
Set-ItemProperty -Path $ChromeRegKey -Name "(Default)" -Value $ManifestPath
Write-Host "تم التسجيل بنجاح في ريجستري Google Chrome: $ChromeRegKey" -ForegroundColor Green

# تسجيل المفتاح في Windows Registry لمتصفح Edge
$EdgeRegKey = "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\com.rustidm.native_host"
New-Item -Path $EdgeRegKey -Force | Out-Null
Set-ItemProperty -Path $EdgeRegKey -Name "(Default)" -Value $ManifestPath
Write-Host "تم التسجيل بنجاح في ريجستري Microsoft Edge: $EdgeRegKey" -ForegroundColor Green

Write-Host "`nاكتمل الإعداد بنجاح! مضيف المراسلة جاهز للاستخدام مع الإضافة." -ForegroundColor Cyan
