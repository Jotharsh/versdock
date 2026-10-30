// lyrics/mod.rs — track → timeline pipeline: LRCLIB fetch, per-track disk
// cache, LRC parse, and the live timeline the overlay renders from.
// The demo track remains as the fallback when no song is playing / the
// sidecar can't deliver (see media/), so the widget is never blank in dev.

pub mod cache;
pub mod lrc;
pub mod lrclib;
pub mod palette;

use crate::playback_clock::{guard, MediaState, SharedClock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// Written-for-Versdock demo song (original lyrics, 3:42-ish timeline).
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

static TIMELINE: RwLock<Vec<lrc::LrcLine>> = RwLock::new(Vec::new());

/// Demo threads run only until the first real sidecar state arrives.
pub static REAL_SOURCE_SEEN: AtomicBool = AtomicBool::new(false);

pub fn timeline() -> Vec<lrc::LrcLine> {
    TIMELINE.read().map(|t| t.clone()).unwrap_or_default()
}

pub fn set_timeline(lines: Vec<lrc::LrcLine>) {
    if let Ok(mut t) = TIMELINE.write() {
        *t = lines;
    }
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

/// Seed the built-in demo track (startup fallback).
pub fn seed_demo() {
    set_timeline(lrc::parse(DEMO_LRC));
}

/// Load lyrics for a track: cache → LRCLIB → cache (write-through).
/// Runs on a worker thread; the webview is updated via "lyrics-timeline".
pub struct TrackKey {
    pub title: String,
    pub artist: String,
    pub duration: f64,
}

/// Last key we loaded for — guard against duplicate fetch storms.
static LAST_KEY: RwLock<(String, String, i64)> = RwLock::new((String::new(), String::new(), 0));

pub fn load_for_track(app: AppHandle, k: &TrackKey) {
    let title = k.title.trim().to_string();
    let artist = k.artist.trim().to_string();
    let duration = k.duration.max(0.0);
    if title.is_empty() {
        return;
    }
    let dur_i = duration.round() as i64;
    // Same track as before → nothing to do (cross-check is exact strings)
    if let Ok(mut last) = LAST_KEY.write() {
        if last.0 == title && last.1 == artist && last.2 == dur_i {
            return;
        }
        *last = (title.clone(), artist.clone(), dur_i);
    }

    thread::spawn(move || {
        // 1. disk cache
        if let Some(hit) = cache::load(&title, &artist, duration) {
            set_timeline(hit.lines);
            let _ = app.emit("lyrics-timeline", timeline());
            // palette in the same worker thread (art lookup is one HTTP call)
            palette::set_for_track(&app, &title, &artist, duration);
            return;
        }
        // 2. LRCLIB (free, no key); album-art accent fetches on its own
        // thread so a slow art lookup never delays the timeline swap
        {
            let app2 = app.clone();
            let (t2, a2, d2) = (title.clone(), artist.clone(), duration);
            thread::spawn(move || palette::set_for_track(&app2, &t2, &a2, d2));
        }
        match lrclib::fetch(&title, &artist, duration) {
            lrclib::Fetched::Synced(lines, credited) => {
                cache::store(
                    &title,
                    &artist,
                    duration,
                    &cache::CachedLyrics {
                        source: "lrclib-synced".into(),
                        lines: lines.clone(),
                    },
                );
                set_timeline(lines);
                let _ = app.emit("lyrics-meta", credited);
                let _ = app.emit("lyrics-timeline", timeline());
            }
            lrclib::Fetched::Plain(text) => {
                // unsynced: show as a slow, static block — honest fallback
                let lines: Vec<lrc::LrcLine> = lrc::parse(&plain_to_lrc(&text, duration));
                cache::store(
                    &title,
                    &artist,
                    duration,
                    &cache::CachedLyrics {
                        source: "lrclib-plain".into(),
                        lines: lines.clone(),
                    },
                );
                set_timeline(lines);
                let _ = app.emit("lyrics-timeline", timeline());
            }
            lrclib::Fetched::Nothing => {
                let _ = app.emit("lyrics-miss", (&title, &artist));
            }
        }
    });
}

/// Wrap plain lyrics as one un-timed line per paragraph so the same
/// painter can render them (they use even spacing; no highlight sync).
fn plain_to_lrc(text: &str, duration: f64) -> String {
    // spread lines evenly across the first 85 % of the track
    let n = text.lines().filter(|l| !l.trim().is_empty()).count();
    let _ = n; // count unused; keep API simple
    let mut out = String::new();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let step = (duration * 0.85 / lines.len().max(1) as f64).max(1.2);
    for (i, l) in lines.iter().enumerate() {
        let t = i as f64 * step;
        let m = (t / 60.0) as i64;
        let s = t % 60.0;
        out.push_str(&format!("[{:02}:{:05.2}]{}\n", m, s, l));
    }
    out
}

/// Milestone-1 stand-in for the macOS sidecar / Windows SMTC.
/// Behaves exactly like a real source while no real source is available:
///   * fresh **snapshot every 4 s** → the periodic drift correction
///   * track end → the player "wraps" and we re-anchor like a new track
pub fn start_demo(app: AppHandle) {
    let ticker = app.clone();
    thread::spawn(move || {
        while !REAL_SOURCE_SEEN.load(Ordering::Relaxed) {
            let clock = ticker.state::<SharedClock>();
            let pos = guard(&clock).position();
            if REAL_SOURCE_SEEN.load(Ordering::Relaxed) {
                break;
            }
            let _ = ticker.emit("playback-tick", pos);
            thread::sleep(Duration::from_millis(50));
        }
    });

    let snapshots = app.clone();
    thread::spawn(move || {
        while !REAL_SOURCE_SEEN.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_secs(4));
            if REAL_SOURCE_SEEN.load(Ordering::Relaxed) {
                break;
            }
            let clock = snapshots.state::<SharedClock>();
            let mut c = guard(&clock);
            if !c.playing() {
                continue;
            }
            let pos = c.raw_position();
            if pos >= DEMO_DURATION {
                c.resync(demo_state(0.2));
            } else {
                c.resync(demo_state(pos));
            }
            if !REAL_SOURCE_SEEN.load(Ordering::Relaxed) {
                let _ = snapshots.emit("media-state", c.state());
            }
        }
    });
}
