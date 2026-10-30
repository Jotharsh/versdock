# Versdock

**Synced lyrics that float over your desktop — for any app, with zero logins.**

Like Spicetify's lyrics view, but for your *whole desktop*: Spotify, Apple
Music, YouTube Music (in a browser), VLC, anything — Versdock reads what
your OS says is playing and lays a transparent, line-synced lyrics overlay
on top of the screen.

```
[Apple Music / Spotify / YT Music / any app]  plays audio
        │  OS now-playing (no accounts, ever)
        ▼
playback clock (Rust) ── snapshot → anchor → extrapolate → 4 s drift fix
        │
        ▼
LRCLIB (free, no key) + LRC parser + per-track cache
        │
        ▼
transparent overlay ── drag · pin · click-through lock · your colours
```

## Free by design
* **No accounts, no OAuth on day one** — the app never talks to your
  player, it listens to the OS media session.
* **LRCLIB** (`lrclib.net`) for line-synced lyrics: community API, no key.
* **Microsoft (Windows SMTC) / macOS MediaRemote** now-playing — OS feature.
* Everything ships as unsigned binaries from GitHub Releases. No App
  Stores, no notarization fees, no paid anything.

## Honest limits
* Line-synced, not word-by-word karaoke (LRCLIB is line-level).
* Lyrics only where LRCLIB has them — the overlay says so otherwise.
* Won't draw over exclusive-fullscreen games (borderless windowed = fine).
* Click-to-seek only in modes where the player allows seeking.
* macOS MediaRemote is a private framework: the sidecar may need a small
  update across macOS versions (accepted maintenance tax).

## Run it
```bash
npm install
npm run tauri dev      # first build takes a few minutes (Rust compile)
```

Overlay controls while it floats:
* **Hover** the top edge → toolbar: play/pause, ±0.5 s sync nudge, lock.
* **⌘⇧L** (global hotkey) → click-through lock / unlock.
* Drag anywhere on the top strip to move it.

## Status — Build-by-milestone (see DESIGN.md)
1. ✅ Overlay shell + playback clock + LRC parser + demo track (this ship)
2. ⬜ LRCLIB fetch + per-track disk cache + duration-tolerance matching
3. ⬜ macOS MediaRemote sidecar → real playback, your Mac
4. ⬜ Painter polish: colour presets, album-art auto colour
5. ⬜ Windows SMTC adapter
6. ⬜ Settings, tray, autostart, per-track offsets

---

Part of [dailytech.build](https://jotharsh.github.io/dailytech-site/) —
one free, open-source build every day. MIT licensed.
