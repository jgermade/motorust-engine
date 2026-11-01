//! A Mac screen's own resolution, through CoreGraphics: the panel's pixels,
//! which a scaled display mode (HiDPI "looks like" a resolution, a laptop's
//! "more space") does not give. winit only says the display mode's.

use std::ffi::c_void;

use bevy::prelude::*;

type DisplayId = u32;
type DisplayMode = *const c_void;
type Array = *const c_void;

#[repr(C)]
#[derive(Clone, Copy)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Rect {
    origin: Point,
    size: Size,
}

/// The display mode's flag for the panel's own resolution (IOKit's
/// `kDisplayModeNativeFlag`).
const NATIVE_FLAG: u32 = 0x0200_0000;

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGGetActiveDisplayList(max: u32, displays: *mut DisplayId, count: *mut u32) -> i32;
    fn CGDisplayBounds(display: DisplayId) -> Rect;
    fn CGDisplayCopyDisplayMode(display: DisplayId) -> DisplayMode;
    fn CGDisplayCopyAllDisplayModes(display: DisplayId, options: *const c_void) -> Array;
    fn CGDisplayModeGetWidth(mode: DisplayMode) -> usize;
    fn CGDisplayModeGetPixelWidth(mode: DisplayMode) -> usize;
    fn CGDisplayModeGetPixelHeight(mode: DisplayMode) -> usize;
    fn CGDisplayModeGetIOFlags(mode: DisplayMode) -> u32;
    fn CGDisplayModeRelease(mode: DisplayMode);
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFArrayGetCount(array: Array) -> isize;
    fn CFArrayGetValueAtIndex(array: Array, index: isize) -> *const c_void;
    fn CFRelease(object: *const c_void);
}

/// What is known of a display: its mode's pixels, where it is (in pixels,
/// as winit gives a monitor's position) and its panel's own pixels.
struct Display {
    pixels: UVec2,
    position: IVec2,
    native: Option<UVec2>,
}

/// The panel's own resolution of the screen whose display mode has `pixels`
/// pixels, at `position` (as Bevy's `Monitor` has them): the one display
/// there is, or the one that matches.
pub fn panel_size(pixels: UVec2, position: IVec2) -> Option<UVec2> {
    let displays = displays();
    let display = if displays.len() == 1 {
        displays.first()
    } else {
        displays
            .iter()
            .find(|display| display.pixels == pixels && display.position == position)
            .or_else(|| displays.iter().find(|display| display.pixels == pixels))
    };
    display.and_then(|display| display.native)
}

fn displays() -> Vec<Display> {
    let mut ids = [0 as DisplayId; 16];
    let mut count = 0u32;
    // SAFETY: the list has room for as many displays as it is told; the
    // count says how many were written.
    let listed = unsafe { CGGetActiveDisplayList(ids.len() as u32, ids.as_mut_ptr(), &mut count) };
    if listed != 0 {
        return Vec::new();
    }
    ids[..(count as usize).min(ids.len())]
        .iter()
        .filter_map(|&id| display(id))
        .collect()
}

fn display(id: DisplayId) -> Option<Display> {
    // SAFETY: a copied mode is released once read; a null one is not used.
    let (pixels, points) = unsafe {
        let mode = CGDisplayCopyDisplayMode(id);
        if mode.is_null() {
            return None;
        }
        let pixels = UVec2::new(
            CGDisplayModeGetPixelWidth(mode) as u32,
            CGDisplayModeGetPixelHeight(mode) as u32,
        );
        let points = CGDisplayModeGetWidth(mode);
        CGDisplayModeRelease(mode);
        (pixels, points)
    };
    // SAFETY: plain values of an active display.
    let bounds = unsafe { CGDisplayBounds(id) };
    let scale = if points > 0 {
        pixels.x as f64 / points as f64
    } else {
        1.0
    };
    let position = IVec2::new(
        (bounds.origin.x * scale).round() as i32,
        (bounds.origin.y * scale).round() as i32,
    );
    Some(Display {
        pixels,
        position,
        native: native_mode(id),
    })
}

/// The pixels of the display's mode marked as the panel's own; or, with
/// none marked, of its largest mode. Only modes drawn one pixel to a point
/// count.
fn native_mode(id: DisplayId) -> Option<UVec2> {
    // SAFETY: the modes in the array belong to it (not released one by
    // one); the array, copied, is released once read.
    unsafe {
        let modes = CGDisplayCopyAllDisplayModes(id, std::ptr::null());
        if modes.is_null() {
            return None;
        }
        let mut marked = None;
        let mut largest: Option<UVec2> = None;
        for index in 0..CFArrayGetCount(modes) {
            let mode = CFArrayGetValueAtIndex(modes, index);
            if mode.is_null() {
                continue;
            }
            let pixels = UVec2::new(
                CGDisplayModeGetPixelWidth(mode) as u32,
                CGDisplayModeGetPixelHeight(mode) as u32,
            );
            // A HiDPI mode has more pixels than points: never the panel's.
            if CGDisplayModeGetWidth(mode) != pixels.x as usize {
                continue;
            }
            let more = |seen: &Option<UVec2>| {
                seen.is_none_or(|seen| pixels.x * pixels.y > seen.x * seen.y)
            };
            if CGDisplayModeGetIOFlags(mode) & NATIVE_FLAG != 0 && more(&marked) {
                marked = Some(pixels);
            }
            if more(&largest) {
                largest = Some(pixels);
            }
        }
        CFRelease(modes);
        marked.or(largest)
    }
}
