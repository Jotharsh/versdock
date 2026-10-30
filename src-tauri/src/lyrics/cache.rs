// cache.rs — per-track disk cache. Same fetch costs nothing the second
// time; also makes Versdock work fully offline for tracks it has seen.

use crate::lyrics::lrc::LrcLine;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::Manager;

/// Structured to later carry more (fetched credits, offset…).
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct CachedLyrics {
    pub source: String, // "lrclib-synced" | "lrclib-plain"
    pub lines: Vec<LrcLine>,
}

static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Initialised once at app setup from Tauri's app-data dir.
pub fn init(app: &tauri::AppHandle) {
    let dir = app
        .path()
        .app_data_dir()
        .map(|d| d.join("lyrics-cache"))
        .unwrap_or_else(|_| std::env::temp_dir().join("versdock-cache"));
    let _ = std::fs::create_dir_all(&dir);
    let _ = DIR.set(dir);
}

/// Stable key for a track: artist + title, hashed with FNV and mangled —
/// same song obtained twice maps to the same file, different songs never
/// collide in practice and through-the-years the hash stays honest even
/// across OS upgrades.
fn key(title: &str, artist: &str, duration: f64) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    let feed = format!(
        "{}|{}|{}",
        artist.to_lowercase().trim_start(),
        title.to_lowercase(),
        duration.round() as i64
    );
    for b in feed.trim().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("v{:016x}", h)
}

fn path_for(k: &str) -> PathBuf {
    DIR.get()
        .expect("cache::init not called")
        .join(format!("{}.json", k))
}

pub fn load(title: &str, artist: &str, duration: f64) -> Option<CachedLyrics> {
    let p = path_for(&key(title, artist, duration));
    let txt = std::fs::read_to_string(p).ok()?;
    serde_json::from_str(&txt).ok()
}

pub fn store(title: &str, artist: &str, duration: f64, c: &CachedLyrics) {
    let p = path_for(&key(title, artist, duration));
    if let Ok(json) = serde_json::to_string_pretty(c) {
        // write-then-rename so a crash mid-write never poisons the cache
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}
