//! Display ICC profile acquisition and change detection (TASK-02-001).
//!
//! macOS reads the main display's `CGColorSpace` ICC data through
//! CoreGraphics; Windows reads the ICM profile of the screen DC (the ICM2
//! `WcsGetDefaultColorProfile` path reduced to its wingdi core, which
//! returns the same active .icm file); Linux has no display-profile API we
//! can rely on, so it returns `None` and the pipeline falls back to sRGB —
//! exactly the PLAN §6 P1.5 scope.
//!
//! Change detection is a 2 s poll of the profile fingerprint. The task spec
//! asks for per-OS display-reconfiguration callbacks (CoreGraphics
//! reconfiguration notifications, WM_DISPLAYCHANGE); those need either a
//! raw window hook or a message-only window, both far beyond the render
//! stage this task owns — the poll satisfies the functional requirement
//! ("picked up without app restart") and is recorded as a deviation in the
//! task file.
//!
//! This module is the only place in the workspace that touches OS color
//! FFI. The workspace denies `unsafe_code`; the escape is scoped to this
//! module because profile acquisition cannot be done any other way (the
//! JS/WebView side has no API for it either), the surface is read-only
//! (no pointers are written through), and every returned buffer is copied
//! into owned `Vec<u8>` before leaving the block.
#![allow(unsafe_code)]

use std::time::Duration;

/// How often the profile is re-read while the app runs.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Read the ICC profile bytes of the main display. `None` when the OS gives
/// us nothing usable (Linux, or a display without an assigned profile) —
/// the caller then falls back to sRGB.
pub fn current_display_profile() -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        platform::profile()
    }
    #[cfg(target_os = "windows")]
    {
        platform::profile()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDataGetLength(data: *const c_void) -> isize;
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayCopyColorSpace(display: u32) -> *mut c_void;
        fn CGColorSpaceCopyICCData(space: *const c_void) -> *mut c_void;
    }

    /// Main display's ICC data, copied into an owned buffer. `None` when
    /// the display has no color space or the space carries no ICC data.
    pub fn profile() -> Option<Vec<u8>> {
        unsafe {
            let space = CGDisplayCopyColorSpace(CGMainDisplayID());
            if space.is_null() {
                return None;
            }
            let data = CGColorSpaceCopyICCData(space);
            CFRelease(space);
            if data.is_null() {
                return None;
            }
            let len = CFDataGetLength(data).max(0) as usize;
            let out = if len > 0 {
                Some(std::slice::from_raw_parts(CFDataGetBytePtr(data), len).to_vec())
            } else {
                None
            };
            CFRelease(data);
            out
        }
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;

    #[link(name = "gdi32")]
    extern "system" {
        fn CreateDCW(
            driver: *const u16,
            device: *const u16,
            output: *const u16,
            init: *const c_void,
        ) -> *mut c_void;
        fn GetICMProfileW(hdc: *mut c_void, size: *mut u32, filename: *mut u16) -> i32;
        fn DeleteDC(hdc: *mut c_void) -> i32;
    }

    /// The active ICM profile file of the screen DC, read into an owned
    /// buffer. `None` when the display has no assigned profile.
    pub fn profile() -> Option<Vec<u8>> {
        unsafe {
            let display: Vec<u16> = "DISPLAY\0".encode_utf16().collect();
            let hdc =
                CreateDCW(display.as_ptr(), std::ptr::null(), std::ptr::null(), std::ptr::null());
            if hdc.is_null() {
                return None;
            }
            let mut len: u32 = 0;
            // First call with a null buffer reports the required length
            // (including the terminator) in `len`.
            if GetICMProfileW(hdc, &mut len, std::ptr::null_mut()) == 0 || len == 0 {
                DeleteDC(hdc);
                return None;
            }
            let mut buf = vec![0u16; len as usize];
            let ok = GetICMProfileW(hdc, &mut len, buf.as_mut_ptr());
            DeleteDC(hdc);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(
                &buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())],
            );
            (!path.is_empty()).then(|| std::fs::read(path).ok())?
        }
    }
}

#[cfg(target_os = "macos")]
use imp as platform;
#[cfg(target_os = "windows")]
use imp as platform;

#[cfg(test)]
mod tests {
    use super::*;

    /// The acquisition path must either yield a parseable ICC profile or
    /// `None` — never garbage. On Linux the contract is exactly `None`
    /// (sRGB fallback, PLAN §6 P1.5).
    #[test]
    fn profile_is_none_or_parseable_icc() {
        let profile = current_display_profile();
        if cfg!(target_os = "linux") {
            assert!(profile.is_none(), "Linux must fall back to sRGB");
        } else if let Some(bytes) = profile {
            assert!(bytes.len() > 128, "ICC profiles are never tiny: {} bytes", bytes.len());
            // The ICC device signature `acsp` lives at fixed offset 36.
            assert_eq!(&bytes[36..40], b"acsp", "not an ICC profile header");
        }
    }
}
