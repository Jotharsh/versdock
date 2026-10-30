// lyrics/mod.rs — track → timeline pipeline.
//
// Milestone 1 ships the **demo track**: a fake player + hardcoded LRC so
// the overlay is fully testable without any media integration. Doubles as
// the "no synced lyrics for this track" fallback screen content.
//
// Milestone 3 replaces the demo source with the Swift MediaRemote sidecar;
// lrclib.rs (fetch) and cache.rs (per-track disk cache) slot in here.

pub mod lrc;

use crate::playback_clock::{guard, MediaState, SharedClock};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Demo song ("Neon Between the Lines" — written for Versdock, not a real
/// track) with an original 3:42-style timeline. Mutate freely while tuning.
pub const DEMO_TITLE: &str = "Neon Between the Lines";
pub const DEMO_ARTIST: &str = "Versdock Demo";

pub const DEMO_DURATION: f64 = 222.0;

pub const DEMO_LRC: &str = r#"[ti:Neon Between the Lines]
[ar:Versdock Demo]
[offset:0]
[00:11.00]
[00:12.40]Midnight above the city grid
[00:17.10]The screens all blink in blue
[00:21.80]A neon hum between the lines
[00:26.50]I follow it to you
[00:31.20]
[00:33.50]And the words come up like headlights
[00:38.20]Bathing in the glass
[00:42.90]One line at a time they bloom
[00:47.60]Watch the dark birds pass
[00:52.30]
[00:55.00]Chasing every little second
[00:59.70]That the rhythm keeps
[01:04.40]Over rooftops over rivers
[01:09.10]Over where we sleep
[01:13.80]
[01:16.50]So hold the beat and hold the light
[01:21.20]Let the chorus rise
[01:25.90]Every letter lands in place
[01:30.60]Before your very eyes
[01:35.30]
[01:38.00]And the bridge is coming, hold on
[01:42.70]Hold on, hold on
[01:47.40](hold on, hold on)
[01:52.10]
[01:55.00]Now the quiet, now the storm
[01:59.70]Now the pulse returning
[02:04.40]Every word has found its home
[02:09.10]Every end is turning
[02:13.80]
[02:16.50]Carry me across the night
[02:21.20]To the lettered shore
[02:25.90]Where the music reads itself
[02:30.60]And asks for nothing more
[02:35.30]
[02:38.00]

[03:32.50]
[03:35.00]Neon between the lines
[03:40.00]"#;

static TIMELINE: OnceLock<Vec<lrc::LrcLine>> = OnceLock::new();

pub fn timeline() -> &'static [lrc::LrcLine] {
    TIMELINE.get_or_init(|| lrc::parse(DEMO_LRC))
}

pub fn demo_state(start: f64) -> MediaState {
    MediaState {
        source: "demo".into(),
        title: DEMO_TITLE.into(),
        artist: DEMO_ARTIST.into(),
        duration: DEMO_DURATION,
        position: start,
        playing: true,
    }
}

/// Milestone-1 stand-in for the macOS sidecar / Windows SMTC.
/// Behaves exactly like a real source so the clock's event paths are
/// exercised for real:
///   * fresh **snapshot every 4 s**  → the periodic drift correction
///   * track end → the player "wraps" and we re-anchor like a new track
pub fn start_demo(app: AppHandle) {
    // 30 Hz tick thread: extrapolate the anchor and stream it to the UI.
    let ticker = app.clone();
    thread::spawn(move || loop {
        let clock = ticker.state::<SharedClock>();
        let pos = guard(&clock).position();
        let _ = ticker.emit("playback-tick", pos);
        thread::sleep(Duration::from_millis(33));
    });

    // Snapshot thread: the "media source" (sidecar / SMTC analogue).
    let snapshots = app.clone();
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(4));
        let clock = snapshots.state::<SharedClock>();
        let mut c = guard(&clock);
        if !c.playing() {
            continue; // a paused player sends no events
        }
        let pos = c.raw_position();
        if pos >= DEMO_DURATION {
            // virtual player wrapped to the start of the demo track
            c.resync(demo_state(0.2));
        } else {
            // ordinary drift check: source reports its own truth,
            // clock silently re-anchors (kills accumulated drift)
            c.resync(demo_state(pos));
        }
        let _ = snapshots.emit("media-state", c.state());
    });
}
