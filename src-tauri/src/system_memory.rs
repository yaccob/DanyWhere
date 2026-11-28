//! DanyWhere: how much memory this computer has, for the post-processing
//! setup: a language model on this computer is offered from 16 GB on
//! (sidecar note `2026-11-26_1840_nachbearbeitung-einrichtung-mit-fragen`,
//! section 4; `post_process_setup`).

/// The computer's physical memory in bytes, or 0 when it can't be read.
#[cfg(target_os = "macos")]
pub fn system_memory_bytes() -> u64 {
    let mut size: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    // SAFETY: `hw.memsize` is a 64-bit integer; `size` and `len` are valid
    // for writes of that size, no new value is passed.
    let result = unsafe {
        libc::sysctlbyname(
            c"hw.memsize".as_ptr(),
            (&mut size as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 {
        size
    } else {
        0
    }
}

/// The computer's physical memory in bytes, or 0 when it can't be read.
#[cfg(target_os = "linux")]
pub fn system_memory_bytes() -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|meminfo| mem_total_from_meminfo(&meminfo))
        .unwrap_or(0)
}

/// The computer's physical memory in bytes, or 0 when it can't be read.
#[cfg(windows)]
pub fn system_memory_bytes() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: `status` is a MEMORYSTATUSEX with its length set, as the call
    // requires.
    match unsafe { GlobalMemoryStatusEx(&mut status) } {
        Ok(()) => status.ullTotalPhys,
        Err(_) => 0,
    }
}

/// The computer's physical memory in bytes, or 0 when it can't be read.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn system_memory_bytes() -> u64 {
    0
}

/// `MemTotal` of Linux's `/proc/meminfo` in bytes (the file gives kB).
#[cfg_attr(not(any(target_os = "linux", test)), allow(dead_code))]
fn mem_total_from_meminfo(meminfo: &str) -> Option<u64> {
    let line = meminfo
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?;
    let kilobytes: u64 = line.trim().strip_suffix("kB")?.trim().parse().ok()?;
    kilobytes.checked_mul(1024)
}

/// The computer's physical memory in bytes, as the post-processing setup
/// shows it (`get_post_process_setup_status` has it too).
#[tauri::command]
#[specta::specta]
pub fn get_system_memory_bytes() -> u64 {
    system_memory_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_computer_has_memory() {
        // Every machine the tests run on has more than a gigabyte.
        assert!(system_memory_bytes() > 1024 * 1024 * 1024);
    }

    #[test]
    fn linux_memory_is_read_from_meminfo() {
        let meminfo = "MemTotal:       16303428 kB\nMemFree:         1234567 kB\nMemAvailable:    9876543 kB\n";
        assert_eq!(mem_total_from_meminfo(meminfo), Some(16_303_428 * 1024));
        assert_eq!(mem_total_from_meminfo("MemFree: 12 kB\n"), None);
        assert_eq!(mem_total_from_meminfo("MemTotal: lots kB\n"), None);
    }
}
