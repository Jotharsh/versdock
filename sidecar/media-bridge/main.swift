// media-bridge/main.swift — macOS sidecar (Milestone 3, live).
//
// Tiny Swift CLI. Versdock (Rust) spawns it and reads JSON playback events
// from stdout, one per line:
//   {"event":"state","title":…,"artist":…,"duration":…,"position":…,"playing":…}
//   {"event":"none"}   — nothing playing system-wide
//
// Source ladder (all free, zero logins — DESIGN §2/§4):
//   1. MediaRemote (private) — the "any app" dream; **silently gated on
//     macOS 26/27**, so it's auto-skipped when its callbacks stay silent.
//   2. AppleScript adapters for Spotify + Apple Music — documented, free,
//     expose title/artist/duration/**exact position**. Needs the one-time
//     Automation permission (honest limit documented in README).
//   3. Demo heartbeat — so the pipeline is never dead in development.
//
// The bridge emits a snapshot only when the player *changes its truth*
// (track / play state) or drifts >0.4 s from local extrapolation —
// Versdock's clock meters the in-between.

import Foundation

// ── output ─────────────────────────────────────────────────────
struct Event: Encodable {
    let event: String
    let title: String?
    let artist: String?
    let duration: Double?
    let position: Double?
    let playing: Bool?
}

var outputLock = NSLock()

func emit(_ e: Event) {
    outputLock.lock()
    defer { outputLock.unlock() }
    guard let data = try? JSONEncoder().encode(e),
          let line = String(data: data, encoding: .utf8) else { return }
    print(line)
    fflush(stdout)
}

func emitNone() {
    emit(Event(event: "none", title: nil, artist: nil, duration: nil, position: nil, playing: nil))
}

// ── shared dedupe state ────────────────────────────────────────
let stateLock = NSLock()
var lastTitle = ""
var lastArtist = ""
var lastPos = -1.0
var lastEmittedAt = Date()
var lastPlaying: Bool?
var noneStreak = 0

func noteEmitted() {
    stateLock.lock()
    lastEmittedAt = Date()
    stateLock.unlock()
}

func lastEmitted() -> Date {
    stateLock.lock()
    defer { stateLock.unlock() }
    return lastEmittedAt
}

func lastWasNone() -> Bool {
    stateLock.lock()
    defer { stateLock.unlock() }
    return noneStreak > 0
}

/// Emit only when the source tells us something new:
/// track changed / play state changed / drifted >0.4 s.
func maybeEmit(title: String, artist: String, duration: Double?, position: Double?, playing: Bool) {
    let now = Date()
    stateLock.lock()
    let cachedTitle = lastTitle
    let cachedArtist = lastArtist
    let cachedPos = lastPos
    let cachedPlaying = lastPlaying
    stateLock.unlock()

    var expected = cachedPos
    if playing { expected = cachedPos + now.timeIntervalSince(lastEmitted()) }
    let jumped: Bool = { position.map { abs($0 - expected) > 0.4 } ?? true }()
    let trackChanged = (title != cachedTitle || artist != cachedArtist) || lastWasNone()
    let stateChanged = cachedPlaying != nil ? (cachedPlaying! != playing) : true

    if !trackChanged && !stateChanged && !jumped {
        return // pure extrapolation is enough; stay quiet
    }
    emit(Event(event: "state", title: title, artist: artist,
               duration: duration, position: position, playing: playing))
    stateLock.lock()
    lastTitle = title
    lastArtist = artist
    lastPos = position ?? -1
    lastPlaying = playing
    noneStreak = 0
    stateLock.unlock()
    noteEmitted()
}

func noteNone() {
    let wasNone = lastWasNone()
    stateLock.lock()
    noneStreak += 1
    stateLock.unlock()
    if !wasNone {
        emitNone()
        noteEmitted()
    }
}

// ── source 1: MediaRemote (best-effort; gated on macOS 26/27) ──
let MR_PATH = "/System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote"
typealias MRGetInfo = @convention(c) (DispatchQueue?, @escaping (CFDictionary?) -> Void) -> Void

let handle = dlopen(MR_PATH, RTLD_LAZY)
let mrGetInfo = handle.flatMap { dlsym($0, "MRMediaRemoteGetNowPlayingInfo") }.map {
    unsafeBitCast($0, to: MRGetInfo.self)
}

let MRQ = DispatchQueue(label: "versdock.mediaremote", qos: .userInitiated)

func mrString(_ dict: CFDictionary?, _ key: String) -> String? {
    guard let dict, let k = key as CFString? else { return nil }
    guard let ptr = CFDictionaryGetValue(dict, Unmanaged.passUnretained(k).toOpaque()) else { return nil }
    return Unmanaged<CFString>.fromOpaque(ptr).takeUnretainedValue() as String
}

func mrNumber(_ dict: CFDictionary?, _ key: String) -> Double? {
    guard let dict, let k = key as CFString? else { return nil }
    guard let ptr = CFDictionaryGetValue(dict, Unmanaged.passUnretained(k).toOpaque()) else { return nil }
    let num = Unmanaged<CFNumber>.fromOpaque(ptr).takeUnretainedValue()
    var d: Double = 0
    CFNumberGetValue(num, .float64Type, &d)
    return d.isNaN ? nil : d
}

