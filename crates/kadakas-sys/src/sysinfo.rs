//! Facts about the computer, without a dependency: today the physical memory.

/// The physical memory of the computer in bytes, or None when the system does not say.
///
/// macOS: `sysctl hw.memsize`. Linux and Android: `MemTotal` in `/proc/meminfo`. Windows:
/// `GlobalMemoryStatusEx`. Other targets (the browser): None. A caller must treat None as "unknown"
/// and never as "small".
pub fn physical_memory() -> Option<u64> {
    imp::physical_memory().filter(|&b| b > 0)
}

/// The bytes of `MemTotal` in the text of `/proc/meminfo` (the line is `MemTotal:  16384 kB`).
/// The text comes from the kernel, but the parser takes nothing for granted.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let rest = text
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))?
        .trim();
    let mut parts = rest.split_whitespace();
    let number: u64 = parts.next()?.parse().ok()?;
    let unit: u64 = match parts.next() {
        None => 1,
        Some(u) if u.eq_ignore_ascii_case("kb") => 1024,
        Some(u) if u.eq_ignore_ascii_case("mb") => 1024 * 1024,
        Some(_) => return None,
    };
    number.checked_mul(unit)
}

#[cfg(target_os = "macos")]
mod imp {
    pub fn physical_memory() -> Option<u64> {
        let mut value: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        // SAFETY: the name is a C string; `value` and `size` match what `hw.memsize` returns (a
        // 64-bit number).
        let r = unsafe {
            libc::sysctlbyname(
                c"hw.memsize".as_ptr(),
                (&mut value as *mut u64).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (r == 0 && size == std::mem::size_of::<u64>()).then_some(value)
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
mod imp {
    pub fn physical_memory() -> Option<u64> {
        super::parse_meminfo(&std::fs::read_to_string("/proc/meminfo").ok()?)
    }
}

#[cfg(windows)]
mod imp {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    pub fn physical_memory() -> Option<u64> {
        let mut m = MemoryStatusEx {
            length: std::mem::size_of::<MemoryStatusEx>() as u32,
            memory_load: 0,
            total_phys: 0,
            avail_phys: 0,
            total_page_file: 0,
            avail_page_file: 0,
            total_virtual: 0,
            avail_virtual: 0,
            avail_extended_virtual: 0,
        };
        // SAFETY: `m` has the layout of MEMORYSTATUSEX and its length field is set.
        let ok = unsafe { GlobalMemoryStatusEx(&mut m) };
        (ok != 0).then_some(m.total_phys)
    }
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "linux",
    target_os = "android",
    windows
)))]
mod imp {
    pub fn physical_memory() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo_text_is_parsed_in_bytes() {
        assert_eq!(
            parse_meminfo("MemFree: 5 kB\nMemTotal:       16384 kB\nBuffers: 1 kB\n"),
            Some(16_384 * 1024)
        );
        assert_eq!(parse_meminfo("MemTotal: 2 MB\n"), Some(2 * 1024 * 1024));
        assert_eq!(parse_meminfo("MemTotal: 777\n"), Some(777));
    }

    #[test]
    fn bad_meminfo_text_gives_none() {
        for text in [
            "",
            "MemFree: 5 kB\n",
            "MemTotal:\n",
            "MemTotal: lots kB\n",
            "MemTotal: 5 pages\n",
            "MemTotal: -5 kB\n",
            // The product does not fit in 64 bits.
            "MemTotal: 18446744073709551615 kB\n",
        ] {
            assert_eq!(parse_meminfo(text), None, "{text:?}");
        }
    }

    #[test]
    fn this_computer_reports_its_memory_or_nothing() {
        // On the platforms that we ship, the answer exists; elsewhere it is None. Either way, it
        // is never zero.
        if let Some(b) = physical_memory() {
            assert!(b >= 64 * 1024 * 1024, "{b}");
        }
        if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
            assert!(physical_memory().is_some());
        }
    }
}
