// playback_clock.rs — THE sync engine (see DESIGN.md §2)
//
// Players don't stream "current time". We get a *snapshot*:
//   (title, artist, duration, position, playing)
// then advance it with our own clock and re-anchor on every event
// (play / pause / seek / new track). That extrapolation + re-anchoring
// is the whole product: get it right and Versdock feels perfectly synced.

use serde::Serialize;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;

pub type SharedClock = Mutex<PlaybackClock>;

/// Lock that survives poisoning (a panic in one thread must not
/// permanently wedge the overlay's clock).
pub fn guard<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// One snapshot from a media source. On macOS this comes from the Swift
/// sidecar (stdout JSON), on Windows from the WinRT SMTC adapter.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct MediaState {
    #[serde(default)]
    pub source: String, // "demo" | "macos" | "windows" | "spotify" …
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub position: f64,
    pub playing: bool,
}

/// Anchor-based playback clock.
///   pos(t) = anchor_pos + (t - anchor_at), frozen while paused, then the
///   user's per-track offset is applied on the way out.
pub struct PlaybackClock {
    state: MediaState,
    anchor_pos: f64,
    anchor_at: Instant,
    offset: f64, // seconds; LRCLIB timestamps are per-uploader, so every
                 // device needs its own figurative nudge (±0.5 s buttons)
}

impl PlaybackClock {
    pub fn new(state: MediaState) -> Self {
        let anchor_pos = state.position;
        Self {
            state,
            anchor_pos,
            anchor_at: Instant::now(),
            offset: 0.0,
        }
    }

    /// Re-anchor to a fresh snapshot from the media source. Fired on every
    /// state event (pause/seek/track change) AND on the periodic 4 s drift
    /// check — never trust the last snapshot for more than a few seconds.
    pub fn resync(&mut self, snapshot: MediaState) {
        self.anchor_pos = snapshot.position;
        self.anchor_at = Instant::now();
        self.state = snapshot;
    }

    /// Offset-corrected position, i.e. what the line highlight uses.
    pub fn position(&self) -> f64 {
        let mut t = self.anchor_pos;
        if self.state.playing {
            t += self.anchor_at.elapsed().as_secs_f64();
        }
        (t.max(0.0).min(self.state.duration)) + self.offset
    }

    /// The player's own truth, without the user offset.
    pub fn raw_position(&self) -> f64 {
        self.position() - self.offset
    }

    pub fn toggle_play(&mut self) {
        let cur = self.raw_position();
        self.state.position = cur;
        self.state.playing = !self.state.playing;
        let mut s = self.state.clone();
        s.position = cur;
        self.resync(s); // re-anchor: a parked position after pause is the
                        // #1 reported bug in every lyrics widget — never
                        // let the extrapolation coast through a pause
    }

    /// `delta` in seconds; negative pulls the highlight *earlier*.
    pub fn bump_offset(&mut self, delta: f64) -> f64 {
        self.offset = (self.offset + delta).clamp(-5.0, 5.0);
        self.offset
    }

    pub fn offset(&self) -> f64 {
        self.offset
    }

    pub fn playing(&self) -> bool {
        self.state.playing
    }

    pub fn state(&self) -> MediaState {
        self.state.clone()
    }
}
