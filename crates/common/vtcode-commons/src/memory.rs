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
#[expect(
    clippy::cast_possible_truncation,
    reason = "The macOS Mach message count is defined as the platform ABI's bounded integer type."
)]
#[allow(
    deprecated,
    unsafe_code,
    unused_qualifications,
    reason = "Intentional compatibility, platform, or test-only suppression."
)] // libc::mach_task_self is deprecated; qualification is required here
pub fn resident_set_size_mb() -> Option<f64> {
    // SAFETY: `mach_task_basic_info` is a plain old data struct; zeroing it
    // produces a valid (all-zero) starting value before `task_info` fills it.
    let mut info: libc::mach_task_basic_info = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<libc::mach_task_basic_info>() / std::mem::size_of::<libc::integer_t>())
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
            std::ptr::addr_of_mut!(info).cast::<libc::integer_t>(),
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
#[allow(
    unsafe_code,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub fn resident_set_size_mb() -> Option<f64> {
    let contents = std::fs::read_to_string("/proc/self/statm").ok()?;
    let field = contents.split_whitespace().nth(1)?;
    let pages: f64 = field.parse().ok()?;
    // SAFETY: `_SC_PAGESIZE` is a compile-time constant selector passed by value.
    // `sysconf` only reads the selector and returns a `c_long`; it performs no
    // mutable aliasing against process memory and has no preconditions on this
    // input. The result is a stable system constant for the process lifetime.
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
    sample_peak_rss_mb_while(duration, poll_interval, || true)
}

/// Like [`sample_peak_rss_mb`], but stops early once `should_continue` returns
/// `false`. The predicate is checked before each sample, so a caller measuring a
/// burst can release the sampler as soon as the burst ends instead of waiting out
/// a worst-case timeout. Intended for blocking threads (e.g. `spawn_blocking`):
/// each poll is a syscall followed by `thread::sleep`, which must not run on a
/// Tokio worker.
pub fn sample_peak_rss_mb_while(
    duration: Duration,
    poll_interval: Duration,
    mut should_continue: impl FnMut() -> bool,
) -> f64 {
    let start = std::time::Instant::now();
    let mut peak: f64 = 0.0;
    while should_continue() && start.elapsed() < duration {
        peak = peak.max(sample_rss_mb());
        std::thread::sleep(poll_interval);
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peak_sampler_stops_before_first_sample_when_predicate_is_false() {
        let peak = sample_peak_rss_mb_while(Duration::from_secs(60), Duration::from_millis(1), || false);
        assert_eq!(peak, 0.0);
    }

    #[test]
    fn peak_sampler_observes_every_iteration_until_predicate_flips() {
        let mut polls = 0;
        let peak = sample_peak_rss_mb_while(Duration::from_secs(60), Duration::from_millis(1), || {
            polls += 1;
            polls <= 3
        });
        // Asymmetric predicate: three `true` answers, then `false`; the loop must
        // not take a fourth sample even though the duration has not elapsed.
        assert_eq!(polls, 4);
        assert!(peak >= 0.0);
    }
}
