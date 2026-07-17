//! Resident Set Size (RSS) sampling for memory diagnostics.
//!
//! Used by the allocator benchmark (`vtcode bench-allocator`) to measure whether
//! the global allocator returns memory to the OS after bursty/sparse workloads.
//! Unlike `performance_profiler::get_memory_usage_mb` (Linux `/proc` only, fake
//! fallback on macOS), this returns a real value on every supported platform.
use std::time::Duration;

/// Returns the current process Resident Set Size in **megabytes**, or `None` if
/// it cannot be determined on the current platform.
#[cfg(target_os = "macos")]
#[allow(deprecated, unsafe_code, unused_qualifications)] // libc::mach_task_self is deprecated; qualification is required here
pub fn resident_set_size_mb() -> Option<f64> {
    // SAFETY: `mach_task_basic_info` is a plain old data struct; zeroing it
    // produces a valid (all-zero) starting value before `task_info` fills it.
    let mut info: libc::mach_task_basic_info = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<libc::mach_task_basic_info>()
        / std::mem::size_of::<libc::integer_t>())
        as libc::mach_msg_type_number_t;
    // SAFETY: `mach_task_self()` returns a send-right to the current task with
    // no preconditions; it cannot fail to produce a valid port name.
    let task = unsafe { libc::mach_task_self() };
    // SAFETY: `task` is our own task port; `info` and `count` are valid
    // out-pointers of the expected size, and `task_info` only writes them on
    // success.
    let ret = unsafe {
        libc::task_info(
            task,
            libc::MACH_TASK_BASIC_INFO,
            &mut info as *mut _ as *mut libc::integer_t,
            &mut count,
        )
    };
    if ret != libc::KERN_SUCCESS {
        return None;
    }
    Some(info.resident_size as f64 / (1024.0 * 1024.0))
}

/// Returns the current process Resident Set Size in **megabytes**, or `None` if
/// it cannot be determined on the current platform.
#[cfg(target_os = "linux")]
pub fn resident_set_size_mb() -> Option<f64> {
    let contents = std::fs::read_to_string("/proc/self/statm").ok()?;
    let field = contents.split_whitespace().nth(1)?;
    let pages: f64 = field.parse().ok()?;
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as f64;
    Some(pages * page_size / (1024.0 * 1024.0))
}

/// Fallback for unsupported platforms.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn resident_set_size_mb() -> Option<f64> {
    None
}

/// Sample RSS once and return the value in MB (0.0 if unavailable).
pub fn sample_rss_mb() -> f64 {
    resident_set_size_mb().unwrap_or(0.0)
}

/// Sample RSS repeatedly, returning the maximum observed value in MB.
/// Useful for capturing peak memory during a burst of activity.
pub fn sample_peak_rss_mb(duration: Duration, poll_interval: Duration) -> f64 {
    let start = std::time::Instant::now();
    let mut peak = 0.0;
    while start.elapsed() < duration {
        let v = sample_rss_mb();
        if v > peak {
            peak = v;
        }
        std::thread::sleep(poll_interval);
    }
    peak
}
