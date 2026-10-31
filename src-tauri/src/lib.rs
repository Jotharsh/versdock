// Versdock — synced lyrics that float over your desktop.
// Core wiring: playback clock (managed state), commands for the overlay,
// global hotkey for the click-through lock, demo source thread.

pub mod lyrics;
pub mod media;
pub mod playback_clock;
pub mod settings;
pub mod window_level;

use playback_clock::SharedClock;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Manager, State};

/// Click-through lock: while true the overlay ignores the cursor entirely
/// (so it never eats your clicks). Toggled here because the shared flag
/// must live across both the hotkey and any future tray button.
pub struct LockState(pub AtomicBool);

#[tauri::command]
fn get_timeline() -> Vec<lyrics::lrc::LrcLine> {
    lyrics::timeline().to_vec()
}

/// Offsetting = what the *UI* shows: offset-corrected position.
#[tauri::command]
fn get_state(clock: State<SharedClock>) -> playback_clock::MediaState {
    let c = playback_clock::guard(&clock);
    playback_clock::MediaState {
        position: c.position(), // UI ticks are offset-corrected too — consistent
        ..c.state()
    }
}

#[tauri::command]
fn toggle_play(clock: State<SharedClock>) -> bool {
    let mut c = playback_clock::guard(&clock);
    c.toggle_play();
    c.playing()
}

/// Nudge the sync by ±0.5 s (persisted per track).
#[tauri::command]
fn bump_offset(clock: State<SharedClock>, delta: f64) -> f64 {
    let mut c = playback_clock::guard(&clock);
    let off = c.bump_offset(delta);
    let s = c.state();
    if !s.title.trim().is_empty() {
        settings::record_offset(&s.title, &s.artist, off);
    }
    off
}

/// The offset currently active on the clock (shown in the overlay).
#[tauri::command]
fn get_offset(clock: State<SharedClock>) -> f64 {
    let c = playback_clock::guard(&clock);
    c.offset()
}

/// Toggle login-at-startup (free macOS LaunchAgent / Windows registry key
/// via the official plugin; no daemon, no services).
#[tauri::command]
fn toggle_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    let al = app.autolaunch();
    let is_on = al.is_enabled().unwrap_or(false);
    let _ = if is_on { al.disable() } else { al.enable() };
    al.is_enabled().unwrap_or(false)
}

/// Toggle the widget living *on the desktop* (behind app windows) instead
/// of always on top. Persisted, re-applied on launch.
#[tauri::command]
fn toggle_desktop_pin(app: tauri::AppHandle) -> bool {
    toggle_pin(&app)
}

fn toggle_pin(app: &tauri::AppHandle) -> bool {
    let mode = !settings::pin_desktop();
    settings::record_pin_desktop(mode);
    apply_window_mode(app, mode);
    mode
}

fn apply_window_mode(app: &tauri::AppHandle, desktop: bool) {
    if let Some(w) = app.get_webview_window("overlay") {
        if let Some(update_mode) = w.ns_window().ok() {
            unsafe {
                window_level::apply_desktop_pin(update_mode, desktop);
            }
            #[cfg(target_os = "windows")]
            let _ = w.set_always_on_top(!desktop);
            #[cfg(target_os = "macos")]
            let _ = &w;
        }
    }
}

/// The toolbar lock button clicks into click-through too (the hotkey
/// ⌘⇧L still unlocks — the overlay can't be clicked in this mode).
#[tauri::command]
fn set_locked(app: tauri::AppHandle) -> bool {
    toggle_lock(&app);
    let ctl = app.state::<LockState>();
    ctl.0.load(Ordering::Relaxed)
}

/// Sheet-row boot states.
#[tauri::command]
fn get_pin(_app: tauri::AppHandle) -> bool {
    settings::pin_desktop()
}

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .manage(playback_clock::SharedClock::new(
            playback_clock::PlaybackClock::new(playback_clock::MediaState {
                source: "idle".into(),
                title: String::new(),
                artist: String::new(),
                duration: 0.0,
                position: 0.0,
                playing: false,
            }),
        ))
        .manage(LockState(AtomicBool::new(false)))
        .invoke_handler(tauri::generate_handler![
            get_timeline,
            get_state,
            toggle_play,
            bump_offset,
            get_offset,
            toggle_autostart,
            toggle_desktop_pin,
            set_locked,
            get_pin,
            get_autostart
        ])
        // closing the overlay hides it instead of quitting — the app lives
        // in the tray; Quit is a tray action (typical dock-app behaviour)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .setup(|app| {
            lyrics::cache::init(app.handle());
            settings::init(app.handle());

            // ⌘⇧L — global hotkey to lock/unlock the overlay. Defined on the
            // Rust side because a click-through overlay can't click anything.
            use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
            app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+L", |app, _s, event| {
                if event.state == ShortcutState::Pressed {
                    toggle_lock(app);
                }
            })?;

            // ── tray: the widget's always-on-home ──────────────────
            use tauri::{
                menu::{Menu, MenuItem},
                tray::TrayIconBuilder,
            };
            let show = MenuItem::with_id(app, "show", "Show overlay", true, None::<&str>)?;
            let lock = MenuItem::with_id(app, "lock", "Lock overlay (⌘⇧L)", true, None::<&str>)?;
            let pin = MenuItem::with_id(app, "pin", "Pin to desktop ↔ on top", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit Versdock", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &lock, &pin, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().expect("bundle icon"))
                .tooltip("Versdock — synced lyrics")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("overlay") {
                            let _ = w.show();
                        }
                    }
                    "lock" => toggle_lock(app),
                    "pin" => {
                        toggle_pin(app);
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;

            media::start_source(app.handle().clone());

            // restore the persisted window mode (desktop pin)
            apply_window_mode(app.handle(), settings::pin_desktop());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Shared by the global hotkey and the tray menu item.
fn toggle_lock(app: &tauri::AppHandle) {
    use tauri::Manager;
    let ctl = app.state::<LockState>();
    let now = !ctl.0.load(Ordering::Relaxed);
    ctl.0.store(now, Ordering::Relaxed);
    if let Some(w) = app.get_webview_window("overlay") {
        let _ = w.set_ignore_cursor_events(now);
    }
}
