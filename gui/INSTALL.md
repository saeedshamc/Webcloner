# نصب و اجرای webcloner GUI روی ویندوز

## پیش‌نیاز
- Rust (rustup) و Visual Studio Build Tools / MSVC
- برای PHP و ASP.NET داخلی: یک‌بار از ریشه پروژه:
  ```powershell
  .\scripts\setup-runtimes.ps1
  ```

## ساخت نصب‌کننده
```powershell
cd gui
.\build.ps1
```

خروجی معمولاً اینجاست:
- `gui\src-tauri\target\release\bundle\msi\`
- یا `gui\src-tauri\target\release\bundle\nsis\`
- باینری خام: `gui\src-tauri\target\release\webcloner-gui.exe`

## توسعه
```powershell
cd gui\src-tauri
cargo tauri dev
```

## تست دود (CLI)
```powershell
.\scripts\smoke-test.ps1
```
