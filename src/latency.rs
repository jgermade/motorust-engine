//! Tells macOS that the game is something the player is doing and that
//! needs its frames on time, for as long as it runs: no App Nap, no timers
//! put off to save power, and neither the display nor the Mac going to
//! sleep while nobody touches the keyboard or mouse (a race on a gamepad,
//! the benchmark). What `caffeinate` does from the terminal, from inside
//! the game (`NSProcessInfo`'s `beginActivityWithOptions:reason:`).
//!
//! The Game Mode of macOS is not asked for here: there is no call for it.
//! macOS turns it on for an app that says it is a game (the bundle's
//! category, see `assets/Info.plist`) when it goes fullscreen.

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};

/// `NSActivityUserInitiated`: the player asked for it (and the Mac does not
/// sleep while it lasts).
const USER_INITIATED: u64 = 0x00FF_FFFF;
/// `NSActivityIdleDisplaySleepDisabled`: nor the display.
const DISPLAY_AWAKE: u64 = 1 << 40;
/// `NSActivityLatencyCritical`: timers on time, for frames and sound.
const LATENCY_CRITICAL: u64 = 0xFF_0000_0000;

/// Begins the activity, which lasts until the game quits: its token is
/// never released.
pub fn begin() {
    // SAFETY: `processInfo`, `stringWithUTF8String:` (with a string that
    // ends in a nul) and `beginActivityWithOptions:reason:` (with an
    // `NSActivityOptions`, a 64-bit integer, and an `NSString`) are what
    // Foundation declares, and each returns an object.
    unsafe {
        let info: Retained<AnyObject> = msg_send![class!(NSProcessInfo), processInfo];
        let reason: Retained<AnyObject> =
            msg_send![class!(NSString), stringWithUTF8String: c"Racing".as_ptr()];
        let activity: Retained<AnyObject> = msg_send![
            &*info,
            beginActivityWithOptions: USER_INITIATED | DISPLAY_AWAKE | LATENCY_CRITICAL,
            reason: &*reason
        ];
        // Ended only by quitting.
        std::mem::forget(activity);
    }
}
