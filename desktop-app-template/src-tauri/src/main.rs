// This binary does nothing but open a native window that shows the site
// bundled inside `dist/` (which Tauri embeds directly into the executable
// at build time). No network access, no shell access, no filesystem access
// beyond the bundled assets — the whole point is a single portable .exe/.app
// that behaves like a desktop app but is really the cloned website.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running the offline site app");
}
