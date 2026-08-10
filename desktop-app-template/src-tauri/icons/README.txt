این پوشه باید قبل از build خالی نباشه — Tauri برای بستن (bundle) کردن اپ به این آیکون‌ها نیاز داره:
32x32.png, 128x128.png, icon.icns (مک), icon.ico (ویندوز)

ساده‌ترین راه: یه لوگوی مربعی (حداقل ۱۰۲۴×۱۰۲۴، فرمت PNG) آماده کن و از خود Tauri CLI بخواه
همه‌ی سایزها رو خودش بسازه:

    cargo install tauri-cli --version "^1"
    cd src-tauri
    cargo tauri icon /path/to/logo.png

این دستور به‌طور خودکار همه‌ی فایل‌های بالا رو داخل همین پوشه می‌سازه.
