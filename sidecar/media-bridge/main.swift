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

// ── AppleScript adapters (Spotify & Apple Music) ──────────────
func osa(_ source: String) -> String {
    var err: NSDictionary?
    guard let out = NSAppleScript(source: source)?.executeAndReturnError(&err) else {
        return ""
    }
    return out.stringValue ?? ""
}

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

func queryApp(_ app: String) -> NowPlaying? {
    let script: String
    if app == "Spotify" {
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

// ── unified runner (AppleScript + MediaRemote) ────────────────
func runUnifiedLoop() -> Never {
    fputs("mediabridge: unified bridge active (AppleScript + MediaRemote)\n", stderr)
    let appCycle = ["Spotify", "Music"]
    var mrState: NowPlaying?
    let mrLock = NSLock()

    // Background MediaRemote poller (catches Chrome, Safari, YouTube, VLC)
    if let getInfo = mrGetInfo {
        MRQ.async {
            while true {
                let sem = DispatchSemaphore(value: 0)
                getInfo(MRQ) { info in
                    let title = mrString(info, "kMRMediaRemoteNowPlayingInfoTitle") ?? ""
                    let artist = mrString(info, "kMRMediaRemoteNowPlayingInfoArtist") ?? ""
                    let dur = mrNumber(info, "kMRMediaRemoteNowPlayingInfoDuration") ?? 0
                    let pos = mrNumber(info, "kMRMediaRemoteNowPlayingInfoElapsedTime") ?? 0
                    mrLock.lock()
                    if !title.isEmpty || !artist.isEmpty {
                        mrState = NowPlaying(title: title, artist: artist, duration: dur, position: pos, playing: true)
                    } else {
                        mrState = nil
                    }
                    mrLock.unlock()
                    sem.signal()
                }
                _ = sem.wait(timeout: .now() + 1.5)
                Thread.sleep(forTimeInterval: 0.8)
                if getppid() == 1 { exit(0) }
            }
        }
    }

    while true {
        // 1. High-fidelity native desktop players: Spotify & Apple Music
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

        // 2. MediaRemote fallback for browser tabs / other media players
        if playing == nil {
            mrLock.lock()
            let mr = mrState
            mrLock.unlock()
            if let mr = mr, !mr.title.isEmpty {
                if paused == nil {
                    playing = mr
                }
            }
        }

        // 3. Emit or silence
        if playing == nil && paused == nil {
            noteNone()
        } else {
            let s = playing ?? paused!
            maybeEmit(title: s.title, artist: s.artist,
                      duration: s.duration > 0 ? s.duration : nil,
                      position: s.position >= 0 ? s.position : nil,
                      playing: s.playing)
        }

        Thread.sleep(forTimeInterval: 1.0)
        if getppid() == 1 { exit(0) }
    }
}

// ── entry ──────────────────────────────────────────────────────
runUnifiedLoop()
