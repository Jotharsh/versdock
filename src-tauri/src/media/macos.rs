// media/macos.rs — macOS adapter. Spawns the Swift sidecar
// (sidecar/media-bridge, compiled to a tiny CLI) which reads Apple's
// private MediaRemote framework and prints JSON playback events on
// stdout, one per line:
//   {"event":"state","title":…,"artist":…,"duration":…,"position":…,"playing":…}
//   {"event":"none"}                       ← nothing playing system-wide
// The adapter never blocks the UI thread: it reads lines in a background
// thread and drives the shared playback clock.

use crate::lyrics;
use crate::playback_clock::{self, guard, SharedClock};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};

fn sidecar_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    // 1. Check bundled resource directory (Tauri .app package)
    if let Ok(res) = app.path().resource_dir() {
        let candidates = [
            res.join("sidecar/media-bridge"),
            res.join("sidecar/media-bridge/media-bridge"),
            res.join("media-bridge"),
        ];
        for c in candidates {
            if c.exists() {
                return Some(c);
            }
        }
    }

    // 2. Check next to current executable inside MacOS/
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let next_to_exe = dir.join("media-bridge");
            if next_to_exe.exists() {
                return Some(next_to_exe);
            }
            let res_sidecar = dir.join("../Resources/sidecar/media-bridge");
            if res_sidecar.exists() {
                return Some(res_sidecar);
            }
        }
    }

    // 3. Dev mode: built by sidecar/media-bridge/build.sh next to the Rust sources
    let dev = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../sidecar/media-bridge/media-bridge");
    if dev.exists() {
        return Some(dev);
    }

    None
}

pub fn start(app: AppHandle) -> bool {
    let Some(exe) = sidecar_path(&app) else {
        eprintln!("versdock: media sidecar not found — falling back to demo source");
        return false;
    };

    // Ensure executable permissions on macOS
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&exe) {
            let mut perms = meta.permissions();
            if perms.mode() & 0o111 == 0 {
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&exe, perms);
            }
        }
    }

    let child = match Command::new(&exe)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // sidecar diagnostics flow to the dev log
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("versdock: failed to spawn sidecar at {:?}: {e}", exe);
            return false;
        }
    };

    let app2 = app.clone();
    std::thread::spawn(move || read_sidecar(app2, child));
    true
}

fn read_sidecar(app: AppHandle, mut child: Child) {
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return,
    };
    let reader = BufReader::new(stdout);
    let mut last_track = String::new();

    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        // "none" → system-wide silence: freeze the clock (as if paused)
        if v.get("event").and_then(|e| e.as_str()) == Some("none") {
            let clock = app.state::<SharedClock>();
            let mut c = guard(&clock);
            if c.playing() {
                let mut s = c.state();
                s.playing = false;
                s.position = c.raw_position();
                c.resync(s);
                let _ = app.emit("media-state", c.state());
            }
            continue;
        }

        // parse the state event
        let Ok(state) = serde_json::from_value::<playback_clock::MediaState>(v) else {
            continue;
        };
        let real_track = !state.title.trim().is_empty();
        if real_track {
            // first real state kills the demo threads permanently
            lyrics::REAL_SOURCE_SEEN.store(true, Ordering::Relaxed);
        }

        {
            let clock = app.state::<SharedClock>();
            guard(&clock).resync(state.clone());
        }
        let _ = app.emit("media-state", state.clone());

        // new track (title changed) → load lyrics + apply its stored offset
        if !state.title.trim().is_empty() {
            let this_track = format!("{}|{}", state.title, state.artist);
            if this_track != last_track {
                last_track = this_track;
                {
                    let clock = app.state::<SharedClock>();
                    let mut c = guard(&clock);
                    if let Some(o) = crate::settings::offset_for(&state.title, &state.artist) {
                        c.set_offset(o);
                    }
                }
                lyrics::load_for_track(
                    app.clone(),
                    &lyrics::TrackKey {
                        title: state.title.clone(),
                        artist: state.artist.clone(),
                        duration: state.duration,
                    },
                );
            }
        }
    }
}
