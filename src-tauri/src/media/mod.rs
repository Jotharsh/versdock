// media/mod.rs — active-source arbitration.
// Whatever the OS reports as playing wins. Multiple players at once:
// the most recent *playing* session is taken (per DESIGN.md §2); a
// manual pick UI arrives with the settings milestone.

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows_smtc;

use crate::lyrics;

/// Start the platform media source; fall back to the demo player when no
/// platform source exists / fails, so the widget is never dead in dev.
pub fn start_source(app: tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        if !macos::start(app.clone()) {
            lyrics::seed_demo();
            lyrics::start_demo(app);
        }
    }
    #[cfg(target_os = "windows")]
    {
        let ok = windows_smtc::start(app.clone());
        if !ok {
            lyrics::seed_demo();
            lyrics::start_demo(app);
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        lyrics::seed_demo();
        lyrics::start_demo(app);
    }
}
