// lrclib.rs — free lyrics source. lrclib.net is a public community API:
// no key, no account, returns LRC (line-synced) + plain lyrics.
// We only keep synced results; a duration tolerance (±3 s) guards against
// live versions / remixes of similarly-named tracks.

use crate::lyrics::lrc::LrcLine;
use serde::Deserialize;

const API: &str = "https://lrclib.net/api";
/// LRCLIB asks consumers for a truthful, identifiable User-Agent.
const UA: &str = concat!(
    "Versdock/0.1.0 (https://github.com/Jotharsh/Versdock; ",
    "jotharsh2004@gmail.com)",
);

#[derive(Deserialize, Debug, Clone)]
pub struct LrclibHit {
    #[serde(rename = "trackName", default)]
    pub track_name: String,
    #[serde(rename = "artistName", default)]
    pub artist_name: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(rename = "syncedLyrics", default)]
    pub synced_lyrics: Option<String>,
    #[serde(rename = "plainLyrics", default)]
    pub plain_lyrics: Option<String>,
}

pub enum Fetched {
    Synced(Vec<LrcLine>, String), // lines + credited track string
    Plain(String),                // unsynced text: real lyrics, no timeline
    Nothing,
}

/// Fetch synced lyrics for a track. Tolerance: ±3 s. Falls back to the
/// best hit with synced lyrics regardless of duration if nothing closer.
pub fn fetch(title: &str, artist: &str, duration: f64) -> Fetched {
    let client = build_client();

    // 1. exact endpoint (album/name known → server picks best; 404 = no hit)
    let url = format!(
        "{}/get?track_name={}&artist_name={}&album_name=&duration={}",
        API,
        enc(title),
        enc(artist),
        duration.round() as i64
    );
    if let Ok(body) = get(&client, &url) {
        if let Ok(hit) = serde_json::from_str::<LrclibHit>(&body) {
            let f = judge(hit, duration);
            if !matches!(f, Fetched::Nothing) {
                return f;
            }
        }
    }

    // 2. search endpoint: pick the closest-duration hit with synced lyrics
    let url = format!(
        "{}/search?track_name={}&artist_name={}",
        API,
        enc(title),
        enc(artist)
    );
    if let Ok(body) = get(&client, &url) {
        if let Ok(hits) = serde_json::from_str::<Vec<LrclibHit>>(&body) {
            let mut best: Option<&LrclibHit> = None;
            for h in hits.iter().filter(|h| h.synced_lyrics.is_some()) {
                let d = (h.duration - duration).abs();
                if best.is_none_or(|b| (b.duration - duration).abs() > d) {
                    best = Some(h);
                }
            }
            if let Some(b) = best {
                if (b.duration - duration).abs() <= 3.0 {
                    return judge(b.clone(), duration);
                }
            }
        }
    }

    // 3. unsynced fallback (plain text beats nothing for a "lyrics" app)
    let url = format!("{}/search?track_name={}&artist_name={}", API, enc(title), enc(artist));
    if let Ok(body) = get(&client, &url) {
        if let Ok(hits) = serde_json::from_str::<Vec<LrclibHit>>(&body) {
            if let Some(h) = hits.iter().find(|h| h.plain_lyrics.is_some()) {
                return Fetched::Plain(h.plain_lyrics.clone().unwrap_or_default());
            }
        }
    }

    Fetched::Nothing
}

fn judge(hit: LrclibHit, duration: f64) -> Fetched {
    match &hit.synced_lyrics {
        Some(lrc) if !lrc.trim().is_empty() => {
            let ok_duration = hit.duration.abs() > 0.0 && (hit.duration - duration).abs() <= 3.0;
            if ok_duration || duration <= 0.0 {
                Fetched::Synced(
                    crate::lyrics::lrc::parse(lrc),
                    format!("{} — {}", hit.track_name, hit.artist_name),
                )
            } else {
                Fetched::Nothing
            }
        }
        _ => match hit.plain_lyrics {
            Some(txt) if !txt.trim().is_empty() => Fetched::Plain(txt),
            _ => Fetched::Nothing,
        },
    }
}

fn build_client() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(6))
        .timeout(std::time::Duration::from_secs(12))
        .build()
}

fn get(client: &ureq::Agent, url: &str) -> Result<String, ureq::Error> {
    let resp = client
        .get(url)
        .set("User-Agent", UA)
        .call()?;
    Ok(resp.into_string().unwrap_or_default())
}

/// Percent-encode a query value (the subset LRCLIB needs).
pub fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
