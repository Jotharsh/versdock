// lrc.rs — parse LRC (the line-synced format LRCLIB serves) into a
// sorted timeline. Kept dependency-free on purpose: it's tiny.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct LrcLine {
    pub t: f64, // seconds
    pub text: String,
}

/// Parse `[mm:ss.xx] text` lines. Supports `[mm:ss]`, `[mm:ss.xx]`,
/// `[mm:ss.xxx]` and multiple timestamps per line. Metadata tags like
/// `[ti:…]`, `[ar:…]` are dropped. Blank lines are kept as fade-out marks.
pub fn parse(src: &str) -> Vec<LrcLine> {
    let mut out: Vec<LrcLine> = Vec::new();

    for raw in src.lines() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }
        let mut times: Vec<f64> = Vec::new();
        let mut i = 0;
        let bytes = line.as_bytes();

        // Consume a run of leading [tags]; only timestamp tags count.
        while i < bytes.len() && bytes[i] == b'[' {
            match line[i..].find(']') {
                Some(end) => {
                    match parse_time(&line[i + 1..i + end]) {
                        Some(t) => {
                            times.push(t);
                            i += end + 1;
                        }
                        None => break, // metadata tag: leave the rest alone
                    }
                }
                None => break,
            }
        }

        if times.is_empty() {
            continue;
        }
        let text = line[i..].trim().to_string();
        for t in times {
            out.push(LrcLine {
                t,
                text: text.clone(),
            });
        }
    }

    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// `[mm:ss]` / `[mm:ss.xx]` / `[mm:ss.xxx]` → seconds.
fn parse_time(tag: &str) -> Option<f64> {
    let (mm, rest) = tag.split_once(':')?;
    let mins: f64 = mm.trim().parse().ok()?;

    let secs: f64 = match rest.split_once('.') {
        Some((s, frac)) => {
            let f: f64 = format!("0.{}", frac.chars().take(3).collect::<String>()).parse().ok()?;
            s.trim().parse::<f64>().ok()? + f
        }
        None => rest.trim().parse::<f64>().ok()?,
    };
    if mins.is_nan() || secs.is_nan() {
        return None;
    }
    Some(mins * 60.0 + secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps_and_sorts() {
        let src = "[ti:demo]\n[00:04.00]c\n[00:01.50]a\n[00:02]b\n[00:01.50]a again";
        let out = parse(src);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].t, 1.5);
        assert_eq!(out[0].text, "a");
        assert_eq!(out[1].text, "a again");
        assert_eq!(out[2].t, 2.0);
        assert_eq!(out[3].text, "c");
    }

    #[test]
    fn drops_metadata_and_blank() {
        assert!(parse("[ar:x]\n[by:y]").is_empty());
    }

    #[test]
    fn handles_ms_precision() {
        let out = parse("[01:02.345]hi");
        assert!((out[0].t - 62.345).abs() < 1e-9);
    }
}