func runMediaRemoteLoop() {
    // Poll at ~1 Hz: the notification registration differs between macOS
    // versions — polling is the well-trodden path (DESIGN §4).
    let getInfo = mrGetInfo!
    while true {
        getInfo(MRQ) { info in
            let title = mrString(info, "kMRMediaRemoteNowPlayingInfoTitle") ?? ""
            let artist = mrString(info, "kMRMediaRemoteNowPlayingInfoArtist") ?? ""
            if title.isEmpty && artist.isEmpty {
                noteNone()
                return
            }
            maybeEmit(title: title, artist: artist,
                      duration: mrNumber(info, "kMRMediaRemoteNowPlayingInfoDuration"),
                      position: mrNumber(info, "kMRMediaRemoteNowPlayingInfoElapsedTime"),
                      playing: true)
        }
        Thread.sleep(forTimeInterval: 0.8)
        if getppid() == 1 { exit(0) }
        // Symbols exist but callbacks stay silent → macOS is gating the
        // framework (26/27). Fall to the AppleScript bridge.
        if lastWasNone() && Date().timeIntervalSince(lastEmitted()) > 4.0 {
            break
        }
    }
    runAppleScriptBridge()
}

// ── source 2: AppleScript adapters (documented, reliable) ──────
func osa(_ source: String) -> String {
    var err: NSDictionary?
    guard let out = NSAppleScript(source: source)?.executeAndReturnError(&err) else {
        if let e = err {
            fputs("mediabridge osa error: \(e)\n", stderr)
        }
        return ""
    }
    return out.stringValue ?? ""
}

/// "mm:ss" fallback (Music may report position as a clock string).
func parseClock(_ s: String) -> Double? {
    let parts = s.split(separator: ":").compactMap { Double($0) }
    guard parts.count == 2 else { return nil }
    return parts[0] * 60 + parts[1]
}

struct NowPlaying {
    var title: String
    var artist: String
    var duration: Double
    var position: Double
    var playing: Bool
}

/// Poll one player via AppleScript. Returns nil when the app isn't running
/// or reports nothing usable. Never *launches* an app: every script starts
/// with an `is running` check.
func queryApp(_ app: String) -> NowPlaying? {
    let script: String
    if app == "Spotify" {
        // Spotify's duration is milliseconds; divide here in Swift.
        script = """
        if application "Spotify" is running then
          tell application "Spotify"
            try
              set pstate to (player state as string)
              if pstate is "playing" or pstate is "paused" then
                return ((name of current track) & "|" & (artist of current track) & "|" & ((duration of current track) as string) & "|" & ((player position) as string) & "|" & pstate)
              end if
            end try
          end tell
        end if
        """
    } else {
        // Music's duration is seconds.
        script = """
        if application "Music" is running then
          tell application "Music"
            try
              set pstate to (player state as string)
              if pstate is "playing" or pstate is "paused" then
                return ((name of current track) & "|" & (artist of current track) & "|" & ((duration of current track) as string) & "|" & ((player position) as string) & "|" & pstate)
              end if
            end try
          end tell
        end if
        """
    }
    let out = osa(script)
    let f = out.components(separatedBy: "|").map { $0.trimmingCharacters(in: .whitespaces) }
    guard f.count >= 5, !f[0].isEmpty else { return nil }
    var duration = Double(f[2]) ?? 0.0
    if app == "Spotify" { duration /= 1000.0 }
    if duration == 0.0 { duration = parseClock(f[2]) ?? 0.0 }
    let position = Double(f[3]) ?? parseClock(f[3]) ?? 0.0
    let playing = f[4].lowercased() == "playing"
    return NowPlaying(title: f[0], artist: f[1], duration: duration,
                      position: position, playing: playing)
}

func runAppleScriptBridge() -> Never {
    fputs("mediabridge: applescript bridge\n", stderr)
    let appCycle = ["Spotify", "Music"]
    while true {
        // arbitration: a *playing* app wins; otherwise a paused one is held
        var playing: NowPlaying?
        var paused: NowPlaying?
        for app in appCycle {
            guard let st = queryApp(app) else { continue }
            if st.playing {
                if playing == nil { playing = st }
            } else if paused == nil {
                paused = st
            }
            if playing != nil { break }
        }
        if playing == nil && paused == nil {
            noteNone()
        } else {
            let s = playing ?? paused!
            maybeEmit(title: s.title, artist: s.artist,
                      duration: s.duration, position: s.position,
                      playing: s.playing)
        }
        Thread.sleep(forTimeInterval: 1.0)
        if getppid() == 1 { exit(0) }   // orphaned by parent exit
    }
}

// ── source 3: demo heartbeat (dev fallback) ────────────────────
func runDemoHeartbeat() -> Never {
    var pos: Double = 11.0
    let duration = 222.0
    while true {
        emit(Event(event: "state", title: "Neon Between the Lines (demo)",
                   artist: "Versdock Demo", duration: duration,
                   position: pos, playing: true))
        pos += 1.0
        if pos >= duration { pos = 0.2 }
        Thread.sleep(forTimeInterval: 1.0)
        if getppid() == 1 { exit(0) }
    }
}

// ── entry ──────────────────────────────────────────────────────
if mrGetInfo != nil {
    runMediaRemoteLoop()   // self-falls back to AppleScript when gated
} else {
    fputs("mediabridge: MediaRemote unavailable — applescript bridge\n", stderr)
    runAppleScriptBridge()
}
