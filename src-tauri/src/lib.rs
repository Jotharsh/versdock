// Versdock — synced lyrics that float over your desktop.
// Core wiring: playback clock (managed state), commands for the overlay,
// global hotkey for the click-through lock, demo source thread.

pub mod lyrics;
pub mod media;
pub mod playback_clock;

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

/// Nudge the sync by ±0.5 s (persisted per track later — settings.rs).
#[tauri::command]
fn bump_offset(clock: State<SharedClock>, delta: f64) -> f64 {
    let mut c = playback_clock::guard(&clock);
    c.bump_offset(delta)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
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
            bump_offset
        ])
        .setup(|app| {
            lyrics::cache::init(app.handle());

            // ⌘⇧L — global hotkey to lock/unlock the overlay. Defined on the
            // Rust side because a click-through overlay can't click anything.
            use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
            app.global_shortcut().on_shortcut("CmdOrCtrl+Shift+L", |app, _s, event| {
                if event.state == ShortcutState::Pressed {
                    let ctl = app.state::<LockState>();
                    let now = !ctl.0.load(Ordering::Relaxed);
                    ctl.0.store(now, Ordering::Relaxed);
                    if let Some(w) = app.get_webview_window("overlay") {
                        let _ = w.set_ignore_cursor_events(now);
                    }
                }
            })?;

            media::start_source(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
