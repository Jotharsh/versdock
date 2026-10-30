// media/windows_smtc.rs — Windows adapter (compiled only on Windows; the
// first CI run on windows-latest validates this file — small API-name
// fixes are expected and noted inline). Uses the WinRT System Media
// Transport Controls: the OS session infra every player (Spotify desktop,
// Chrome/YT Music, foobar…) feeds. No private APIs, no sidecar, no login.
//
// Known limitation (DESIGN.md §4): SMTC gives position *snapshots* only —
// no seek, thin metadata for some browser tabs; duration-tolerant LRCLIB
// matching absorbs the thin-metadata case.

use crate::lyrics;
use crate::playback_clock::{self, guard, SharedClock};
use std::sync::atomic::Ordering;
use windows::media::control::{
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};

pub fn start(app: tauri::AppHandle) -> bool {
    // WinRT wants an initialized apartment; run the source loop on a
    // dedicated thread and fall back to demo on any hard setup failure.
    std::thread::Builder::new()
        .name("smtc-source".into())
        .spawn(move || {
            if let Err(e) = run(app.clone()) {
                eprintln!("versdock smtc: {e} — falling back to demo");
                lyrics::seed_demo();
                lyrics::start_demo(app);
            }
        })
        .map(|_| true)
        .unwrap_or(false)
}

fn run(app: tauri::AppHandle) -> windows::core::Result<()> {
    let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()?.get()?;

    let last_track = std::sync::Arc::new(std::sync::Mutex::new(String::new()));

    loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Pick the session that's Playing; SMTC exposes every media
        // app's session, so "whichever is playing wins" is the rule.
        let sessions = manager.GetSessions()?;
        let chosen = sessions
            .into_iter()
            .find(|s| {
                s.GetPlaybackInfo()
                    .ok()
                    .and_then(|p| p.PlaybackStatus().ok())
                    .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
                    .unwrap_or(false)
            })
            .or_else(|| manager.GetCurrentSession().ok());

        let Some(session) = chosen else {
            emit_none(&app);
            continue;
        };

        let timeline = session.GetTimelineProperties()?;
        let title = session
            .TryGetMediaProperties()
            .and_then(|m| m.Title())
            .map(|t| t.to_string())
            .unwrap_or_default();
        let artist = session
            .TryGetMediaProperties()
            .and_then(|m| m.Artist())
            .map(|a| a.to_string())
            .unwrap_or_default();
        let duration = timeline.EndTime().Duration as f64 / 1e7;
        let position = timeline.Position().Duration as f64 / 1e7;
        let playing = session
            .GetPlaybackInfo()?
            .PlaybackStatus()
            .map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing)
            .unwrap_or(false);

        if title.trim().is_empty() {
            emit_none(&app);
            continue;
        }

        lyrics::REAL_SOURCE_SEEN.store(true, Ordering::Relaxed);
        let state = playback_clock::MediaState {
            source: "windows-smtc".into(),
            title: title.clone(),
            artist: artist.clone(),
            duration,
            position,
            playing,
        };
        {
            let clock = app.state::<SharedClock>();
            guard(&clock).resync(state.clone());
        }
        let _ = app.emit("media-state", state.clone());

        // new track → fetch lyrics (cache-aware, worker thread)
        let key = format!("{}|{}", title, artist);
        let changed = {
            let mut last = last_track.lock().unwrap();
            if *last == key {
                false
            } else {
                *last = key;
                true
            }
        };
        if changed {
            // per-track offset first, then lyrics
            if let Some(o) = crate::settings::offset_for(&title, &artist) {
                let clock = app.state::<SharedClock>();
                guard(&clock).set_offset(o);
            }
            lyrics::load_for_track(
                app.clone(),
                &lyrics::TrackKey {
                    title,
                    artist,
                    duration: state.duration,
                },
            );
        }
    }
}

/// System-wide silence: freeze the clock like a pause.
fn emit_none(app: &tauri::AppHandle) {
    let clock = app.state::<SharedClock>();
    let mut c = guard(&clock);
    if c.playing() {
        let mut s = c.state();
        s.playing = false;
        s.position = c.raw_position();
        c.resync(s);
        let _ = app.emit("media-state", c.state());
    }
}
