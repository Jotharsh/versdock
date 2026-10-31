// window_level.rs — desktop pin (Milestone 6, user-reported bug fix).
//
// The user wants the widget to be able to live *on the desktop* (behind
// their app windows, above the wallpaper) instead of always cutting over
// every app. macOS exposes that as a window level — the private API flag
// is already on in tauri.conf.json, so we send two NSWindow messages
// through objc2 (already in the tree via tao):
//   * setLevel:        CGWindowLevel(kCGDesktopWindowLevel) when pinned, or
//                      NSFloatingWindowLevel (3) for the normal top mode
//   * setCollectionBehavior: canJoinAllSpaces (visible on every Space)
//     + stationary; "top" mode additionally joins fullscreen apps
//     (fullScreenAuxiliary) — pinned mode deliberately does not, so a
//     fullscreen video covers the widget, as a desktop widget should.
//
// Windows: needs SetWindowPos(HWND_BOTTOM) — honest TODO, returns false
// until Milestone-5 CI (recorded in README).
#[cfg(target_os = "macos")]
pub unsafe fn apply_desktop_pin(ns_window_ptr: *mut std::ffi::c_void, desktop: bool) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    unsafe extern "C" {
        #[link_name = "CGWindowLevelForKey"]
        fn CGWindowLevelForKey(key: i32) -> i32;
    }
    // Keys follow the CGWindowLevelKey order (matches tao's macOS ffi.rs):
    const K_CG_DESKTOP_WINDOW_LEVEL_KEY: i32 = 2;
    // NSFloatingWindowLevel — what tao's set_always_on_top uses
    const NS_FLOATING_WINDOW_LEVEL: i32 = 3;

    let level: i32 = if desktop {
        unsafe { CGWindowLevelForKey(K_CG_DESKTOP_WINDOW_LEVEL_KEY) }
    } else {
        NS_FLOATING_WINDOW_LEVEL
    };

    // NSWindow.CollectionBehavior
    const CAN_JOIN_ALL_SPACES: i64 = 1 << 0;
    const STATIONARY: i64 = 1 << 2;
    const FULLSCREEN_AUXILIARY: i64 = 1 << 4;

    let behavior: i64 = if desktop {
        CAN_JOIN_ALL_SPACES | STATIONARY
    } else {
        CAN_JOIN_ALL_SPACES | FULLSCREEN_AUXILIARY
    };

    let w = ns_window_ptr as *mut AnyObject;
    let _: () = unsafe { msg_send![w, setLevel: level as isize] };
    let _: () = unsafe { msg_send![w, setCollectionBehavior: behavior as u64] };
}

#[cfg(not(target_os = "macos"))]
pub fn apply_desktop_pin(_ns_window_ptr: *mut std::ffi::c_void, _desktop: bool) {
    // TODO (Windows): SetWindowPos(hwnd, HWND_BOTTOM / HWND_TOPMOST)
}
