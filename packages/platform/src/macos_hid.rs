//! Apple Silicon thermal sensors via IOKit's `IOHIDEventSystemClient`.
//!
//! The SMC does not expose CPU/GPU **die** temperatures on Apple Silicon; those
//! live behind the (private) IOHID temperature-sensor API that Activity-Monitor-
//! style tools use. We match HID services on the Apple-vendor temperature usage
//! page and read each one's temperature event.
//!
//! These IOKit functions are private (not in public headers) but exported by
//! `IOKit.framework`, so we declare them ourselves. Each call creates a client,
//! reads all sensors, and releases everything (no cached CF state — keeps the
//! provider `Send + Sync`).

use std::os::raw::c_void;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use core_foundation::base::{kCFAllocatorDefault, CFAllocatorRef, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
use core_foundation_sys::base::CFRelease;
use core_foundation_sys::dictionary::CFDictionaryRef;
use core_foundation_sys::string::CFStringRef;

type ClientRef = *mut c_void;
type ServiceRef = *mut c_void;
type EventRef = *mut c_void;

/// `kIOHIDEventTypeTemperature`.
const TEMP_TYPE: i64 = 15;
/// `kHIDPage_AppleVendor`.
const PAGE: i32 = 0xff00;
/// Apple-vendor temperature-sensor usage.
const USAGE_TEMP: i32 = 5;
/// A regular headline read and an expanded raw-sensor read can start on the
/// same menu-bar tick. Sharing that one hardware sample avoids rebuilding the
/// private IOHID client two or three times without making the 2–3 second UI
/// refresh cadence any less current.
const TEMP_CACHE_TTL: Duration = Duration::from_millis(900);
type CachedTemps = Option<(Instant, Vec<(String, f32)>)>;
static TEMP_CACHE: OnceLock<Mutex<CachedTemps>> = OnceLock::new();

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreate(alloc: CFAllocatorRef) -> ClientRef;
    fn IOHIDEventSystemClientSetMatching(client: ClientRef, matching: CFDictionaryRef) -> i32;
    fn IOHIDEventSystemClientCopyServices(client: ClientRef) -> CFArrayRef;
    fn IOHIDServiceClientCopyProperty(service: ServiceRef, key: CFStringRef) -> CFStringRef;
    fn IOHIDServiceClientCopyEvent(service: ServiceRef, ev: i64, opts: i64, t: i64) -> EventRef;
    fn IOHIDEventGetFloatValue(event: EventRef, field: i64) -> f64;
}

/// Read all IOHID temperature sensors as `(name, °C)` pairs.
pub fn read_temps() -> Vec<(String, f32)> {
    let cache = TEMP_CACHE.get_or_init(|| Mutex::new(None));
    let mut cached = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let now = Instant::now();
    if let Some(values) = fresh_cached_temps(&cached, now) {
        return values;
    }

    // Keep the lock during the hardware call. Readers run only on background
    // threads, and serialization is what prevents two simultaneous cache
    // misses from constructing duplicate IOHID clients.
    let values = read_temps_uncached();
    *cached = Some((Instant::now(), values.clone()));
    values
}

fn fresh_cached_temps(cached: &CachedTemps, now: Instant) -> Option<Vec<(String, f32)>> {
    cached
        .as_ref()
        .filter(|(sampled_at, _)| now.saturating_duration_since(*sampled_at) < TEMP_CACHE_TTL)
        .map(|(_, values)| values.clone())
}

fn read_temps_uncached() -> Vec<(String, f32)> {
    let mut out = Vec::new();
    unsafe {
        let matching = CFDictionary::from_CFType_pairs(&[
            (
                CFString::new("PrimaryUsagePage").as_CFType(),
                CFNumber::from(PAGE).as_CFType(),
            ),
            (
                CFString::new("PrimaryUsage").as_CFType(),
                CFNumber::from(USAGE_TEMP).as_CFType(),
            ),
        ]);

        let client = IOHIDEventSystemClientCreate(kCFAllocatorDefault);
        if client.is_null() {
            return out;
        }
        IOHIDEventSystemClientSetMatching(client, matching.as_concrete_TypeRef());

        let services = IOHIDEventSystemClientCopyServices(client);
        if !services.is_null() {
            let key = CFString::new("Product");
            let count = CFArrayGetCount(services);
            for i in 0..count {
                let svc = CFArrayGetValueAtIndex(services, i) as ServiceRef;
                if svc.is_null() {
                    continue;
                }
                let event = IOHIDServiceClientCopyEvent(svc, TEMP_TYPE, 0, 0);
                if event.is_null() {
                    continue;
                }
                let temp = IOHIDEventGetFloatValue(event, TEMP_TYPE << 16) as f32;
                CFRelease(event as *const c_void);
                if !(1.0..=130.0).contains(&temp) {
                    continue;
                }
                let name_ref = IOHIDServiceClientCopyProperty(svc, key.as_concrete_TypeRef());
                let name = if name_ref.is_null() {
                    String::new()
                } else {
                    CFString::wrap_under_create_rule(name_ref).to_string()
                };
                out.push((name, temp));
            }
            CFRelease(services as *const c_void);
        }
        CFRelease(client as *const c_void);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    #[test]
    fn short_lived_hid_cache_reuses_only_fresh_samples() {
        let sampled_at = Instant::now();
        let cached = Some((sampled_at, vec![("CPU die".to_string(), 61.5)]));

        assert_eq!(
            super::fresh_cached_temps(&cached, sampled_at + Duration::from_millis(500)),
            Some(vec![("CPU die".to_string(), 61.5)])
        );
        assert_eq!(
            super::fresh_cached_temps(
                &cached,
                sampled_at + super::TEMP_CACHE_TTL + Duration::from_millis(1)
            ),
            None
        );
    }

    #[test]
    #[ignore = "prints local IOHID temperature sensor names for manual debugging"]
    fn print_hid_temperature_sensors() {
        for (name, temp) in super::read_temps() {
            println!("{temp:6.2}  {name}");
        }
    }
}
