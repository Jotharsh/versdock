// palette.rs — album-art auto colour. Milestone 4.
//
// The spicy-lyrics signature look is the *tinted* active line. We pick the
// dominant colour of the album artwork (the free way to get art without
// per-player deals: the iTunes Search API is public, no account, no key)
// and push it to the overlay as an accent colour.
//
// Honest limits (DESIGN §4): art lookup is by track/artist name; rare
// tracks with no iTunes release simply keep the default accent.

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::RwLock;
use tauri::Emitter;

const SEARCH: &str = "https://itunes.apple.com/search";
const UA: &str = concat!(
    "Versdock/0.1.0 (https://github.com/Jotharsh/Versdock; ",
    "jotharsh2004@gmail.com)"
);

/// In-memory cache: track key → accent RGB.
static ACCENTS: RwLock<Option<HashMap<String, [u8; 3]>>> = RwLock::new(None);

#[derive(serde::Serialize, Clone)]
struct PaletteMsg {
    accent: String, // "r, g, b" (CSS rgb() args)
}

/// Worker-thread entry: find art → dominant colour → cache → emit.
/// All failures are silent: a missing palette is a valid palette.
pub fn set_for_track(app: &tauri::AppHandle, title: &str, artist: &str, duration: f64) {
    let key = format!(
        "{}|{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    );
    let cached = match ACCENTS.read() {
        Ok(g) => g
            .as_ref()
            .and_then(|m| m.get(&key).copied())
            .map(|c| c),
        Err(_) => None,
    };
    if let Some(c) = cached {
        emit_accent(app, c);
        return;
    }

    let Some(c) = dominant_from_itunes(title, artist, duration) else {
        return;
    };
    if let Ok(mut m) = ACCENTS.write() {
        m.get_or_insert_with(HashMap::new).insert(key, c);
    }
    emit_accent(app, c);
}

fn emit_accent(app: &tauri::AppHandle, c: [u8; 3]) {
    let _ = app.emit(
        "palette",
        PaletteMsg {
            accent: format!("{}, {}, {}", c[0], c[1], c[2]),
        },
    );
}

fn client() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(8))
        .build()
}

fn dominant_from_itunes(title: &str, artist: &str, duration: f64) -> Option<[u8; 3]> {
    let url = format!(
        "{}?term={}&entity=song&limit=6",
        SEARCH,
        crate::lyrics::lrclib::enc(&format!("{} {}", title, artist))
    );
    let body = client()
        .get(&url)
        .set("User-Agent", UA)
        .call()
        .ok()?
        .into_string()
        .ok()?;
    let resp: ITunesResp = serde_json::from_str(&body).ok()?;

    // pick the hit that best matches name + duration (±8 s)
    let mut best: Option<(&ITunesHit, f64)> = None;
    for h in &resp.results {
        if h.track_name.is_empty() || h.artwork_url_100.is_empty() {
            continue;
        }
        let name_ok = h.track_name.to_lowercase().contains(&title.trim().to_lowercase())
            || title
                .trim()
                .to_lowercase()
                .contains(&h.track_name.to_lowercase());
        let t = h.track_time_millis as f64 / 1000.0;
        let dur_score = if (t - duration).abs() <= 8.0 { 0.0 } else { 1000.0 };
        let name_score = if name_ok { 0.0 } else { 500.0 };
        let score = name_score + dur_score;
        if best.is_none_or(|(_, s)| score < s) {
            best = Some((h, score));
        }
    }
    let (hit, _score) = best?; // tuple is (hit, score) — mind the order

    // artworkUrl100 is square; the CDN serves bigger buckets with the same path
    let art_url = hit.artwork_url_100.replacen("100x100bb", "600x600bb", 1);
    let mut reader = client().get(&art_url).call().ok()?.into_reader();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut bytes).ok()?;
    let raw = dominant_colour(&bytes).map(boost_for_text)?;
    Some(raw)
}

#[derive(Deserialize, Debug)]
struct ITunesResp {
    #[serde(default)]
    results: Vec<ITunesHit>,
}

#[derive(Deserialize, Debug)]
struct ITunesHit {
    #[serde(rename = "trackName", default)]
    track_name: String,
    #[serde(rename = "artworkUrl100", default)]
    artwork_url_100: String,
    #[serde(rename = "trackTimeMillis", default)]
    track_time_millis: i64,
}

/// Dominant hue bucket of the artwork, weighted by saturation.
fn dominant_colour(bytes: &[u8]) -> Option<[f64; 3]> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = image::imageops::thumbnail(&img.to_rgb8(), 48, 48);

    let mut weight = [0.0f64; 12];
    let mut sum = [[0.0f64; 3]; 12];
    for px in small.pixels() {
        let (r, g, b) = (px[0] as f64, px[1] as f64, px[2] as f64);
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        if mx == 0.0 {
            continue;
        }
        let sat = (mx - mn) / mx;
        if sat < 0.12 {
            continue; // near-grey: excludes white text/washes on covers
        }
        let (h, _, _) = rgb_to_hsv(r, g, b);
        let bin = ((h * 12.0) as usize).min(11);
        let w = sat * sat;
        weight[bin] += w;
        sum[bin][0] += r * w;
        sum[bin][1] += g * w;
        sum[bin][2] += b * w;
    }
    let top = weight
        .iter()
        .enumerate()
        .filter(|(_, w)| **w > 0.01)
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))?
        .0;
    if weight[top] <= 0.0 {
        return None;
    }
    Some([sum[top][0] / weight[top], sum[top][1] / weight[top], sum[top][2] / weight[top]])
}

/// Nudge the artwork colour towards good text contrast (not neon, not mud).
fn boost_for_text(c: [f64; 3]) -> [u8; 3] {
    let (h, s, _) = rgb_to_hsv(c[0], c[1], c[2]);
    let s = s.max(0.75).min(1.0);
    let v = 0.68; // bright enough over dark glass
    let (r, g, b) = hsv_to_rgb(h, s, v);
    [r.round() as u8, g.round() as u8, b.round() as u8]
}

fn rgb_to_hsv(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h.fract(), if mx == 0.0 { 0.0 } else { d / mx }, mx / 255.0)
}

fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let i = (h * 6.0) as i64;
    let f = h * 6.0 - i as f64;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    (r * 255.0, g * 255.0, b * 255.0)
}
