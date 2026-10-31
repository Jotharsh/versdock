// settings.rs — persisted preferences that must survive restarts.
// Milestone 6 (incremental): per-track sync offsets, autostart flag.
// One small JSON in the app-data dir; write-through on every change with
// a tmp-rename so a crash never truncates the file.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone)]
struct File {
    /// "artist|title" → seconds of highlight shift (-5..+5)
    #[serde(default)]
    offsets: HashMap<String, f64>,
    /// widget pinned *on the desktop* (behind app windows)
    #[serde(default)]
    pin_desktop: bool,
}

static FILE: OnceLock<PathBuf> = OnceLock::new();
static DATA: RwLock<Option<File>> = RwLock::new(None);

pub fn init(app: &tauri::AppHandle) {
    let dir = tauri::Manager::path(app)
        .app_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir().join("versdock-data"));
    let _ = std::fs::create_dir_all(&dir);
    let _ = FILE.set(dir.join("settings.json"));
    let data = std::fs::read_to_string(FILE.get().unwrap())
        .ok()
        .and_then(|s| serde_json::from_str::<File>(&s).ok())
        .unwrap_or_default();
    let _ = DATA.write().map(|mut d| *d = Some(data));
}

fn read() -> File {
    DATA.read()
        .ok()
        .and_then(|d| d.clone())
        .unwrap_or_default()
}

fn persist(f: &File) {
    let Some(p) = FILE.get() else { return };
    if let Ok(json) = serde_json::to_string_pretty(f) {
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, &json).is_ok() {
            let _ = std::fs::rename(&tmp, p);
        }
    }
}

pub fn offset_for(title: &str, artist: &str) -> Option<f64> {
    let key = track_key(title, artist);
    read().offsets.get(&key).copied().filter(|o| *o != 0.0)
}

pub fn record_offset(title: &str, artist: &str, offset: f64) {
    let key = track_key(title, artist);
    if let Ok(mut d) = DATA.write() {
        let f = d.get_or_insert_with(File::default);
        if offset.abs() < 0.001 {
            f.offsets.remove(&key);
        } else {
            f.offsets.insert(key, offset);
        }
        persist(f);
    }
}

fn track_key(title: &str, artist: &str) -> String {
    format!("{}|{}", artist.trim().to_lowercase(), title.trim().to_lowercase())
}

// ── desktop pin ──────────────────────────────────────────────
pub fn pin_desktop() -> bool {
    read().pin_desktop
}

pub fn record_pin_desktop(on: bool) {
    if let Ok(mut d) = DATA.write() {
        let f = d.get_or_insert_with(File::default);
        f.pin_desktop = on;
        persist(f);
    }
}
