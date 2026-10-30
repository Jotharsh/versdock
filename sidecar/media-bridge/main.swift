// media-bridge/main.swift — macOS sidecar (Milestone 3).
//
// Standalone Swift CLI. Versdock (Rust/Tauri) spawns it and reads
// JSON playback events from stdout, one per line. Windows needs no
// sidecar (WinRT SMTC in Rust); this binary is macOS-only.
//
// The private MediaRemote framework is what makes "lyrics for ANY
// player with zero logins" possible, but Apple doesn't document it.
// Strategy follows DESIGN.md: try to dlsym the well-known symbols;
// if any are missing (macOS version drift — the accepted ongoing
// tax), degrade to demo heartbeats so the pipeline stays testable.

import Foundation

struct Event: Encodable {
    let event: String
    let title: String
    let artist: String
    let duration: Double
    let position: Double
    let playing: Bool
}

func emit(_ e: Event) {
    if let data = try? JSONEncoder().encode(e),
       let line = String(data: data, encoding: .utf8) {
        print(line)
        fflush(stdout)   // flush: Rust reads line-by-line
    }
}

// ── demo mode (default until Milestone 3 wires MediaRemote) ──
// Same JSON shape the real bridge will emit, so the Rust reader is
// written once and never changes:
//   {"event":"state","title":…,"artist":…,"duration":…,"position":…,"playing":true}
var pos: Double = 11.0
let duration = 222.0
while true {
    emit(Event(event: "state",
               title: "NEON BETWEEN THE LINES",
               artist: "Versdock Demo",
               duration: duration,
               position: pos,
               playing: true))
    pos += 1.0
    if pos >= duration { pos = 0.2 }
    Thread.sleep(forTimeInterval: 1.0)
}

// ── Milestone 3 integration plan (kept here as the contract) ──
//
// 1. dlopen /System/Library/PrivateFrameworks/MediaRemote.framework/MediaRemote
// 2. dlsym:
//      MRMediaRemoteGetNowPlayingInfo          (title, artist, duration)
//      MRMediaRemoteGetNowPlayingApplicationIsPlaying
//      MRMediaRemoteGetNowPlayingInfoHasAvailability.standard
//    register MRMediaRemoteRegisterForNowPlayingNotifications —
//      kMRMediaRemoteNowPlayingInfoDidChangeNotification,
//      kMRMediaRemoteNowPlayingApplicationDidChangeNotification
// 3. On info-did-change: re-emit a full "state" event.
// 4. No periodic position polling in the sidecar — Versdock's clock
//    extrapolates between events; seeking is detected by position jumps
//    in the info payload.
// 5. RunLoop: CFRunLoopRun() must run on the main thread; keep the
//    CLI process alive with dispatchMain() after registering.
// 6. macOS version gates: on 15+ some symbols changed behaviour —
//    detect via #available / uname and fall back per DESIGN.md §4.
