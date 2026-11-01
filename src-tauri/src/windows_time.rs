//! V8 ICUTimezoneCache uses ICU, including FORMER for both gaps and overlaps.
//! Windows ships a system ICU C API; load it dynamically to avoid a compiler/CRT-specific
//! import library. Its tzdata version can differ from Electron's bundled ICU (see report).
use std::{ffi::c_void, sync::OnceLock};
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
type Cal = *mut c_void;
struct Icu {
    open: unsafe extern "C" fn(*const u16, i32, *const u8, i32, *mut i32) -> Cal,
    close: unsafe extern "C" fn(Cal),
    set: unsafe extern "C" fn(Cal, f64, *mut i32),
    get: unsafe extern "C" fn(Cal, i32, *mut i32) -> i32,
    zone: unsafe extern "C" fn(Cal, *mut u16, i32, *mut i32) -> i32,
    local: Option<unsafe extern "C" fn(Cal, i32, i32, *mut i32, *mut i32, *mut i32)>,
    canonical: unsafe extern "C" fn(*const u16, i32, *mut u16, i32, *mut i8, *mut i32) -> i32,
}
fn icu() -> Option<&'static Icu> {
    static API: OnceLock<Option<Icu>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let mut dll = std::ptr::null_mut();
        for name in ["icu.dll", "icuin.dll"] {
            let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
            dll = LoadLibraryExW(
                name.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_SYSTEM32,
            );
            if !dll.is_null() {
                break;
            }
        }
        if dll.is_null() {
            return None;
        }
        // The library remains loaded for the lifetime of the cached function pointers.
        macro_rules! sym {
            ($name:literal) => {
                std::mem::transmute(GetProcAddress(dll, concat!($name, "\0").as_ptr())?)
            };
        }
        let local =
            GetProcAddress(dll, c"ucal_getTimeZoneOffsetFromLocal".as_ptr().cast()).map(|p| {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "C" fn(Cal, i32, i32, *mut i32, *mut i32, *mut i32),
                >(p)
            });
        Some(Icu {
            local,
            open: sym!("ucal_open"),
            close: sym!("ucal_close"),
            set: sym!("ucal_setMillis"),
            get: sym!("ucal_get"),
            zone: sym!("ucal_getTimeZoneID"),
            canonical: sym!("ucal_getCanonicalTimeZoneID"),
        })
    })
    .as_ref()
}
fn with_calendar<T>(f: impl FnOnce(&Icu, Cal) -> T) -> Option<T> {
    let api = icu()?;
    let tz = std::env::var("TZ")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| s.encode_utf16().collect::<Vec<_>>());
    let (ptr, len) = tz
        .as_ref()
        .map_or((std::ptr::null(), 0), |s| (s.as_ptr(), s.len() as i32));
    let mut status = 0;
    unsafe {
        let cal = (api.open)(ptr, len, c"en_US_POSIX".as_ptr().cast(), 1, &mut status);
        if status > 0 || cal.is_null() {
            return None;
        }
        let result = f(api, cal);
        (api.close)(cal);
        Some(result)
    }
}
fn offset(api: &Icu, cal: Cal, ms: i64) -> i64 {
    let mut status = 0;
    unsafe {
        (api.set)(cal, ms as f64, &mut status);
        let raw = (api.get)(cal, 15, &mut status); // UCAL_ZONE_OFFSET
        let dst = (api.get)(cal, 16, &mut status); // UCAL_DST_OFFSET
        if status > 0 {
            0
        } else {
            i64::from(raw) + i64::from(dst)
        }
    }
}
pub fn utc_offset_ms(ms: i64) -> i64 {
    with_calendar(|api, cal| offset(api, cal, ms)).unwrap_or(0)
}
pub fn local_offset_ms(ms: i64) -> i64 {
    with_calendar(|api, cal| {
        if let Some(local) = api.local {
            let (mut status, mut raw, mut dst) = (0, 0, 0);
            unsafe {
                (api.set)(cal, ms as f64, &mut status);
                local(cal, 4, 4, &mut raw, &mut dst, &mut status); // UCAL_TZ_LOCAL_FORMER
            }
            if status <= 0 {
                return i64::from(raw) + i64::from(dst);
            }
        }
        // ICU <69 lacks the C API. Find offsets either side of a transition; test the UTC candidates. During an
        // overlap select the earlier instant (larger offset); in a gap use the former
        // offset. Covers IANA's transitions, including 24-hour date-line jumps.
        let before = offset(api, cal, ms - 172_800_000);
        let after = offset(api, cal, ms + 172_800_000);
        [before, after]
            .into_iter()
            .filter(|off| offset(api, cal, ms - off) == *off)
            .max()
            .unwrap_or(before)
    })
    .unwrap_or(0)
}
pub fn time_zone() -> String {
    with_calendar(|api, cal| unsafe {
        let mut id = [0u16; 256];
        let mut status = 0;
        let len = (api.zone)(cal, id.as_mut_ptr(), id.len() as i32, &mut status);
        if status > 0 {
            return "UTC".into();
        }
        let mut canonical = [0u16; 256];
        let mut system = 0;
        let n = (api.canonical)(
            id.as_ptr(),
            len,
            canonical.as_mut_ptr(),
            canonical.len() as i32,
            &mut system,
            &mut status,
        );
        let zone = if status <= 0 {
            String::from_utf16_lossy(&canonical[..n as usize])
        } else {
            String::from_utf16_lossy(&id[..len as usize])
        };
        if matches!(zone.as_str(), "Etc/UTC" | "Etc/GMT") {
            "UTC".into()
        } else {
            zone
        }
    })
    .unwrap_or_else(|| "UTC".into())
}
