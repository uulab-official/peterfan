//! Apple Silicon GPU active-residency via the private **IOReport** API.
//!
//! **Status: experimental, off by default** (`experimental-gpu` feature).
//!
//! macOS exposes no public GPU-usage API. The plumbing here is correct and
//! reusable — it subscribes to the `GPU Stats` IOReport group, samples twice,
//! diffs, and reads the `GPUPH` power-state residency histogram (one `OFF`
//! idle state plus active states `P1..P15`):
//!
//! ```text
//! usage% = (total_residency − OFF_residency) / total_residency × 100
//! ```
//!
//! **Why it's not shipped as a metric:** this "not fully off" residency does
//! *not* match what Activity Monitor reports as GPU %. At true desktop idle the
//! GPU still spends ~half its time in a low active P-state servicing display
//! compositing, so this formula reads ~50–70% when Activity Monitor shows
//! single digits. Rather than present a number that visibly disagrees with the
//! OS, we keep this behind a feature flag until we can derive a frequency- or
//! power-weighted figure that matches (and can be verified without root). The
//! `extern "C"` plumbing below is good reference for that future work.

#![allow(non_snake_case, non_upper_case_globals)]

use std::ffi::c_void;
use std::os::raw::c_int;

use core_foundation::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::dictionary::{
    CFDictionaryGetValueIfPresent, CFDictionaryRef, CFMutableDictionaryRef,
};
use core_foundation::string::{CFString, CFStringRef};

type IOReportSubscriptionRef = *const c_void;

// The IOReport symbols live in /usr/lib/libIOReport.dylib (a plain dylib, not
// a framework), so link it by library name.
#[link(name = "IOReport")]
extern "C" {
    fn IOReportCopyChannelsInGroup(
        group: CFStringRef,
        subgroup: CFStringRef,
        a: u64,
        b: u64,
        c: u64,
    ) -> CFMutableDictionaryRef;
    fn IOReportCreateSubscription(
        a: *const c_void,
        desired_channels: CFMutableDictionaryRef,
        subbed_channels: *mut CFMutableDictionaryRef,
        channel_id: u64,
        b: CFTypeRef,
    ) -> IOReportSubscriptionRef;
    fn IOReportCreateSamples(
        subscription: IOReportSubscriptionRef,
        desired_channels: CFMutableDictionaryRef,
        b: CFTypeRef,
    ) -> CFDictionaryRef;
    fn IOReportCreateSamplesDelta(
        prev: CFDictionaryRef,
        cur: CFDictionaryRef,
        b: CFTypeRef,
    ) -> CFDictionaryRef;

    fn IOReportChannelGetGroup(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportChannelGetChannelName(ch: CFDictionaryRef) -> CFStringRef;
    fn IOReportStateGetCount(ch: CFDictionaryRef) -> c_int;
    fn IOReportStateGetNameForIndex(ch: CFDictionaryRef, index: c_int) -> CFStringRef;
    fn IOReportStateGetResidency(ch: CFDictionaryRef, index: c_int) -> i64;
}

/// Wrap a borrowed `CFStringRef` (Get-rule, not owned) into a Rust `String`.
unsafe fn cfstr(s: CFStringRef) -> String {
    if s.is_null() {
        return String::new();
    }
    CFString::wrap_under_get_rule(s).to_string()
}

/// One GPU performance-state residency sample, summed across the GPU channel.
struct Residency {
    total: i64,
    idle: i64,
}

/// Read the residency delta for the "GPU Stats" group between two samples taken
/// `interval_ms` apart. Returns `None` if IOReport is unavailable or reports no
/// GPU performance-state channel.
fn sample(interval_ms: u64) -> Option<Residency> {
    unsafe {
        let group = CFString::new("GPU Stats");
        let channels =
            IOReportCopyChannelsInGroup(group.as_concrete_TypeRef(), std::ptr::null(), 0, 0, 0);
        if channels.is_null() {
            return None;
        }

        let mut subbed: CFMutableDictionaryRef = std::ptr::null_mut();
        let sub = IOReportCreateSubscription(
            std::ptr::null(),
            channels,
            &mut subbed,
            0,
            std::ptr::null(),
        );
        if sub.is_null() {
            CFRelease(channels as CFTypeRef);
            return None;
        }

        let s1 = IOReportCreateSamples(sub, channels, std::ptr::null());
        std::thread::sleep(std::time::Duration::from_millis(interval_ms.max(50)));
        let s2 = IOReportCreateSamples(sub, channels, std::ptr::null());

        let res = if s1.is_null() || s2.is_null() {
            None
        } else {
            let delta = IOReportCreateSamplesDelta(s1, s2, std::ptr::null());
            let r = delta_residency(delta);
            if !delta.is_null() {
                CFRelease(delta as CFTypeRef);
            }
            r
        };

        if !s1.is_null() {
            CFRelease(s1 as CFTypeRef);
        }
        if !s2.is_null() {
            CFRelease(s2 as CFTypeRef);
        }
        CFRelease(sub as CFTypeRef);
        CFRelease(channels as CFTypeRef);
        res
    }
}

/// Walk the `IOReportChannels` array of a delta sample, summing residency for
/// the GPU performance-state channel.
unsafe fn delta_residency(delta: CFDictionaryRef) -> Option<Residency> {
    if delta.is_null() {
        return None;
    }
    let key = CFString::new("IOReportChannels");
    let mut arr_ptr: *const c_void = std::ptr::null();
    if CFDictionaryGetValueIfPresent(
        delta,
        key.as_concrete_TypeRef() as *const c_void,
        &mut arr_ptr,
    ) == 0
        || arr_ptr.is_null()
    {
        return None;
    }
    let arr = arr_ptr as CFArrayRef;
    let n = CFArrayGetCount(arr);

    let mut total = 0i64;
    let mut idle = 0i64;
    let mut found = false;

    for i in 0..n {
        let ch = CFArrayGetValueAtIndex(arr, i) as CFDictionaryRef;
        if ch.is_null() {
            continue;
        }
        let group = cfstr(IOReportChannelGetGroup(ch));
        if group != "GPU Stats" {
            continue;
        }
        // `GPUPH` is the hardware GPU power-state residency histogram: one
        // "OFF" (idle) state plus active states P1..P15. Active residency is
        // everything that is not OFF. (This is the channel powermetrics, asitop
        // and macmon all read for Apple Silicon GPU utilization.)
        let name = cfstr(IOReportChannelGetChannelName(ch));
        if name != "GPUPH" {
            continue;
        }
        let count = IOReportStateGetCount(ch);
        for s in 0..count {
            let state = cfstr(IOReportStateGetNameForIndex(ch, s)).to_uppercase();
            let r = IOReportStateGetResidency(ch, s).max(0);
            total += r;
            if state == "OFF" || state == "IDLE_OFF" {
                idle += r;
            }
        }
        found = true;
    }

    if found && total > 0 {
        Some(Residency { total, idle })
    } else {
        None
    }
}

/// GPU active-residency utilization in percent (0..=100), or `None` if the
/// IOReport GPU performance-state channel is unavailable.
pub fn gpu_usage_percent() -> Option<f32> {
    let r = sample(200)?;
    let busy = (r.total - r.idle).max(0) as f64;
    Some((busy / r.total as f64 * 100.0) as f32)
}
